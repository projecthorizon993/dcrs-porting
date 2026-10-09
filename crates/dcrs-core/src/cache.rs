//! The cache and its store registry.
//!
//! The source survey's most actionable finding: mods read from 71 Flux stores with ~400 getters,
//! and reimplementing them is wasted work — what is needed is a *registry* the UI and plugins can
//! reach through typed handles. That is what this module is, and it is what
//! `dcrs-compat`'s class (a) surfaces resolve against.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::event::Event;
use crate::ids::Snowflake;

/// A user, as far as the cache is concerned.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct User {
    /// Snowflake.
    pub id: Snowflake,
    /// Username.
    pub username: String,
    /// Display name, when set.
    pub global_name: Option<String>,
    /// Avatar hash.
    pub avatar: Option<String>,
    /// Bot accounts are cached too, since messages reference them.
    #[serde(default)]
    pub is_bot: bool,
    /// Whether the account has been deleted.
    #[serde(default)]
    pub deleted: bool,
}

impl User {
    /// The name to show: display name if set, else username.
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.global_name.as_deref().unwrap_or(&self.username)
    }
}

/// A guild channel.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Channel {
    /// Snowflake.
    pub id: Snowflake,
    /// Guild this channel belongs to, or `None` for a DM.
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    /// Channel name, absent for DMs.
    #[serde(default)]
    pub name: Option<String>,
    /// Numeric channel type.
    #[serde(rename = "type", default)]
    pub kind: u8,
    /// Parent category, for text channels inside a folder.
    #[serde(default)]
    pub parent_id: Option<Snowflake>,
    /// Position in the channel list.
    #[serde(default)]
    pub position: u32,
}

/// A guild.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Guild {
    /// Snowflake.
    pub id: Snowflake,
    /// Guild name.
    pub name: String,
    /// Icon hash.
    #[serde(default)]
    pub icon: Option<String>,
    /// The owner's id.
    #[serde(default)]
    pub owner_id: Option<Snowflake>,
    /// Member count.
    #[serde(default)]
    pub member_count: u32,
}

/// A message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Message {
    /// Snowflake.
    pub id: Snowflake,
    /// Channel the message is in.
    pub channel_id: Snowflake,
    /// Author.
    pub author_id: Snowflake,
    /// Message content.
    #[serde(default)]
    pub content: String,
    /// Guild, when the channel is in one.
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    /// Mentions and attachments are left raw until the UI needs them.
    #[serde(default)]
    pub mentions: Vec<Snowflake>,
    /// Timestamp.
    #[serde(default)]
    pub timestamp: Option<Snowflake>,
}

/// A member of a guild.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Member {
    /// Guild id.
    pub guild_id: Snowflake,
    /// User id.
    pub user_id: Snowflake,
    /// Nickname, if set.
    #[serde(default)]
    pub nick: Option<String>,
    /// Role ids.
    #[serde(default)]
    pub roles: Vec<Snowflake>,
}

/// The in-memory cache.
///
/// Cloning is cheap: clones share the same data, so the UI can hold a read handle on another
/// thread while the session loop writes. Every mutation goes through
/// [`Cache::apply`], which is fed by the session machine, so cache changes stay traceable to an
/// event name.
#[derive(Debug, Clone, Default)]
pub struct Cache {
    inner: Arc<RwLock<CacheInner>>,
}

impl Cache {
    /// A read guard over the shared data.
    ///
    /// Returns `None` only if a writer panicked while holding the lock, which would already be a
    /// bug; treating it as "cache unavailable" beats propagating a panic into the UI.
    fn read(&self) -> Option<RwLockReadGuard<'_, CacheInner>> {
        self.inner.read().ok()
    }

    /// A write guard over the shared data.
    fn write(&self) -> Option<RwLockWriteGuard<'_, CacheInner>> {
        self.inner.write().ok()
    }
}

#[derive(Debug, Default)]
struct CacheInner {
    users: BTreeMap<Snowflake, User>,
    guilds: BTreeMap<Snowflake, Guild>,
    channels: BTreeMap<Snowflake, Channel>,
    messages: BTreeMap<Snowflake, Message>,
    /// Keyed by `(guild_id, user_id)`.
    members: BTreeMap<(Snowflake, Snowflake), Member>,
    /// Guild ids in the order the user arranged them.
    guild_order: Vec<Snowflake>,
    /// Unread flags, keyed by `(guild_id, channel_id)`.
    unread: BTreeMap<(Snowflake, Snowflake), bool>,
    /// The authenticated user.
    self_id: Snowflake,
    /// Bumped on every mutation, so a UI can cheaply detect staleness.
    revision: u64,
}

