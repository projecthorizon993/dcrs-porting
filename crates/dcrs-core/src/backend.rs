//! Backend integration: using a third-party Discord library instead of the built-in transport.
//!
//! `dcrs-core` deliberately owns the *protocol*, not the *socket*. Everything a gateway backend
//! has to provide is behind [`GatewayTransport`], and everything
//! it has to produce is a normalized [`Inbound`] frame.
//!
//! That means any Rust Discord library can be used as the transport, without this crate depending
//! on it:
//!
//! - `discord_client_gateway` - user-mode gateway with TLS/HTTP2 impersonation. The most relevant
//!   one, since it is built for exactly the non-browser fingerprint a native client needs.
//! - `discordrs` - typed gateway and REST, voice and DAVE support.
//! - `twilight` - fast and well-typed, but bot-oriented: it is gated on intents rather than
//!   `capabilities`, so it needs a user-mode adapter.
//! - `serenity` - bot framework; same caveat as twilight.
//!
//! Adapting one is mechanical. Implement [`GatewayTransport`] and translate that library's frames
//! into [`Inbound`]; if the library hands you a raw event name and a `serde_json::Value`, which all
//! of them do, [`decode_event`] does the rest.
//!
//! The built-in implementation is `WsTransport`, behind the `websocket` feature. It is a
//! convenience, not a requirement.

use crate::cache::{Channel, Guild, Member, Message, User};
use crate::event::Event;
use crate::gateway::Inbound;
use crate::ids::Snowflake;

// Named in the module docs, and feature-gated in `transport`, so bring them into scope for the
// doc build only.
#[cfg(doc)]
use crate::gateway::GatewayTransport;
#[cfg(all(doc, feature = "websocket"))]
use crate::transport::WsTransport;

/// Why an event could not be decoded.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// The payload was not a JSON object.
    #[error("event {name}: payload is not a JSON object")]
    NotAnObject {
        /// Event name.
        name: String,
    },
    /// A required field was missing or the wrong shape.
    #[error("event {name}: {detail}")]
    Malformed {
        /// Event name.
        name: String,
        /// What was wrong.
        detail: String,
    },
}

