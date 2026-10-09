//! Discord snowflake identifiers.
//!
//! A snowflake packs a creation timestamp, a worker id and an incrementing counter into a single
//! `u64`. Plugins compare, sort and filter ids constantly, and being able to recover *when* an
//! entity was created is what makes efficient "load messages newer than X" queries possible.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds between the Discord epoch (2015-01-01T00:00:00Z) and the Unix epoch.
pub const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

/// A Discord snowflake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Snowflake(pub u64);

impl Snowflake {
    /// The zero id, which Discord uses as "absent" in optional fields.
    pub const ZERO: Self = Self(0);

    /// Wraps a raw value.
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw value.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Whether this is the zero id.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// The creation timestamp, in milliseconds since the Unix epoch.
    #[must_use]
    pub const fn timestamp_ms(self) -> u64 {
        (self.0 >> 22) + DISCORD_EPOCH_MS
    }

    /// The creation timestamp as a [`SystemTime`].
    ///
    /// Returns `None` only if the platform clock cannot represent the value, which for a u64
    /// millisecond count is not reachable in practice.
    #[must_use]
    pub fn timestamp(self) -> Option<SystemTime> {
        UNIX_EPOCH.checked_add(std::time::Duration::from_millis(self.timestamp_ms()))
    }

    /// The worker/process id that minted this snowflake.
    #[must_use]
    pub const fn worker_id(self) -> u8 {
        ((self.0 >> 17) & 0x3f) as u8
    }

    /// The per-worker incrementing counter.
    #[must_use]
    pub const fn increment(self) -> u16 {
        (self.0 & 0x1f_ffff) as u16
    }

    /// Whether `self` is strictly newer than `other`.
    ///
    /// Because the timestamp occupies the high bits, plain integer ordering already sorts by
    /// creation time within a worker. This is the explicit form.
    #[must_use]
    pub const fn is_newer_than(self, other: Self) -> bool {
        self.0 > other.0
    }
}

impl fmt::Display for Snowflake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for Snowflake {
    fn from(raw: u64) -> Self {
        Self(raw)
    }
}

impl From<Snowflake> for u64 {
    fn from(id: Snowflake) -> u64 {
        id.0
    }
}

impl std::str::FromStr for Snowflake {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u64>().map(Self)
    }
}

impl serde::Serialize for Snowflake {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // Snowflakes exceed the 53-bit safe integer range, so they must be serialized as strings
        // for any consumer backed by JavaScript numbers.
        s.serialize_str(&self.0.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for Snowflake {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = Snowflake;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a snowflake as a string or an integer")
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Snowflake, E> {
                Ok(Snowflake(v))
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Snowflake, E> {
                u64::try_from(v)
                    .map(Snowflake)
                    .map_err(serde::de::Error::custom)
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Snowflake, E> {
                v.parse::<u64>()
                    .map(Snowflake)
                    .map_err(serde::de::Error::custom)
            }
        }
        d.deserialize_any(Visitor)
    }
}

/// Builds a snowflake from its parts. Used by tests and by synthetic fixtures.
#[must_use]
pub const fn snowflake_from_parts(timestamp_ms: u64, worker: u8, increment: u16) -> Snowflake {
    let shifted = timestamp_ms.saturating_sub(DISCORD_EPOCH_MS) & 0xffff_ffff_ffff;
    Snowflake((shifted << 22) | ((worker as u64) << 17) | (increment as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_the_timestamp() {
        // A snowflake minted at the Discord epoch must decode to the epoch timestamp.
        let id = snowflake_from_parts(DISCORD_EPOCH_MS, 0, 0);
        assert_eq!(id.timestamp_ms(), DISCORD_EPOCH_MS);

        let later = snowflake_from_parts(DISCORD_EPOCH_MS + 60_000, 0, 0);
        assert_eq!(later.timestamp_ms(), DISCORD_EPOCH_MS + 60_000);
    }

    #[test]
    fn recovers_worker_and_increment() {
        let id = snowflake_from_parts(DISCORD_EPOCH_MS + 5_000, 17, 1_234);
        assert_eq!(id.worker_id(), 17);
        assert_eq!(id.increment(), 1_234);
    }

    #[test]
    fn ordering_matches_creation_time() {
        let a = snowflake_from_parts(DISCORD_EPOCH_MS + 1_000, 0, 0);
        let b = snowflake_from_parts(DISCORD_EPOCH_MS + 2_000, 0, 0);
        assert!(b.is_newer_than(a));
        assert!(!a.is_newer_than(b));
        assert!(a < b);
    }

    #[test]
    fn zero_is_absent() {
        assert!(Snowflake::ZERO.is_zero());
        assert!(!Snowflake::new(1).is_zero());
    }

    #[test]
    fn timestamp_is_a_system_time() {
        let id = snowflake_from_parts(DISCORD_EPOCH_MS, 0, 0);
        let t = id.timestamp().expect("representable");
        let millis = t.duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        assert_eq!(millis, DISCORD_EPOCH_MS);
    }

    #[test]
    fn serializes_as_a_string_to_avoid_javascript_precision_loss() {
        // 2^53 is the first integer JavaScript cannot represent exactly.
        let id = Snowflake(9_007_199_254_740_993);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"9007199254740993\"");
        assert!(serde_json::from_str::<Snowflake>(&json).is_ok());
    }

    #[test]
    fn deserializes_from_string_or_number() {
        assert_eq!(
            serde_json::from_str::<Snowflake>("\"123\"").unwrap(),
            Snowflake(123)
        );
        assert_eq!(
            serde_json::from_str::<Snowflake>("123").unwrap(),
            Snowflake(123)
        );
        assert!(serde_json::from_str::<Snowflake>("\"abc\"").is_err());
    }

    #[test]
    fn parses_and_displays() {
        let id: Snowflake = "175928847299117063".parse().unwrap();
        assert_eq!(id.raw(), 175_928_847_299_117_063);
        assert_eq!(id.to_string(), "175928847299117063");
        assert_eq!(u64::from(id), 175_928_847_299_117_063);
    }
}