/// The store surface ids this cache implements.
///
/// Kept in sync with `assets/capabilities.toml`.
pub const SUPPORTED_STORES: &[&str] = &[
    "stores.UserStore",
    "stores.GuildStore",
    "stores.ChannelStore",
    "stores.MessageStore",
    "stores.GuildMemberStore",
];

impl Cache {
    /// Creates an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The authenticated user's id.
    #[must_use]
    pub fn self_id(&self) -> Snowflake {
        self.read().map_or(Snowflake::ZERO, |g| g.self_id)
    }

    /// Sets the authenticated user's id.
    pub fn set_self_id(&self, id: Snowflake) {
        if let Some(mut g) = self.write() {
            g.self_id = id;
            g.revision += 1;
        }
    }

    /// A counter that changes on every mutation, for cheap staleness checks in the UI.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.read().map_or(0, |g| g.revision)
    }

    // -- reads ------------------------------------------------------------

    /// A user by id.
    #[must_use]
    pub fn user(&self, id: Snowflake) -> Option<User> {
        self.read()?.users.get(&id).cloned()
    }

    /// Every user.
    #[must_use]
    pub fn users(&self) -> Vec<User> {
        self.read()
            .map_or_else(Vec::new, |g| g.users.values().cloned().collect())
    }

    /// A guild by id.
    #[must_use]
    pub fn guild(&self, id: Snowflake) -> Option<Guild> {
        self.read()?.guilds.get(&id).cloned()
    }

    /// Guilds in the user's own ordering.
    #[must_use]
    pub fn guilds(&self) -> Vec<Guild> {
        let Some(g) = self.read() else {
            return Vec::new();
        };
        g.guild_order
            .iter()
            .filter_map(|id| g.guilds.get(id).cloned())
            .collect()
    }

    /// A channel by id.
    #[must_use]
    pub fn channel(&self, id: Snowflake) -> Option<Channel> {
        self.read()?.channels.get(&id).cloned()
    }

    /// Channels in a guild, ordered by position then id.
    #[must_use]
    pub fn guild_channels(&self, guild_id: Snowflake) -> Vec<Channel> {
        let Some(g) = self.read() else {
            return Vec::new();
        };
        let mut out: Vec<Channel> = g
            .channels
            .values()
            .filter(|c| c.guild_id == Some(guild_id))
            .cloned()
            .collect();
        out.sort_by_key(|c| (c.position, c.id.raw()));
        out
    }

    /// A DM channel by the other participant's user id.
    ///
    /// Mirrors `ChannelStore.getDMFromUserId`, which is how mods resolve a recipient to a channel.
    #[must_use]
    pub fn dm_from_user_id(&self, user_id: Snowflake) -> Option<Channel> {
        let g = self.read()?;
        g.channels
            .values()
            .find(|c| c.guild_id.is_none() && c.name.as_deref() == Some(&user_id.to_string()))
            .cloned()
    }

    /// A message by id.
    #[must_use]
    pub fn message(&self, id: Snowflake) -> Option<Message> {
        self.read()?.messages.get(&id).cloned()
    }

    /// Messages in a channel, oldest first.
    #[must_use]
    pub fn channel_messages(&self, channel_id: Snowflake) -> Vec<Message> {
        let Some(g) = self.read() else {
            return Vec::new();
        };
        let mut out: Vec<Message> = g
            .messages
            .values()
            .filter(|m| m.channel_id == channel_id)
            .cloned()
            .collect();
        out.sort_by_key(|m| m.id);
        out
    }

    /// A guild member.
    #[must_use]
    pub fn member(&self, guild_id: Snowflake, user_id: Snowflake) -> Option<Member> {
        self.read()?.members.get(&(guild_id, user_id)).cloned()
    }

    /// Members of a guild.
    #[must_use]
    pub fn guild_members(&self, guild_id: Snowflake) -> Vec<Member> {
        let Some(g) = self.read() else {
            return Vec::new();
        };
        g.members
            .values()
            .filter(|m| m.guild_id == guild_id)
            .cloned()
            .collect()
    }

    /// Whether a channel is marked unread.
    #[must_use]
    pub fn is_unread(&self, guild_id: Snowflake, channel_id: Snowflake) -> bool {
        self.read().is_some_and(|g| {
            g.unread
                .get(&(guild_id, channel_id))
                .copied()
                .unwrap_or(false)
        })
    }

    // -- writes -----------------------------------------------------------
    //
    // These take `&self` because the cache is shared through an `Arc`: the UI holds clones and
    // must see writes. Serialization comes from the `RwLock`, not from the borrow checker.

    /// Inserts or replaces a user.
    pub fn put_user(&self, user: User) {
        if let Some(mut g) = self.write() {
            g.users.insert(user.id, user);
            g.revision += 1;
        }
    }

    /// Inserts or replaces a guild.
    pub fn put_guild(&self, guild: Guild) {
        if let Some(mut g) = self.write() {
            if !g.guild_order.contains(&guild.id) {
                g.guild_order.push(guild.id);
            }
            g.guilds.insert(guild.id, guild);
            g.revision += 1;
        }
    }

    /// Removes a guild and everything under it.
    pub fn remove_guild(&self, id: Snowflake) {
        if let Some(mut g) = self.write() {
            g.guilds.remove(&id);
            g.guild_order.retain(|gid| *gid != id);
            g.channels.retain(|_, c| c.guild_id != Some(id));
            g.members.retain(|(gid, _), _| *gid != id);
            g.unread.retain(|(gid, _), _| *gid != id);
            g.revision += 1;
        }
    }

    /// Reorders the guild list.
    ///
    /// Guilds absent from `order` are kept at the end rather than dropped, so a partial reordering
    /// cannot silently remove guilds from the rail.
    pub fn set_guild_order(&self, order: Vec<Snowflake>) {
        if let Some(mut g) = self.write() {
            let known: Vec<Snowflake> = g.guilds.keys().copied().collect();
            let mut merged = order;
            for id in known {
                if !merged.contains(&id) {
                    merged.push(id);
                }
            }
            g.guild_order = merged;
            g.revision += 1;
        }
    }

    /// Inserts or replaces a channel.
    pub fn put_channel(&self, channel: Channel) {
        if let Some(mut g) = self.write() {
            g.channels.insert(channel.id, channel);
            g.revision += 1;
        }
    }

    /// Inserts or replaces a message.
    pub fn put_message(&self, message: Message) {
        if let Some(mut g) = self.write() {
            g.messages.insert(message.id, message);
            g.revision += 1;
        }
    }

    /// Removes a message.
    pub fn remove_message(&self, id: Snowflake) {
        if let Some(mut g) = self.write() {
            g.messages.remove(&id);
            g.revision += 1;
        }
    }

    /// Inserts or replaces a member.
    pub fn put_member(&self, member: Member) {
        if let Some(mut g) = self.write() {
            g.members.insert((member.guild_id, member.user_id), member);
            g.revision += 1;
        }
    }

    /// Sets a channel's unread flag.
    pub fn set_unread(&self, guild_id: Snowflake, channel_id: Snowflake, unread: bool) {
        if let Some(mut g) = self.write() {
            if unread {
                g.unread.insert((guild_id, channel_id), true);
            } else {
                g.unread.remove(&(guild_id, channel_id));
            }
            g.revision += 1;
        }
    }

    /// Applies a session event to the cache.
    ///
    /// This is the only mutation path the gateway uses, which keeps every cache change traceable
    /// to an event name.
    pub fn apply(&self, event: &Event) {
        match event {
            Event::Ready { user_id, .. } => self.set_self_id(*user_id),
            Event::GuildCreate {
                guild,
                channels,
                members,
            } => {
                self.put_guild(guild.clone());
                for c in channels {
                    self.put_channel(c.clone());
                }
                for m in members {
                    self.put_member(m.clone());
                }
            }
            Event::GuildDelete { guild_id } => self.remove_guild(*guild_id),
            Event::ChannelCreate { channel } | Event::ChannelUpdate { channel, .. } => {
                self.put_channel(channel.clone());
            }
            Event::ChannelDelete { id, .. } => {
                if let Some(mut g) = self.write() {
                    g.channels.remove(id);
                    g.revision += 1;
                }
            }
            Event::MessageCreate { message } | Event::MessageUpdate { message, .. } => {
                self.put_message(message.clone());
            }
            Event::MessageDelete { id, .. } => self.remove_message(*id),
            Event::GuildMemberAdd { member } => self.put_member(member.clone()),
            Event::UserUpdate { user } => self.put_user(user.clone()),
            Event::TypingStart { .. }
            | Event::PresenceUpdate { .. }
            | Event::Unknown { .. }
            | Event::UnknownDispatch { .. } => {}
        }
    }

    /// Total entity counts, for diagnostics.
    #[must_use]
    pub fn stats(&self) -> CacheStats {
        self.read().map_or(
            CacheStats {
                users: 0,
                guilds: 0,
                channels: 0,
                messages: 0,
                members: 0,
            },
            |g| CacheStats {
                users: g.users.len(),
                guilds: g.guilds.len(),
                channels: g.channels.len(),
                messages: g.messages.len(),
                members: g.members.len(),
            },
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    /// Cached users.
    pub users: usize,
    /// Cached guilds.
    pub guilds: usize,
    /// Cached channels.
    pub channels: usize,
    /// Cached messages.
    pub messages: usize,
    /// Cached members.
    pub members: usize,
}

impl CacheStats {
    /// Total entities across all stores.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.users + self.guilds + self.channels + self.messages + self.members
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: u64) -> User {
        User {
            id: Snowflake::new(id),
            username: format!("user{id}"),
            global_name: None,
            avatar: None,
            is_bot: false,
            deleted: false,
        }
    }

    fn guild(id: u64) -> Guild {
        Guild {
            id: Snowflake::new(id),
            name: format!("guild{id}"),
            icon: None,
            owner_id: None,
            member_count: 0,
        }
    }

    /// A DM channel has no guild and encodes the recipient in its name, so it can still be found
    /// by user id the way `ChannelStore.getDMFromUserId` does upstream.
    fn channel(id: u64, guild: Option<u64>, position: u32) -> Channel {
        Channel {
            id: Snowflake::new(id),
            guild_id: guild.map(Snowflake::new),
            name: Some(match guild {
                Some(_) => format!("chan{id}"),
                None => Snowflake::new(id).to_string(),
            }),
            kind: 0,
            parent_id: None,
            position,
        }
    }

    fn message(id: u64, channel: u64) -> Message {
        Message {
            id: Snowflake::new(id),
            channel_id: Snowflake::new(channel),
            author_id: Snowflake::new(1),
            content: "hi".to_owned(),
            guild_id: None,
            mentions: vec![],
            timestamp: None,
        }
    }

    fn member(guild: u64, user: u64) -> Member {
        Member {
            guild_id: Snowflake::new(guild),
            user_id: Snowflake::new(user),
            nick: None,
            roles: vec![],
        }
    }

    #[test]
    fn empty_cache_reports_zero() {
        let cache = Cache::new();
        assert_eq!(cache.stats().total(), 0);
        assert!(cache.user(Snowflake::new(1)).is_none());
        assert_eq!(cache.revision(), 0);
    }

    #[test]
    fn put_and_read_a_user() {
        let cache = Cache::new();
        cache.put_user(user(1));
        assert_eq!(cache.user(Snowflake::new(1)).unwrap().username, "user1");
        assert!(cache.revision() > 0, "a write must bump the revision");
    }

    #[test]
    fn display_name_prefers_global_name() {
        let mut u = user(1);
        assert_eq!(u.display_name(), "user1");
        u.global_name = Some("Pretty".to_owned());
        assert_eq!(u.display_name(), "Pretty");
    }

    #[test]
    fn guilds_keep_insertion_order_and_can_be_reordered() {
        let cache = Cache::new();
        cache.put_guild(guild(1));
        cache.put_guild(guild(2));
        cache.put_guild(guild(3));
        let ids: Vec<u64> = cache.guilds().iter().map(|g| g.id.raw()).collect();
        assert_eq!(ids, vec![1, 2, 3]);

        cache.set_guild_order(vec![Snowflake::new(3), Snowflake::new(1)]);
        let ids: Vec<u64> = cache.guilds().iter().map(|g| g.id.raw()).collect();
        // 2 was not in the new order and must not be dropped.
        assert_eq!(ids.len(), 3);
        assert_eq!(&ids[..2], &[3, 1]);
    }

    #[test]
    fn removing_a_guild_cascades() {
        let cache = Cache::new();
        cache.put_guild(guild(1));
        cache.put_channel(channel(10, Some(1), 0));
        cache.put_member(member(1, 100));
        cache.set_unread(Snowflake::new(1), Snowflake::new(10), true);

        cache.remove_guild(Snowflake::new(1));

        assert!(cache.guild(Snowflake::new(1)).is_none());
        assert!(
            cache.channel(Snowflake::new(10)).is_none(),
            "channels must cascade"
        );
        assert!(
            cache
                .member(Snowflake::new(1), Snowflake::new(100))
                .is_none(),
            "members must cascade"
        );
        assert!(!cache.is_unread(Snowflake::new(1), Snowflake::new(10)));
    }

    #[test]
    fn guild_channels_sort_by_position() {
        let cache = Cache::new();
        cache.put_channel(channel(12, Some(1), 2));
        cache.put_channel(channel(10, Some(1), 0));
        cache.put_channel(channel(11, Some(1), 1));
        cache.put_channel(channel(99, Some(2), 0));
        let ids: Vec<u64> = cache
            .guild_channels(Snowflake::new(1))
            .iter()
            .map(|c| c.id.raw())
            .collect();
        assert_eq!(ids, vec![10, 11, 12]);
    }

    #[test]
    fn dm_lookup_by_user_id() {
        let cache = Cache::new();
        // A DM has no guild and its name encodes the recipient.
        let mut dm = channel(0, None, 0);
        dm.name = Some("4242".to_owned());
        cache.put_channel(dm);
        assert!(cache.dm_from_user_id(Snowflake::new(4242)).is_some());
        assert!(cache.dm_from_user_id(Snowflake::new(1)).is_none());
    }

    #[test]
    fn messages_sort_oldest_first() {
        let cache = Cache::new();
        cache.put_message(message(30, 10));
        cache.put_message(message(10, 10));
        cache.put_message(message(20, 10));
        cache.put_message(message(99, 11));
        let ids: Vec<u64> = cache
            .channel_messages(Snowflake::new(10))
            .iter()
            .map(|m| m.id.raw())
            .collect();
        assert_eq!(ids, vec![10, 20, 30]);
    }

    #[test]
    fn unread_defaults_to_false_and_is_cleared() {
        let cache = Cache::new();
        assert!(!cache.is_unread(Snowflake::new(1), Snowflake::new(10)));
        cache.set_unread(Snowflake::new(1), Snowflake::new(10), true);
        assert!(cache.is_unread(Snowflake::new(1), Snowflake::new(10)));
        cache.set_unread(Snowflake::new(1), Snowflake::new(10), false);
        assert!(!cache.is_unread(Snowflake::new(1), Snowflake::new(10)));
    }

    #[test]
    fn apply_ready_sets_self_id() {
        let cache = Cache::new();
        cache.apply(&Event::Ready {
            user_id: Snowflake::new(7),
            session_id: "s".to_owned(),
        });
        assert_eq!(cache.self_id(), Snowflake::new(7));
    }

    #[test]
    fn apply_guild_create_populates_every_store() {
        let cache = Cache::new();
        cache.apply(&Event::GuildCreate {
            guild: guild(1),
            channels: vec![channel(10, Some(1), 0)],
            members: vec![member(1, 100)],
        });
        assert_eq!(cache.stats().guilds, 1);
        assert_eq!(cache.stats().channels, 1);
        assert_eq!(cache.stats().members, 1);
    }

    #[test]
    fn apply_message_delete_removes_it() {
        let cache = Cache::new();
        cache.put_message(message(10, 1));
        cache.apply(&Event::MessageDelete {
            id: Snowflake::new(10),
            channel_id: Snowflake::new(1),
        });
        assert!(cache.message(Snowflake::new(10)).is_none());
    }

    #[test]
    fn apply_channel_delete_removes_it() {
        let cache = Cache::new();
        cache.put_channel(channel(10, Some(1), 0));
        cache.apply(&Event::ChannelDelete {
            id: Snowflake::new(10),
            guild_id: Snowflake::new(1),
        });
        assert!(cache.channel(Snowflake::new(10)).is_none());
    }

    #[test]
    fn unhandled_events_do_not_mutate() {
        let cache = Cache::new();
        cache.put_guild(guild(1));
        let before = cache.revision();
        cache.apply(&Event::TypingStart {
            channel_id: Snowflake::new(1),
            user_id: Snowflake::new(2),
        });
        cache.apply(&Event::Unknown {
            name: "SOMETHING_NEW".to_owned(),
        });
        assert_eq!(cache.revision(), before, "no mutation should have occurred");
    }

    #[test]
    fn every_supported_store_is_declared() {
        for id in SUPPORTED_STORES {
            assert!(id.starts_with("stores."), "unexpected store id {id}");
        }
        assert!(SUPPORTED_STORES.contains(&"stores.ChannelStore"));
        assert!(SUPPORTED_STORES.contains(&"stores.GuildStore"));
        assert!(SUPPORTED_STORES.contains(&"stores.UserStore"));
    }

    #[test]
    fn revision_advances_on_every_write() {
        let cache = Cache::new();
        let start = cache.revision();
        cache.put_user(user(1));
        let a = cache.revision();
        assert!(a > start);
        cache.set_unread(Snowflake::new(1), Snowflake::new(2), true);
        assert!(cache.revision() > a);
    }
}