/// Turns a raw `(name, payload)` pair into a normalized [`Event`].
///
/// This is the integration point. Any gateway library that surfaces Discord's event name and JSON
/// payload can feed this directly, regardless of its own type system — so a backend does not need
/// to model Discord's entities at all.
///
/// Events this function does not model become [`Event::Unknown`], carrying the name and the raw
/// payload. That is deliberate: a Discord event added tomorrow is observable before it is
/// supported, and a plugin can consume it through the same event bus.
///
/// # Errors
/// Returns [`DecodeError`] when the payload is not an object, or a modelled event is missing a
/// field that cannot be defaulted.
#[allow(
    clippy::too_many_lines,
    reason = "one arm per event; splitting would obscure the table"
)]
pub fn decode_event(name: &str, payload: &serde_json::Value) -> Result<Event, DecodeError> {
    // Unknown events never fail: keeping the raw payload is more useful than erroring.
    const KNOWN: &[&str] = &[
        "READY",
        "GUILD_CREATE",
        "GUILD_DELETE",
        "CHANNEL_CREATE",
        "CHANNEL_UPDATE",
        "CHANNEL_DELETE",
        "MESSAGE_CREATE",
        "MESSAGE_UPDATE",
        "MESSAGE_DELETE",
        "GUILD_MEMBER_ADD",
        "USER_UPDATE",
        "TYPING_START",
        "PRESENCE_UPDATE",
    ];
    let known = KNOWN.contains(&name);
    if !known {
        return Ok(Event::Unknown {
            name: name.to_owned(),
        });
    }

    if !payload.is_object() {
        return Err(DecodeError::NotAnObject {
            name: name.to_owned(),
        });
    }

    match name {
        "READY" => Ok(Event::Ready {
            user_id: snowflake(payload.get("user").and_then(|u| u.get("id")), name)?,
            session_id: string(payload.get("session_id"), name, "session_id")?,
        }),
        "GUILD_CREATE" => {
            let guild = guild(payload, name)?;
            let guild_id = guild.id;
            Ok(Event::GuildCreate {
                channels: vec_of(
                    payload.get("channels"),
                    Channel::deserialize_shallow,
                    name,
                    "channels",
                )?,
                // Members inside GUILD_CREATE carry no `guild_id`: the enclosing guild implies it.
                members: payload
                    .get("members")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .map(|m| member_with_guild(m, guild_id, name))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?
                    .unwrap_or_default(),
                guild,
            })
        }
        "GUILD_DELETE" => Ok(Event::GuildDelete {
            guild_id: snowflake(payload.get("id"), name)?,
        }),
        "CHANNEL_CREATE" => Ok(Event::ChannelCreate {
            channel: channel(payload, name)?,
        }),
        "CHANNEL_UPDATE" => Ok(Event::ChannelUpdate {
            channel: channel(payload, name)?,
            // Discord sends the prior state under `before_update`, but only sometimes.
            before: payload
                .get("before_update")
                .filter(|b| b.is_object())
                .map(|b| channel(b, name))
                .transpose()?,
        }),
        "CHANNEL_DELETE" => Ok(Event::ChannelDelete {
            id: snowflake(payload.get("id"), name)?,
            guild_id: snowflake(payload.get("guild_id"), name)?,
        }),
        "MESSAGE_CREATE" => Ok(Event::MessageCreate {
            message: message(payload, name)?,
        }),
        "MESSAGE_UPDATE" => Ok(Event::MessageUpdate {
            message: message(payload, name)?,
        }),
        "MESSAGE_DELETE" => Ok(Event::MessageDelete {
            id: snowflake(payload.get("id"), name)?,
            channel_id: snowflake(payload.get("channel_id"), name)?,
        }),
        "GUILD_MEMBER_ADD" => Ok(Event::GuildMemberAdd {
            member: member(payload, name)?,
        }),
        "USER_UPDATE" => Ok(Event::UserUpdate {
            user: user(payload, name)?,
        }),
        "TYPING_START" => Ok(Event::TypingStart {
            channel_id: snowflake(payload.get("channel_id"), name)?,
            user_id: snowflake(payload.get("user_id"), name)?,
        }),
        "PRESENCE_UPDATE" => Ok(Event::PresenceUpdate {
            user_id: snowflake(payload.get("user").and_then(|u| u.get("id")), name)?,
            status: payload
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("offline")
                .to_owned(),
        }),
        _ => Ok(Event::Unknown {
            name: name.to_owned(),
        }),
    }
}

/// Decodes an [`Inbound`] frame into an [`Event`], if it carries one.
///
/// Non-dispatch frames yield `Ok(None)`, since `Hello`, `HeartbeatAck` and the rest are session
/// concerns rather than events.
pub fn decode_frame(frame: &Inbound) -> Result<Option<Event>, DecodeError> {
    if !frame.is_dispatch() {
        return Ok(None);
    }
    match &frame.t {
        Some(name) => decode_event(name, &frame.d).map(Some),
        None => Ok(Some(Event::UnknownDispatch { name: None })),
    }
}

// -- field helpers ---------------------------------------------------------

fn malformed(name: &str, detail: impl Into<String>) -> DecodeError {
    DecodeError::Malformed {
        name: name.to_owned(),
        detail: detail.into(),
    }
}

fn snowflake(value: Option<&serde_json::Value>, name: &str) -> Result<Snowflake, DecodeError> {
    match value {
        Some(serde_json::Value::String(s)) => s
            .parse::<u64>()
            .map(Snowflake::new)
            .map_err(|e| malformed(name, format!("id {s:?}: {e}"))),
        Some(serde_json::Value::Number(n)) => Ok(Snowflake::new(n.as_u64().unwrap_or(0))),
        _ => Err(malformed(name, "missing or non-scalar id")),
    }
}

