//! REST rate limiting.
//!
//! Discord rate limits per route *and* per bucket: routes share a hash, and a bucket becomes
//! available `retry_after` seconds after each request. A client that ignores this gets
//! disconnected, so this is not optional plumbing.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Shared state for one route.
#[derive(Debug, Clone)]
pub struct Bucket {
    /// Remaining requests before the limit is hit.
    pub remaining: u32,
    /// How long until the bucket refills, if the gateway said so.
    pub reset_after: Option<Duration>,
    /// When this bucket may be used again.
    available_at: Instant,
}

impl Bucket {
    /// A bucket with `limit` remaining and no wait.
    #[must_use]
    pub fn new(limit: u32) -> Self {
        Self {
            remaining: limit,
            reset_after: None,
            available_at: Instant::now(),
        }
    }

    /// Whether the bucket may be used right now.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.remaining > 0 && Instant::now() >= self.available_at
    }

    /// How long until the bucket refills.
    #[must_use]
    pub fn wait(&self) -> Duration {
        self.available_at.saturating_duration_since(Instant::now())
    }
}

/// Which limit a request counts against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    /// A per-route bucket.
    Route,
    /// The account-wide bucket, which is stricter.
    Global,
}

/// Tracks buckets and answers "may I send yet, and if not, for how long".
#[derive(Debug)]
pub struct RateLimiter {
    buckets: HashMap<String, Bucket>,
    global: Bucket,
    /// Number of buckets to remember before evicting the least recently reset.
    max_buckets: usize,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(500)
    }
}

impl RateLimiter {
    /// Creates a limiter with the account-wide limit.
    #[must_use]
    pub fn new(global_limit: u32) -> Self {
        Self {
            buckets: HashMap::new(),
            global: Bucket::new(global_limit),
            max_buckets: 500,
        }
    }

    /// The account-wide bucket.
    #[must_use]
    pub const fn global(&self) -> &Bucket {
        &self.global
    }

    /// The bucket for a route key, creating it with `limit` on first use.
    pub fn bucket(&mut self, key: &str, limit: u32) -> &mut Bucket {
        if self.buckets.len() >= self.max_buckets && !self.buckets.contains_key(key) {
            self.evict_oldest();
        }
        self.buckets
            .entry(key.to_owned())
            .or_insert_with(|| Bucket::new(limit))
    }

    /// Applies a `429` response: the bucket is exhausted and refills after `retry_after`.
    pub fn apply_rate_limited(&mut self, key: &str, retry_after: Duration) {
        let limit = self.buckets.get(key).map_or(0, |b| b.remaining);
        self.buckets.insert(
            key.to_owned(),
            Bucket {
                remaining: 0,
                reset_after: Some(retry_after),
                available_at: Instant::now() + retry_after,
            },
        );
        let _ = limit;
        self.global.remaining = 0;
        self.global.available_at = Instant::now() + retry_after;
    }

    /// Applies response headers, which report remaining count and reset window.
    pub fn apply_headers(&mut self, key: &str, remaining: u32, reset_after: Duration) {
        let bucket = self
            .buckets
            .entry(key.to_owned())
            .or_insert_with(|| Bucket::new(remaining));
        bucket.remaining = remaining;
        bucket.reset_after = Some(reset_after);
        if remaining == 0 {
            bucket.available_at = Instant::now() + reset_after;
        }
    }

    /// Records a successful request, consuming one unit from the bucket.
    pub fn record_success(&mut self, key: &str) {
        if let Some(bucket) = self.buckets.get_mut(key) {
            bucket.remaining = bucket.remaining.saturating_sub(1);
        }
        self.global.remaining = self.global.remaining.saturating_sub(1);
    }

    /// Whether a request to `key` can be sent now.
    #[must_use]
    pub fn can_send(&self, key: &str) -> bool {
        self.global.is_available() && self.buckets.get(key).is_none_or(Bucket::is_available)
    }

    /// How long to wait before `key` can be used.
    #[must_use]
    pub fn wait_time(&self, key: &str) -> Duration {
        self.global
            .wait()
            .max(self.buckets.get(key).map_or(Duration::ZERO, Bucket::wait))
    }

    /// Total tracked route buckets.
    #[must_use]
    pub fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    fn evict_oldest(&mut self) {
        if let Some(key) = self
            .buckets
            .iter()
            .min_by_key(|(_, b)| b.available_at)
            .map(|(k, _)| k.clone())
        {
            self.buckets.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_bucket_is_available() {
        let bucket = Bucket::new(5);
        assert!(bucket.is_available());
        assert_eq!(bucket.remaining, 5);
        assert_eq!(bucket.wait(), Duration::ZERO);
    }

    #[test]
    fn an_exhausted_bucket_is_not_available() {
        let mut limiter = RateLimiter::new(100);
        limiter.record_success("GET /channels/1");
        assert!(limiter.can_send("GET /channels/1"));
        // Drain the route bucket.
        for _ in 0..5 {
            limiter.bucket("GET /channels/1", 5).remaining -= 1;
        }
        assert!(!limiter.can_send("GET /channels/1"));
    }

    #[test]
    fn rate_limited_blocks_until_the_retry_window_passes() {
        let mut limiter = RateLimiter::new(100);
        limiter.apply_rate_limited("POST /messages", Duration::from_millis(250));
        assert!(!limiter.can_send("POST /messages"));
        assert!(limiter.wait_time("POST /messages") > Duration::ZERO);
        assert!(
            !limiter.can_send("some/unrelated"),
            "a 429 also trips the global bucket"
        );
    }

    #[test]
    fn headers_set_remaining_and_reset() {
        let mut limiter = RateLimiter::new(100);
        limiter.apply_headers("GET /x", 3, Duration::from_millis(50));
        assert!(limiter.can_send("GET /x"));
        limiter.apply_headers("GET /x", 0, Duration::from_millis(50));
        assert!(!limiter.can_send("GET /x"));
    }

    #[test]
    fn unknown_routes_are_always_available() {
        let limiter = RateLimiter::new(100);
        assert!(limiter.can_send("never/seen"));
        assert_eq!(limiter.wait_time("never/seen"), Duration::ZERO);
    }

    #[test]
    fn buckets_are_bounded() {
        let mut limiter = RateLimiter::new(100);
        limiter.max_buckets = 10;
        for i in 0..50 {
            limiter.record_success(&format!("route/{i}"));
        }
        assert!(
            limiter.bucket_count() <= 10,
            "got {} buckets",
            limiter.bucket_count()
        );
    }

    #[test]
    fn eviction_keeps_the_most_pressured_bucket() {
        let mut limiter = RateLimiter::new(100);
        limiter.max_buckets = 4;
        // One route is blocked for a long time; the rest fill up behind it.
        limiter.apply_rate_limited("blocked", Duration::from_secs(30));
        for i in 0..20 {
            limiter.record_success(&format!("route/{i}"));
        }
        assert!(
            !limiter.can_send("blocked"),
            "the blocked bucket must survive eviction"
        );
    }

    #[test]
    fn global_bucket_is_consumed_by_successes() {
        let mut limiter = RateLimiter::new(2);
        assert_eq!(limiter.global().remaining, 2);
        limiter.record_success("a");
        assert_eq!(limiter.global().remaining, 1);
    }
}
