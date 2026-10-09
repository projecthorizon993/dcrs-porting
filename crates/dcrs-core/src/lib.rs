//! Headless Discord client core.
//!
//! Everything here is free of UI concerns so it can be unit-tested without a window. The layering:
//!
//! - [`gateway`] — wire types and the [`gateway::GatewayTransport`] seam.
//! - [`session`] — the state machine that drives a transport through connect/identify/resume.
//! - [`event`] — decoded events fanned out over a broadcast channel.
//! - [`cache`] — the store registry, applying events to in-memory state.
//! - [`ratelimit`] — REST bucket tracking.
//! - [`credentials`] — pluggable, non-logging token storage.
//! - [`ids`] — snowflakes.
//!
//! Nothing in this crate opens a socket by itself. The production WebSocket transport is a
//! separate concern, and every state transition is testable against a mock.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod cache;
pub mod credentials;
pub mod event;
pub mod gateway;
pub mod ids;
pub mod ratelimit;
pub mod session;

pub use cache::{Cache, CacheStats, Channel, Guild, Member, Message, User};
pub use credentials::{Credential, CredentialStore, MemoryStore, StoreError};
pub use event::{Event, EventBus};
pub use gateway::{
    CloseReason, GatewayTransport, IdentifyConfig, Inbound, OpCode, Outbound, TransportError,
};
pub use ids::Snowflake;
pub use ratelimit::RateLimiter;
pub use session::{Backoff, Notice, Session, SessionConfig, SessionFailure, State, Step};