fn string(
    value: Option<&serde_json::Value>,
    name: &str,
    field: &str,
) -> Result<String, DecodeError> {
    value
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| malformed(name, format!("missing {field}")))
}

fn opt_string(value: Option<&serde_json::Value>) -> Option<String> {
    value.and_then(serde_json::Value::as_str).map(str::to_owned)
}

fn opt_snowflake(value: Option<&serde_json::Value>) -> Option<Snowflake> {
    match value {
        Some(serde_json::Value::String(s)) => s.parse::<u64>().ok().map(Snowflake::new),
        Some(serde_json::Value::Number(n)) => Some(Snowflake::new(n.as_u64().unwrap_or(0))),
        _ => None,
    }
}

fn opt_u32(value: Option<&serde_json::Value>) -> Option<u32> {
    value
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
}

fn string_list(value: Option<&serde_json::Value>) -> Vec<Snowflake> {
    value
        .and_then(serde_json::Value::as_array)
        // The iterator yields `&Value`; `opt_snowflake` takes `Option<&Value>` so it can also be
        // used for an absent field.
        .map(|a| a.iter().filter_map(|v| opt_snowflake(Some(v))).collect())
        .unwrap_or_default()
}

fn vec_of<T, F>(
    value: Option<&serde_json::Value>,
    decode: F,
    name: &str,
    field: &str,
) -> Result<Vec<T>, DecodeError>
where
    F: Fn(&serde_json::Value, &str) -> Result<T, DecodeError>,
{
    match value {
        None | Some(serde_json::Value::Null) => Ok(Vec::new()),
        Some(serde_json::Value::Array(items)) => items.iter().map(|i| decode(i, name)).collect(),
        Some(_) => Err(malformed(name, format!("{field} is not an array"))),
    }
}

fn user(value: &serde_json::Value, name: &str) -> Result<User, DecodeError> {
    Ok(User {
        id: snowflake(value.get("id"), name)?,
        username: string(value.get("username"), name, "username")?,
        global_name: opt_string(value.get("global_name")),
        avatar: opt_string(value.get("avatar")),
        is_bot: value
            .get("bot")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        deleted: false,
    })
}

fn guild(value: &serde_json::Value, name: &str) -> Result<Guild, DecodeError> {
    Ok(Guild {
        id: snowflake(value.get("id"), name)?,
        name: string(value.get("name"), name, "name")?,
        icon: opt_string(value.get("icon")),
        owner_id: opt_snowflake(value.get("owner_id")),
        member_count: opt_u32(value.get("member_count")).unwrap_or(0),
    })
}

fn channel(value: &serde_json::Value, name: &str) -> Result<Channel, DecodeError> {
    Ok(Channel {
        id: snowflake(value.get("id"), name)?,
        guild_id: opt_snowflake(value.get("guild_id")),
        name: opt_string(value.get("name")),
        kind: value
            .get("type")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u8,
        parent_id: opt_snowflake(value.get("parent_id")),
        position: opt_u32(value.get("position")).unwrap_or(0),
    })
}

fn message(value: &serde_json::Value, name: &str) -> Result<Message, DecodeError> {
    Ok(Message {
        id: snowflake(value.get("id"), name)?,
        channel_id: snowflake(value.get("channel_id"), name)?,
        author_id: snowflake(value.get("author").and_then(|a| a.get("id")), name)?,
        content: value
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        guild_id: opt_snowflake(value.get("guild_id")),
        mentions: string_list(value.get("mentions")),
        timestamp: opt_snowflake(value.get("timestamp")),
    })
}

fn member(value: &serde_json::Value, name: &str) -> Result<Member, DecodeError> {
    let guild_id = snowflake(value.get("guild_id"), name)?;
    member_body(value, guild_id, name)
}

/// Decodes a member whose guild is known from context.
///
/// Discord omits `guild_id` from the members nested in `GUILD_CREATE`, because the enclosing guild
/// already determines it.
fn member_with_guild(
    value: &serde_json::Value,
    guild_id: Snowflake,
    name: &str,
) -> Result<Member, DecodeError> {
    match value.get("guild_id") {
        Some(_) => member(value, name),
        None => member_body(value, guild_id, name),
    }
}

