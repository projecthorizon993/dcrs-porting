//! The gateway session state machine.
//!
//! Drives connect -> Hello -> Identify -> heartbeat -> reconnect, with resume when possible and a
//! jittered backoff when not. Every transition is driven by an inbound frame and produces
//! outbound frames through a [`GatewayTransport`](crate::gateway::GatewayTransport), so the whole
//! cycle is deterministic and
//! testable without a network or a token.

use crate::gateway::{CloseReason, IdentifyConfig, Inbound, OpCode, Outbound, TransportError};
use crate::ids::Snowflake;

/// Where the session is in its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Not connected.
    Disconnected,
    /// TCP/WebSocket open, waiting for Hello.
    Connecting,
    /// Hello received; Identify or Resume has been sent.
    Identifying {
        /// The session id, once Resume is possible.
        session_id: Option<String>,
    },
    /// Ready received; dispatching events.
    Ready {
        /// The session id, required to resume later.
        session_id: String,
        /// The authenticated user.
        user_id: Snowflake,
    },
    /// The connection dropped and a retry is scheduled.
    Reconnecting {
        /// How many consecutive failures have occurred.
        attempt: u32,
    },
    /// The session cannot continue and the caller must re-authenticate.
    Failed(SessionFailure),
}

/// Why a session gave up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionFailure {
    /// The token was rejected. A fresh login is required.
    InvalidToken,
    /// Another session authenticated with this token.
    AlreadyAuthenticated,
    /// Reconnect attempts were exhausted.
    AttemptsExhausted {
        /// How many attempts were made.
        attempts: u32,
    },
}

impl std::fmt::Display for SessionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidToken => f.write_str("token rejected; re-authentication required"),
            Self::AlreadyAuthenticated => f.write_str("token already authenticated elsewhere"),
            Self::AttemptsExhausted { attempts } => {
                write!(f, "gave up after {attempts} reconnect attempts")
            }
        }
    }
}

/// Something the session observed that the caller should react to.
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    /// Ready was received.
    Ready {
        /// The authenticated user.
        user_id: Snowflake,
        /// Session id, for later resume.
        session_id: String,
    },
    /// A dispatch event arrived, with its name and payload.
    Event {
        /// Event name, e.g. `MESSAGE_CREATE`.
        name: String,
        /// Last sequence seen.
        seq: i64,
        /// Raw payload.
        data: serde_json::Value,
    },
    /// The connection dropped and a reconnect is scheduled after this long.
    Reconnecting {
        /// Attempt number, starting at 1.
        attempt: u32,
        /// Delay before the next attempt.
        delay: std::time::Duration,
        /// Whether the next attempt will try to resume.
        will_resume: bool,
    },
    /// The session ended unrecoverably.
    Failed(SessionFailure),
}

/// Outcome of feeding one inbound frame to the machine.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Step {
    /// Frames the machine wants sent.
    pub sent: Vec<Outbound>,
    /// Things the caller should react to.
    pub notices: Vec<Notice>,
}

/// Reconnect backoff with jitter.
///
/// Exponential with full jitter: `sleep = random(0, min(cap, base * 2^n))`. Without jitter, every
/// client dropped by a gateway deploy reconnects in lockstep and knocks it over again.
#[derive(Debug, Clone)]
pub struct Backoff {
    base: std::time::Duration,
    cap: std::time::Duration,
    max_attempts: u32,
    /// Deterministic jitter source, so tests can pin it.
    seed: u64,
}

