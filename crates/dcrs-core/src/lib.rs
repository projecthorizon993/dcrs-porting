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
//! - [`backend`] — how to plug in a third-party gateway library.
//! - [`transport`] — the built-in WebSocket transport (`websocket` feature).
//! - [`ids`] — snowflakes.
//!
//! # Backends
//!
//! Nothing here opens a socket, and no gateway library is a required dependency. A backend is
//! anything implementing [`gateway::GatewayTransport`], which means `discord_client_gateway`,
//! `discordrs`, `twilight`, `serenity`, or a hand-rolled socket all work equally well. The
//! built-in transport is a convenience behind a feature flag; see [`backend`].
//!
//! ```no_run
//! use dcrs_core::{IdentifyConfig, Session, SessionConfig, Inbound};
//!
//! let mut session = Session::new(SessionConfig::new(IdentifyConfig::new("token", 402_402)));
//! session.on_frame(Inbound::hello(41_250));
//! ```

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod backend;
pub mod cache;
pub mod credentials;
pub mod event;
pub mod gateway;
pub mod ids;
pub mod ratelimit;
pub mod session;
pub mod transport;

pub use backend::{DecodeError, decode_event, decode_frame};
pub use cache::{Cache, CacheStats, Channel, Guild, Member, Message, User};
pub use credentials::{Credential, CredentialStore, MemoryStore, StoreError};
pub use event::{Event, EventBus};
pub use gateway::{
    CloseReason, GatewayTransport, IdentifyConfig, Inbound, OpCode, Outbound, TransportError,
};
pub use ids::Snowflake;
pub use ratelimit::RateLimiter;
pub use session::{Backoff, Notice, Session, SessionConfig, SessionFailure, State, Step};

#[cfg(feature = "websocket")]
pub use transport::{WsTransport, ZlibStream};