fn member_body(
    value: &serde_json::Value,
    guild_id: Snowflake,
    name: &str,
) -> Result<Member, DecodeError> {
    Ok(Member {
        guild_id,
        // A lazily-discovered member carries the id at the top level rather than under `user`.
        user_id: opt_snowflake(value.get("user").and_then(|u| u.get("id")))
            .or_else(|| opt_snowflake(value.get("user_id")))
            .ok_or_else(|| malformed(name, "member has no user id"))?,
        nick: opt_string(value.get("nick")),
        roles: string_list(value.get("roles")),
    })
}

/// `serde` shims, so the shallow decoders above and the derived `Deserialize` impls stay in sync.
trait Shallow {
    fn deserialize_shallow(value: &serde_json::Value, event: &str) -> Result<Self, DecodeError>
    where
        Self: Sized;
}

impl Shallow for Channel {
    fn deserialize_shallow(value: &serde_json::Value, event: &str) -> Result<Self, DecodeError> {
        channel(value, event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> serde_json::Value {
        serde_json::json!({
            "v": 9,
            "session_id": "abc",
            "user": { "id": "42", "username": "tester" }
        })
    }

    #[test]
    fn decodes_ready() {
        let event = decode_event("READY", &ready()).unwrap();
        assert_eq!(
            event,
            Event::Ready {
                user_id: Snowflake::new(42),
                session_id: "abc".to_owned()
            }
        );
    }

    #[test]
    fn unknown_events_are_never_errors() {
        let payload = serde_json::json!([1, 2, 3]);
        let event = decode_event("SOMETHING_BRAND_NEW", &payload).unwrap();
        assert_eq!(
            event,
            Event::Unknown {
                name: "SOMETHING_BRAND_NEW".to_owned()
            }
        );
    }

    #[test]
    fn known_events_require_an_object() {
        let err = decode_event("READY", &serde_json::json!([1])).unwrap_err();
        assert!(matches!(err, DecodeError::NotAnObject { .. }));
    }

    #[test]
    fn ready_without_a_session_id_is_an_error() {
        let payload = serde_json::json!({ "user": { "id": "1" } });
        let err = decode_event("READY", &payload).unwrap_err();
        assert!(matches!(err, DecodeError::Malformed { .. }));
    }

    #[test]
    fn snowflakes_parse_from_strings_and_numbers() {
        assert_eq!(
            snowflake(Some(&serde_json::json!("123")), "T").unwrap(),
            Snowflake::new(123)
        );
        assert_eq!(
            snowflake(Some(&serde_json::json!(123)), "T").unwrap(),
            Snowflake::new(123)
        );
        assert!(snowflake(None, "T").is_err());
        assert!(snowflake(Some(&serde_json::json!("abc")), "T").is_err());
    }

    #[test]
    fn decodes_a_guild_create_with_children() {
        let payload = serde_json::json!({
            "id": "1",
            "name": "Test Guild",
            "owner_id": "9",
            "member_count": 42,
            "channels": [{ "id": "10", "name": "general", "type": 0, "position": 1 }],
            "members": [{ "user": { "id": "2" }, "roles": ["3"], "nick": "nick" }]
        });
        let event = decode_event("GUILD_CREATE", &payload).unwrap();
        let Event::GuildCreate {
            guild,
            channels,
            members,
        } = event
        else {
            panic!("expected GuildCreate");
        };
        assert_eq!(guild.name, "Test Guild");
        assert_eq!(guild.owner_id, Some(Snowflake::new(9)));
        assert_eq!(guild.member_count, 42);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name.as_deref(), Some("general"));
        assert_eq!(members[0].user_id, Snowflake::new(2));
        assert_eq!(members[0].nick.as_deref(), Some("nick"));
        assert_eq!(members[0].roles, vec![Snowflake::new(3)]);
    }

    #[test]
    fn guild_create_tolerates_absent_children() {
        let payload = serde_json::json!({ "id": "1", "name": "G" });
        let event = decode_event("GUILD_CREATE", &payload).unwrap();
        let Event::GuildCreate {
            channels, members, ..
        } = event
        else {
            panic!()
        };
        assert_eq!(channels, Vec::new());
        assert_eq!(members, Vec::new());
    }

    #[test]
    fn member_id_falls_back_to_the_top_level_field() {
        // Lazily-discovered members carry `user_id` instead of a nested `user`.
        let payload = serde_json::json!({ "guild_id": "1", "user_id": "7", "roles": [] });
        let event = decode_event("GUILD_MEMBER_ADD", &payload).unwrap();
        let Event::GuildMemberAdd { member } = event else {
            panic!()
        };
        assert_eq!(member.user_id, Snowflake::new(7));
    }

    #[test]
    fn member_without_any_user_id_is_an_error() {
        let payload = serde_json::json!({ "guild_id": "1", "roles": [] });
        assert!(decode_event("GUILD_MEMBER_ADD", &payload).is_err());
    }

    #[test]
    fn channel_update_captures_the_prior_state_when_sent() {
        let with_before = serde_json::json!({
            "id": "10", "name": "new",
            "before_update": { "id": "10", "name": "old" }
        });
        let Event::ChannelUpdate { channel, before } =
            decode_event("CHANNEL_UPDATE", &with_before).unwrap()
        else {
            panic!();
        };
        assert_eq!(channel.name.as_deref(), Some("new"));
        assert_eq!(before.unwrap().name.as_deref(), Some("old"));

        let without = serde_json::json!({ "id": "10", "name": "new" });
        let Event::ChannelUpdate { before, .. } = decode_event("CHANNEL_UPDATE", &without).unwrap()
        else {
            panic!();
        };
        assert!(before.is_none());
    }

    #[test]
    fn presence_defaults_to_offline() {
        let with_status = serde_json::json!({ "user": { "id": "5" }, "status": "online" });
        let Event::PresenceUpdate { status, .. } =
            decode_event("PRESENCE_UPDATE", &with_status).unwrap()
        else {
            panic!();
        };
        assert_eq!(status, "online");

        let bare = serde_json::json!({ "user": { "id": "5" } });
        let Event::PresenceUpdate { status, .. } = decode_event("PRESENCE_UPDATE", &bare).unwrap()
        else {
            panic!();
        };
        assert_eq!(status, "offline");
    }

    #[test]
    fn message_mentions_default_to_empty() {
        let payload = serde_json::json!({
            "id": "1", "channel_id": "2", "author": { "id": "3" }, "content": "hi"
        });
        let Event::MessageCreate { message } = decode_event("MESSAGE_CREATE", &payload).unwrap()
        else {
            panic!();
        };
        assert_eq!(message.content, "hi");
        assert_eq!(message.mentions, Vec::new());
    }

    #[test]
    fn decode_frame_ignores_non_dispatches() {
        assert!(decode_frame(&Inbound::hello(1000)).unwrap().is_none());
        assert!(decode_frame(&Inbound::heartbeat_ack()).unwrap().is_none());
    }

    #[test]
    fn decode_frame_passes_dispatches_through() {
        let frame = Inbound::dispatch("READY", 1, ready());
        let event = decode_frame(&frame).unwrap().unwrap();
        assert_eq!(event.name(), "READY");
    }

    #[test]
    fn typing_start_decodes() {
        let payload = serde_json::json!({ "channel_id": "1", "user_id": "2" });
        assert_eq!(
            decode_event("TYPING_START", &payload).unwrap(),
            Event::TypingStart {
                channel_id: Snowflake::new(1),
                user_id: Snowflake::new(2)
            }
        );
    }

    #[test]
    fn wrong_type_for_a_list_is_an_error() {
        let payload = serde_json::json!({ "id": "1", "name": "G", "channels": "nope" });
        assert!(matches!(
            decode_event("GUILD_CREATE", &payload),
            Err(DecodeError::Malformed { .. })
        ));
    }
}