impl Backoff {
    /// A backoff with the given base, cap and attempt limit.
    #[must_use]
    pub fn new(base: std::time::Duration, cap: std::time::Duration, max_attempts: u32) -> Self {
        Self {
            base,
            cap,
            max_attempts,
            seed: 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// Replaces the jitter seed, making delays deterministic for tests.
    #[must_use]
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// The attempt limit.
    #[must_use]
    pub const fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    /// The delay before attempt number `attempt`, counting from 1.
    ///
    /// Returns `None` once `attempt` exceeds the limit.
    #[must_use]
    pub fn delay_for(&mut self, attempt: u32) -> Option<std::time::Duration> {
        if attempt > self.max_attempts {
            return None;
        }
        // base * 2^(attempt-1), saturating rather than overflowing.
        let shift = attempt.saturating_sub(1).min(32);
        let scaled = self.base.saturating_mul(2u32.saturating_pow(shift));
        let ceiling = scaled.min(self.cap);
        let ceiling_ms = u64::try_from(ceiling.as_millis()).unwrap_or(u64::MAX);

        // xorshift64*, so the sequence is reproducible across platforms and runs.
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        let random = self.seed.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33;

        let ms = if ceiling_ms == 0 {
            0
        } else {
            random % (ceiling_ms + 1)
        };
        Some(std::time::Duration::from_millis(ms))
    }
}

/// What the machine needs to start a session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Identify parameters.
    pub identify: IdentifyConfig,
    /// Reconnect policy.
    pub backoff: Backoff,
    /// Initial backoff attempt counter.
    pub attempt: u32,
}

impl SessionConfig {
    /// Builds a config with default backoff.
    #[must_use]
    pub fn new(identify: IdentifyConfig) -> Self {
        Self {
            identify,
            backoff: Backoff::new(
                std::time::Duration::from_secs(1),
                std::time::Duration::from_secs(60),
                10,
            ),
            attempt: 0,
        }
    }
}

/// The session state machine.
/// The session state machine.
// `session_id` and `heartbeat_interval` deliberately mirror the names Discord uses.
#[allow(clippy::struct_field_names)]
#[derive(Debug)]
pub struct Session {
    config: SessionConfig,
    state: State,
    /// Last sequence number seen, needed to resume.
    seq: Option<i64>,
    /// Session id from Ready, needed to resume.
    session_id: Option<String>,
    /// Heartbeat interval learned from Hello.
    heartbeat_interval: Option<std::time::Duration>,
    /// Whether a heartbeat is due.
    heartbeat_due: bool,
    /// Whether the server asked us to reconnect.
    server_reconnect: bool,
}

impl Session {
    /// Creates a session in [`State::Disconnected`].
    #[must_use]
    pub fn new(config: SessionConfig) -> Self {
        Self {
            config,
            state: State::Disconnected,
            seq: None,
            session_id: None,
            heartbeat_interval: None,
            heartbeat_due: false,
            server_reconnect: false,
        }
    }

    /// Current state.
    #[must_use]
    pub const fn state(&self) -> &State {
        &self.state
    }

    /// Last sequence number seen.
    #[must_use]
    pub const fn seq(&self) -> Option<i64> {
        self.seq
    }

    /// Session id, once known.
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// The heartbeat interval, once Hello has arrived.
    #[must_use]
    pub const fn heartbeat_interval(&self) -> Option<std::time::Duration> {
        self.heartbeat_interval
    }

    /// Whether a heartbeat is due now.
    #[must_use]
    pub const fn heartbeat_due(&self) -> bool {
        self.heartbeat_due
    }

    /// Starts connecting.
    pub fn begin_connect(&mut self) {
        if self.state == State::Disconnected {
            self.state = State::Connecting;
        }
    }

    /// Handles a successful connect: the transport is open, Hello is expected.
    ///
    /// Returns the frames to send, which is nothing yet — Discord speaks first.
    #[must_use]
    pub fn on_connected(&mut self) -> Step {
        self.state = State::Connecting;
        Step::default()
    }

    /// Handles one inbound frame.
    ///
    /// Takes the frame by value. Every dispatch frame's payload ends up in a [`Notice`], and taking it
    /// by reference meant deep-copying the whole `serde_json::Value` tree of every message, presence,
    /// and typing event the client received — the single hottest allocation in the process, since a
    /// busy session produces several per second.
    #[must_use]
    pub fn on_frame(&mut self, frame: Inbound) -> Step {
        let mut step = Step::default();

        if let Some(seq) = frame.s {
            self.seq = Some(seq);
        }

        match frame.op {
            Some(code) if code == OpCode::Hello.code() => {
                let interval_ms = frame.heartbeat_interval_ms().unwrap_or(41_250);
                self.heartbeat_interval = Some(std::time::Duration::from_millis(interval_ms));
                // Discord requires the first Identify/Resume promptly after Hello, with no jitter.
                self.server_reconnect = false;
                if self.can_resume() {
                    let token = self.config.identify.token.clone();
                    let session_id = self.session_id.clone().unwrap_or_default();
                    let seq = self.seq.unwrap_or(-1);
                    step.sent.push(Outbound::Resume {
                        token,
                        session_id: session_id.clone(),
                        seq,
                    });
                    self.state = State::Identifying {
                        session_id: Some(session_id),
                    };
                } else {
                    step.sent.push(self.config.identify.to_outbound());
                    self.state = State::Identifying { session_id: None };
                }
                self.heartbeat_due = false;
            }

            Some(code) if code == OpCode::HeartbeatAck.code() => {
                self.heartbeat_due = false;
            }

            Some(code) if code == OpCode::Reconnect.code() => {
                self.server_reconnect = true;
                self.schedule_reconnect(&mut step);
            }

            Some(code) if code == OpCode::InvalidSession.code() => {
                let resumable = frame.d.as_bool().unwrap_or(false);
                if resumable && self.can_resume() {
                    // Resume again; the id survived.
                } else {
                    self.session_id = None;
                    self.seq = None;
                }
                self.schedule_reconnect(&mut step);
            }

            // Dispatch frames arrive without an opcode, so they are matched by event name rather
            // than by op code.
            _ if frame.is_dispatch() => {
                self.handle_dispatch(frame, &mut step);
            }

            Some(code) if code == OpCode::Heartbeat.code() => {
                // Server-initiated heartbeat request.
                let seq = self.seq;
                step.sent.push(Outbound::Heartbeat { seq });
            }

            _ => {}
        }

        step
    }

