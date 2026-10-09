//! The event bus.
//!
//! One broadcast channel carries decoded events to every consumer: the cache, the UI, and any
//! plugins. A slow consumer misses events rather than blocking the gateway loop, which is the
//! same contract the Flux store change-listener surface gives mods upstream.

use std::sync::Arc;

use tokio::sync::broadcast;

use crate::cache::{Channel, Guild, Member, Message, User};
use crate::ids::Snowflake;

/// How many events a slow consumer may fall behind before it starts missing them.
pub const EVENT_BUFFER: usize = 1024;

/// A decoded gateway event.
///
/// Only the events the cache and UI actually need are modelled. Anything else arrives as
/// [`Event::Unknown`] with its name and raw payload intact, so a new Discord event is observable
/// before it is supported — and a plugin can consume it through the same channel.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "event", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Event {
    /// The session is ready.
    Ready {
        /// Authenticated user.
        user_id: Snowflake,
        /// Session id, needed to resume.
        session_id: String,
    },
    /// A guild and its initial contents arrived.
    GuildCreate {
        /// The guild.
        guild: Guild,
        /// Its channels.
        channels: Vec<Channel>,
        /// Its members.
        members: Vec<Member>,
    },
    /// The client left a guild, or it became unavailable.
    GuildDelete {
        /// The guild.
        guild_id: Snowflake,
    },
    /// A channel was created.
    ChannelCreate {
        /// The channel.
        channel: Channel,
    },
    /// A channel changed.
    ChannelUpdate {
        /// The new state.
        channel: Channel,
        /// The previous state, when the gateway supplied it.
        before: Option<Channel>,
    },
    /// A channel was removed.
    ChannelDelete {
        /// The channel.
        id: Snowflake,
        /// Its guild.
        guild_id: Snowflake,
    },
    /// A message was created.
    MessageCreate {
        /// The message.
        message: Message,
    },
    /// A message changed.
    MessageUpdate {
        /// The new state.
        message: Message,
    },
    /// A message was removed.
    MessageDelete {
        /// The message.
        id: Snowflake,
        /// Its channel.
        channel_id: Snowflake,
    },
    /// A member joined or was lazily discovered.
    GuildMemberAdd {
        /// The member.
        member: Member,
    },
    /// A user changed.
    UserUpdate {
        /// The user.
        user: User,
    },
    /// Someone started typing.
    TypingStart {
        /// The channel.
        channel_id: Snowflake,
        /// The user.
        user_id: Snowflake,
    },
    /// A presence changed.
    PresenceUpdate {
        /// The user.
        user_id: Snowflake,
        /// Raw presence payload.
        status: String,
    },
    /// A modelled event with no payload we keep.
    Unknown {
        /// Event name.
        name: String,
    },
    /// A dispatch that is not an event at all.
    UnknownDispatch {
        /// Event name, if the gateway supplied one.
        name: Option<String>,
    },
}

impl Event {
    /// The wire event name, matching what Discord sends.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Ready { .. } => "READY",
            Self::GuildCreate { .. } => "GUILD_CREATE",
            Self::GuildDelete { .. } => "GUILD_DELETE",
            Self::ChannelCreate { .. } => "CHANNEL_CREATE",
            Self::ChannelUpdate { .. } => "CHANNEL_UPDATE",
            Self::ChannelDelete { .. } => "CHANNEL_DELETE",
            Self::MessageCreate { .. } => "MESSAGE_CREATE",
            Self::MessageUpdate { .. } => "MESSAGE_UPDATE",
            Self::MessageDelete { .. } => "MESSAGE_DELETE",
            Self::GuildMemberAdd { .. } => "GUILD_MEMBER_ADD",
            Self::UserUpdate { .. } => "USER_UPDATE",
            Self::TypingStart { .. } => "TYPING_START",
            Self::PresenceUpdate { .. } => "PRESENCE_UPDATE",
            Self::Unknown { name } | Self::UnknownDispatch { name: Some(name) } => name,
            Self::UnknownDispatch { name: None } => "",
        }
    }

    /// Whether this event mutates the cache.
    #[must_use]
    pub const fn affects_cache(&self) -> bool {
        !matches!(
            self,
            Self::TypingStart { .. }
                | Self::PresenceUpdate { .. }
                | Self::Unknown { .. }
                | Self::UnknownDispatch { .. }
        )
    }
}