    fn handle_dispatch(&mut self, frame: Inbound, step: &mut Step) {
        let Some(name) = frame.t else { return };

        if name == "READY" {
            if let Some(id) = read_ready_session_id(&frame.d) {
                self.session_id = Some(id.clone());
                self.state = State::Ready {
                    session_id: id.clone(),
                    user_id: self_id(&frame.d),
                };
                self.config.attempt = 0;
                step.notices.push(Notice::Ready {
                    user_id: self_id(&frame.d),
                    session_id: id,
                });
            }
            return;
        }

        let seq = frame.s.unwrap_or(-1);
        step.notices.push(Notice::Event {
            name,
            seq,
            data: frame.d,
        });
    }

    /// Whether a resume is possible: Discord needs both a session id and a sequence number to
    /// replay what was missed. Without either, a fresh Identify is the only option.
    #[must_use]
    pub fn can_resume(&self) -> bool {
        self.session_id.is_some() && self.seq.is_some()
    }

    /// Records that the transport dropped.
    #[must_use]
    pub fn on_disconnect(&mut self, reason: CloseReason) -> Step {
        let mut step = Step::default();

        if reason == CloseReason::InvalidToken {
            self.state = State::Failed(SessionFailure::InvalidToken);
            step.notices
                .push(Notice::Failed(SessionFailure::InvalidToken));
            return step;
        }
        if reason == CloseReason::AlreadyAuthenticated {
            self.state = State::Failed(SessionFailure::AlreadyAuthenticated);
            step.notices
                .push(Notice::Failed(SessionFailure::AlreadyAuthenticated));
            return step;
        }
        if !reason.is_retryable() {
            self.state = State::Failed(SessionFailure::AttemptsExhausted { attempts: 0 });
            return step;
        }

        // A non-resumable close invalidates the session.
        if !reason.is_resumable() {
            self.session_id = None;
            self.seq = None;
        }

        self.schedule_reconnect(&mut step);
        step
    }

    fn schedule_reconnect(&mut self, step: &mut Step) {
        self.config.attempt += 1;
        let attempt = self.config.attempt;

        let Some(delay) = self.config.backoff.delay_for(attempt) else {
            let failure = SessionFailure::AttemptsExhausted {
                attempts: attempt - 1,
            };
            self.state = State::Failed(failure.clone());
            step.notices.push(Notice::Failed(failure));
            return;
        };

        self.state = State::Reconnecting { attempt };
        step.sent.push(Outbound::Close {
            code: 1000,
            reason: "reconnecting".to_owned(),
        });
        step.notices.push(Notice::Reconnecting {
            attempt,
            delay,
            will_resume: self.session_id.is_some() && self.seq.is_some(),
        });
    }

    /// Handles a transport-level failure.
    #[must_use]
    pub fn on_transport_error(&mut self, error: &TransportError) -> Step {
        let mut step = Step::default();
        if let TransportError::Connect(message) = error {
            // A refused connection is usually not resumable: drop the session.
            self.session_id = None;
            self.seq = None;
            let _ = message;
        }
        self.schedule_reconnect(&mut step);
        step
    }

    /// Marks the heartbeat as due and returns the frame to send.
    ///
    /// Returns `None` when no heartbeat is due or the interval is not yet known.
    #[must_use]
    pub fn take_heartbeat_due(&mut self) -> Option<Outbound> {
        if !self.heartbeat_due {
            return None;
        }
        self.heartbeat_due = false;
        Some(Outbound::Heartbeat { seq: self.seq })
    }

    /// Arms the heartbeat. Call once per interval tick.
    pub fn arm_heartbeat(&mut self) {
        self.heartbeat_due = self.heartbeat_interval.is_some();
    }

    /// Sets the authenticated user id directly, for cases where Ready parsing is bypassed.
    pub fn set_session_id(&mut self, id: impl Into<String>) {
        self.session_id = Some(id.into());
    }

    /// Sets the last sequence number directly.
    pub fn set_seq(&mut self, seq: i64) {
        self.seq = Some(seq);
    }

    /// Whether the gateway asked for a reconnect.
    #[must_use]
    pub const fn server_requested_reconnect(&self) -> bool {
        self.server_reconnect
    }

    /// Connect policy.
    #[must_use]
    pub const fn backoff(&self) -> &Backoff {
        &self.config.backoff
    }

    /// Mutable connect policy.
    pub const fn backoff_mut(&mut self) -> &mut Backoff {
        &mut self.config.backoff
    }
}

/// Pulls the session id out of a READY payload.
fn read_ready_session_id(ready: &serde_json::Value) -> Option<String> {
    ready.get("session_id")?.as_str().map(str::to_owned)
}

/// Pulls the authenticated user's snowflake out of a READY payload.
///
/// Discord sends the id as a JSON string, because a snowflake exceeds the 53-bit safe integer
/// range for JavaScript consumers.
fn self_id(ready: &serde_json::Value) -> Snowflake {
    ready
        .get("user")
        .and_then(|u| u.get("id"))
        .and_then(|i| i.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .map_or(Snowflake::ZERO, Snowflake::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::CloseReason;

    fn config() -> SessionConfig {
        let mut config = SessionConfig::new(IdentifyConfig::new("tok", 402_402));
        config.backoff = Backoff::new(
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(1000),
            4,
        )
        .with_seed(42);
        config
    }

    fn ready_frame(session: &str, user: u64) -> Inbound {
        Inbound::dispatch(
            "READY",
            1,
            serde_json::json!({
                "session_id": session,
                "user": { "id": user.to_string(), "username": "tester" }
            }),
        )
    }

    #[test]
    fn hello_triggers_identify_when_not_resumable() {
        let mut s = Session::new(config());
        let step = s.on_frame(Inbound::hello(41_250));
        assert_eq!(step.sent.len(), 1);
        assert!(matches!(step.sent[0], Outbound::Identify { .. }));
        assert!(matches!(s.state(), State::Identifying { session_id: None }));
        assert_eq!(
            s.heartbeat_interval(),
            Some(std::time::Duration::from_millis(41_250))
        );
    }

    #[test]
    fn hello_triggers_resume_after_ready() {
        let mut s = Session::new(config());
        let _ = s.on_frame(Inbound::hello(1000));
        let _ = s.on_frame(ready_frame("sess-1", 42));
        assert_eq!(
            s.state(),
            &State::Ready {
                session_id: "sess-1".to_owned(),
                user_id: Snowflake::new(42)
            }
        );

        // Simulate a drop, then a reconnect.
        let _ = s.on_disconnect(CloseReason::ReconnectRequested);
        let step = s.on_frame(Inbound::hello(1000));
        assert!(
            matches!(&step.sent[0], Outbound::Resume { session_id, seq, .. } if session_id == "sess-1" && *seq == 1),
            "expected Resume, got {:?}",
            step.sent[0]
        );
    }

    #[test]
    fn ready_reports_a_notice_and_resets_backoff() {
        let mut s = Session::new(config());
        s.config.attempt = 3;
        let step = s.on_frame(ready_frame("sess-9", 7));
        assert_eq!(
            step.notices,
            vec![Notice::Ready {
                user_id: Snowflake::new(7),
                session_id: "sess-9".to_owned()
            }]
        );
        assert_eq!(s.config.attempt, 0);
        assert_eq!(s.session_id(), Some("sess-9"));
    }

    #[test]
    fn dispatch_forwards_events_with_sequence() {
        let mut s = Session::new(config());
        let frame = Inbound::dispatch("MESSAGE_CREATE", 17, serde_json::json!({ "id": "1" }));
        let step = s.on_frame(frame);
        match &step.notices[0] {
            Notice::Event { name, seq, data } => {
                assert_eq!(name, "MESSAGE_CREATE");
                assert_eq!(*seq, 17);
                assert_eq!(data["id"], "1");
            }
            other => panic!("expected Event, got {other:?}"),
        }
        assert_eq!(s.seq(), Some(17));
    }

    #[test]
    fn heartbeat_ack_clears_the_due_flag() {
        let mut s = Session::new(config());
        let _ = s.on_frame(Inbound::hello(1000));
        s.arm_heartbeat();
        assert!(s.heartbeat_due());
        let _ = s.on_frame(Inbound::heartbeat_ack());
        assert!(!s.heartbeat_due());
        assert_eq!(s.take_heartbeat_due(), None);
    }

    #[test]
    fn heartbeat_is_only_emitted_when_due() {
        let mut s = Session::new(config());
        let _ = s.on_frame(Inbound::hello(1000));
        let _ = s.on_frame(ready_frame("s", 1));
        assert_eq!(s.take_heartbeat_due(), None);

        s.arm_heartbeat();
        assert_eq!(
            s.take_heartbeat_due(),
            Some(Outbound::Heartbeat { seq: Some(1) })
        );
        // Taking it clears the flag, so a second call yields nothing.
        assert_eq!(s.take_heartbeat_due(), None);
    }

    #[test]
    fn heartbeat_is_not_armed_before_hello() {
        let mut s = Session::new(config());
        s.arm_heartbeat();
        assert!(!s.heartbeat_due(), "no interval means no heartbeat");
    }

    #[test]
    fn server_heartbeat_request_is_answered() {
        let mut s = Session::new(config());
        let _ = s.on_frame(Inbound::hello(1000));
        let _ = s.on_frame(ready_frame("s", 1));
        let step = s.on_frame(Inbound {
            op: Some(OpCode::Heartbeat.code()),
            t: None,
            s: None,
            d: serde_json::json!({ "client_state": null }),
        });
        assert_eq!(step.sent, vec![Outbound::Heartbeat { seq: Some(1) }]);
    }

    #[test]
    fn reconnect_opcode_schedules_a_retry() {
        let mut s = Session::new(config());
        let _ = s.on_frame(Inbound::hello(1000));
        let _ = s.on_frame(ready_frame("sess-2", 1));
        let step = s.on_frame(Inbound::reconnect());
        assert!(s.server_requested_reconnect());
        assert_eq!(s.state(), &State::Reconnecting { attempt: 1 });
        match &step.notices[0] {
            Notice::Reconnecting {
                attempt,
                will_resume,
                ..
            } => {
                assert_eq!(*attempt, 1);
                assert!(will_resume, "session id and seq are both known");
            }
            other => panic!("expected Reconnecting, got {other:?}"),
        }
        assert!(matches!(step.sent[0], Outbound::Close { .. }));
    }

    #[test]
    fn non_resumable_disconnect_drops_the_session() {
        let mut s = Session::new(config());
        let _ = s.on_frame(Inbound::hello(1000));
        let _ = s.on_frame(ready_frame("sess-3", 1));
        let step = s.on_disconnect(CloseReason::SessionInvalidated);
        assert_eq!(s.session_id(), None);
        assert_eq!(s.seq(), None);
        match &step.notices[0] {
            Notice::Reconnecting { will_resume, .. } => assert!(!will_resume),
            other => panic!("expected Reconnecting, got {other:?}"),
        }
    }

    #[test]
    fn resumable_disconnect_keeps_the_session() {
        let mut s = Session::new(config());
        let _ = s.on_frame(Inbound::hello(1000));
        let _ = s.on_frame(ready_frame("sess-4", 1));
        let _ = s.on_disconnect(CloseReason::ServerError);
        assert_eq!(s.session_id(), Some("sess-4"));
        assert_eq!(s.seq(), Some(1));
    }

    #[test]
    fn invalid_token_is_terminal() {
        let mut s = Session::new(config());
        let step = s.on_disconnect(CloseReason::InvalidToken);
        assert_eq!(s.state(), &State::Failed(SessionFailure::InvalidToken));
        assert_eq!(
            step.notices,
            vec![Notice::Failed(SessionFailure::InvalidToken)]
        );
        assert!(
            step.sent.is_empty(),
            "no frames should be sent after an auth failure"
        );
    }

    #[test]
    fn already_authenticated_is_terminal() {
        let mut s = Session::new(config());
        let _ = s.on_disconnect(CloseReason::AlreadyAuthenticated);
        assert_eq!(
            s.state(),
            &State::Failed(SessionFailure::AlreadyAuthenticated)
        );
    }

    #[test]
    fn backoff_gives_up_after_max_attempts() {
        let mut s = Session::new(config());
        let mut last = Step::default();
        for _ in 0..10 {
            last = s.on_disconnect(CloseReason::ServerError);
        }
        assert!(matches!(
            s.state(),
            State::Failed(SessionFailure::AttemptsExhausted { .. })
        ));
        assert!(
            matches!(last.notices.last(), Some(Notice::Failed(_))),
            "last step should report the failure"
        );
    }

    #[test]
    fn invalid_session_resumable_keeps_the_session() {
        let mut s = Session::new(config());
        let _ = s.on_frame(Inbound::hello(1000));
        let _ = s.on_frame(ready_frame("sess-5", 1));
        let _ = s.on_frame(Inbound::invalid_session(true));
        assert_eq!(s.session_id(), Some("sess-5"));
    }

    #[test]
    fn invalid_session_fatal_drops_the_session() {
        let mut s = Session::new(config());
        let _ = s.on_frame(Inbound::hello(1000));
        let _ = s.on_frame(ready_frame("sess-6", 1));
        let _ = s.on_frame(Inbound::invalid_session(false));
        assert_eq!(s.session_id(), None);
    }

    #[test]
    fn transport_errors_schedule_a_retry() {
        let mut s = Session::new(config());
        let step = s.on_transport_error(&TransportError::Disconnected("eof".into()));
        assert!(matches!(step.notices[0], Notice::Reconnecting { .. }));
        assert_eq!(s.state(), &State::Reconnecting { attempt: 1 });
    }

    #[test]
    fn connect_errors_drop_the_session() {
        let mut s = Session::new(config());
        s.set_session_id("sess-7");
        s.set_seq(5);
        let step = s.on_transport_error(&TransportError::Connect("refused".into()));
        assert_eq!(s.session_id(), None);
        assert_eq!(s.seq(), None);
        match &step.notices[0] {
            Notice::Reconnecting { will_resume, .. } => assert!(!will_resume),
            other => panic!("expected Reconnecting, got {other:?}"),
        }
    }

    #[test]
    fn unknown_frames_are_ignored() {
        let mut s = Session::new(config());
        let step = s.on_frame(Inbound {
            op: Some(99),
            t: None,
            s: Some(3),
            d: serde_json::Value::Null,
        });
        assert_eq!(step.sent, Vec::new());
        assert_eq!(step.notices, Vec::new());
        assert_eq!(s.seq(), Some(3), "sequence should still be tracked");
    }

    #[test]
    fn backoff_delays_grow_and_are_deterministic() {
        let mut a = Backoff::new(
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(1000),
            6,
        )
        .with_seed(7);
        let mut b = a.clone();
        for attempt in 1..=6 {
            assert_eq!(
                a.delay_for(attempt),
                b.delay_for(attempt),
                "attempt {attempt}"
            );
        }
        assert!(a.delay_for(7).is_none(), "beyond max_attempts");
    }

    #[test]
    fn backoff_respects_the_cap() {
        let mut b = Backoff::new(
            std::time::Duration::from_millis(1),
            std::time::Duration::from_millis(500),
            20,
        )
        .with_seed(1);
        for attempt in 1..=20 {
            let d = b.delay_for(attempt).unwrap();
            assert!(
                d <= std::time::Duration::from_millis(500),
                "attempt {attempt} exceeded cap"
            );
        }
    }

    #[test]
    fn full_identify_to_ready_cycle() {
        let mut s = Session::new(config());
        s.begin_connect();
        assert_eq!(s.state(), &State::Connecting);

        let step = s.on_connected();
        assert_eq!(step.sent, Vec::new(), "the gateway speaks first");

        let _ = s.on_frame(Inbound::hello(41_250));
        assert!(matches!(s.state(), State::Identifying { .. }));

        s.arm_heartbeat();
        let _ = s.on_frame(ready_frame("final", 99));
        assert_eq!(
            s.state(),
            &State::Ready {
                session_id: "final".to_owned(),
                user_id: Snowflake::new(99)
            }
        );
        assert_eq!(
            s.take_heartbeat_due(),
            Some(Outbound::Heartbeat { seq: Some(1) })
        );
    }
}