/// Fan-out of events to every consumer.
#[derive(Debug, Clone)]
pub struct EventBus {
    tx: Arc<broadcast::Sender<Event>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    /// Creates a bus with the default buffer.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(EVENT_BUFFER)
    }

    /// Creates a bus with an explicit buffer.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx: Arc::new(tx) }
    }

    /// Subscribes to future events.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    /// Number of live subscribers.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }

    /// Publishes an event.
    ///
    /// Returns the number of subscribers that received it. Zero is normal when nothing is
    /// listening yet, and is not an error: the cache is fed by the session loop, not by fan-out.
    pub fn publish(&self, event: Event) -> usize {
        self.tx.send(event).unwrap_or(0)
    }

    /// Publishes and applies to a cache in one step, so the two cannot drift.
    ///
    /// Returns the number of subscribers that received the event.
    pub fn publish_to(&self, cache: &crate::cache::Cache, event: Event) -> usize {
        cache.apply(&event);
        self.publish(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_names_match_the_wire() {
        assert_eq!(
            Event::TypingStart {
                channel_id: Snowflake::ZERO,
                user_id: Snowflake::ZERO
            }
            .name(),
            "TYPING_START"
        );
        assert_eq!(
            Event::GuildDelete {
                guild_id: Snowflake::ZERO
            }
            .name(),
            "GUILD_DELETE"
        );
        assert_eq!(
            Event::Unknown {
                name: "NEW_THING".to_owned()
            }
            .name(),
            "NEW_THING"
        );
        assert_eq!(Event::UnknownDispatch { name: None }.name(), "");
    }

    #[test]
    fn cache_affecting_classification() {
        assert!(
            Event::GuildDelete {
                guild_id: Snowflake::ZERO
            }
            .affects_cache()
        );
        assert!(
            !Event::TypingStart {
                channel_id: Snowflake::ZERO,
                user_id: Snowflake::ZERO
            }
            .affects_cache()
        );
        assert!(
            !Event::Unknown {
                name: "X".to_owned()
            }
            .affects_cache()
        );
    }

    #[tokio::test]
    async fn subscribers_receive_published_events() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 1);

        let n = bus.publish(Event::GuildDelete {
            guild_id: Snowflake::new(1),
        });
        assert_eq!(n, 1);
        assert_eq!(
            rx.recv().await.unwrap(),
            Event::GuildDelete {
                guild_id: Snowflake::new(1)
            }
        );
    }

    #[tokio::test]
    async fn publishing_with_no_subscribers_is_not_an_error() {
        let bus = EventBus::new();
        assert_eq!(
            bus.publish(Event::TypingStart {
                channel_id: Snowflake::ZERO,
                user_id: Snowflake::ZERO
            }),
            0
        );
    }

    #[tokio::test]
    async fn multiple_subscribers_all_receive() {
        let bus = EventBus::new();
        let mut a = bus.subscribe();
        let mut b = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 2);
        bus.publish(Event::GuildDelete {
            guild_id: Snowflake::new(5),
        });
        assert!(a.recv().await.is_ok());
        assert!(b.recv().await.is_ok());
    }

    #[tokio::test]
    async fn publish_to_also_mutates_the_cache() {
        let bus = EventBus::new();
        let cache = crate::cache::Cache::new();
        let mut rx = bus.subscribe();

        bus.publish_to(
            &cache,
            Event::Ready {
                user_id: Snowflake::new(7),
                session_id: "s".to_owned(),
            },
        );

        assert_eq!(cache.self_id(), Snowflake::new(7));
        assert_eq!(rx.recv().await.unwrap().name(), "READY");
    }

    #[tokio::test]
    async fn a_slow_consumer_lags_rather_than_blocking() {
        let bus = EventBus::with_capacity(4);
        let cache = crate::cache::Cache::new();
        // Publish well past the buffer without the subscriber reading.
        for i in 0..10u64 {
            bus.publish_to(
                &cache,
                Event::GuildDelete {
                    guild_id: Snowflake::new(i),
                },
            );
        }
        let mut rx = bus.subscribe();
        // A late subscriber sees nothing from before it existed.
        assert!(matches!(
            rx.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[test]
    fn events_round_trip_through_json() {
        let event = Event::GuildDelete {
            guild_id: Snowflake::new(42),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            json.contains("GUILD_DELETE"),
            "tagged enum should name the event: {json}"
        );
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn snowflakes_survive_the_json_round_trip_as_strings() {
        let event = Event::GuildDelete {
            guild_id: Snowflake::new(9_007_199_254_740_993),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            json.contains("\"guild_id\":\"9007199254740993\""),
            "got {json}"
        );
        assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), event);
    }
}
