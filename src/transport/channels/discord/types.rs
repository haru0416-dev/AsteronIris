//! Discord API constants and type definitions.

/// Discord API base URL (v10).
pub const API_BASE: &str = "https://discord.com/api/v10";

/// Default Gateway intents bitmask.
///
/// GUILDS (1) | `GUILD_MESSAGES` (512) | `GUILD_MESSAGE_REACTIONS` (1024)
/// | `DIRECT_MESSAGES` (4096) | `MESSAGE_CONTENT` (32768) = 38401
pub const DEFAULT_INTENTS: u64 = 38401;

/// Default heartbeat interval when server does not provide one (ms).
pub const DEFAULT_HEARTBEAT_INTERVAL_MS: u64 = 41250;

/// Discord maximum message length (characters).
pub const MAX_MESSAGE_LENGTH: usize = 2000;

/// Gateway rate limit: maximum identify requests per 24 hours.
pub const MAX_IDENTIFIES_PER_DAY: u32 = 1000;

/// Gateway rate limit: maximum commands per 60 seconds (excluding heartbeats).
pub const MAX_GATEWAY_COMMANDS_PER_MINUTE: u32 = 120;

/// Gateway opcodes used in the Discord WebSocket protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GatewayOpcode {
    /// An event was dispatched (server → client).
    Dispatch = 0,
    /// Fired periodically to keep the connection alive.
    Heartbeat = 1,
    /// Starts a new session during the initial handshake.
    Identify = 2,
    /// Update the client's presence.
    PresenceUpdate = 3,
    /// Join/leave or move between voice channels.
    VoiceStateUpdate = 4,
    /// Resume a previous session that was disconnected.
    Resume = 6,
    /// Server is telling the client to reconnect.
    Reconnect = 7,
    /// Request information about offline guild members.
    RequestGuildMembers = 8,
    /// The session has been invalidated.
    InvalidSession = 9,
    /// Sent immediately after connecting; contains heartbeat interval.
    Hello = 10,
    /// Acknowledges a received heartbeat.
    HeartbeatAck = 11,
}

impl GatewayOpcode {
    /// Convert a raw u64 value to an opcode, if valid.
    pub fn from_u64(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::Dispatch),
            1 => Some(Self::Heartbeat),
            2 => Some(Self::Identify),
            3 => Some(Self::PresenceUpdate),
            4 => Some(Self::VoiceStateUpdate),
            6 => Some(Self::Resume),
            7 => Some(Self::Reconnect),
            8 => Some(Self::RequestGuildMembers),
            9 => Some(Self::InvalidSession),
            10 => Some(Self::Hello),
            11 => Some(Self::HeartbeatAck),
            _ => None,
        }
    }
}

/// Discord interaction types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InteractionType {
    Ping = 1,
    ApplicationCommand = 2,
    MessageComponent = 3,
    ApplicationCommandAutocomplete = 4,
    ModalSubmit = 5,
}

impl InteractionType {
    pub fn from_u64(value: u64) -> Option<Self> {
        match value {
            1 => Some(Self::Ping),
            2 => Some(Self::ApplicationCommand),
            3 => Some(Self::MessageComponent),
            4 => Some(Self::ApplicationCommandAutocomplete),
            5 => Some(Self::ModalSubmit),
            _ => None,
        }
    }
}

/// Interaction callback types for responding to interactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InteractionCallbackType {
    /// ACK a Ping.
    Pong = 1,
    /// Respond to an interaction with a message.
    ChannelMessageWithSource = 4,
    /// ACK an interaction and edit a response later (shows "thinking...").
    DeferredChannelMessageWithSource = 5,
    /// For components: ACK an interaction and edit the original message later.
    DeferredUpdateMessage = 6,
    /// For components: edit the message the component was attached to.
    UpdateMessage = 7,
}

/// Discord channel types relevant for message routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DiscordChannelType {
    GuildText = 0,
    Dm = 1,
    GuildVoice = 2,
    GroupDm = 3,
    GuildCategory = 4,
    GuildAnnouncement = 5,
    AnnouncementThread = 10,
    PublicThread = 11,
    PrivateThread = 12,
    GuildStageVoice = 13,
    GuildForum = 15,
    GuildMedia = 16,
}

impl DiscordChannelType {
    pub fn from_u64(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::GuildText),
            1 => Some(Self::Dm),
            2 => Some(Self::GuildVoice),
            3 => Some(Self::GroupDm),
            4 => Some(Self::GuildCategory),
            5 => Some(Self::GuildAnnouncement),
            10 => Some(Self::AnnouncementThread),
            11 => Some(Self::PublicThread),
            12 => Some(Self::PrivateThread),
            13 => Some(Self::GuildStageVoice),
            15 => Some(Self::GuildForum),
            16 => Some(Self::GuildMedia),
            _ => None,
        }
    }

    /// Whether this channel type represents a thread.
    pub fn is_thread(self) -> bool {
        matches!(
            self,
            Self::AnnouncementThread | Self::PublicThread | Self::PrivateThread
        )
    }

    /// Whether this channel type is a voice channel (including stage).
    pub fn is_voice(self) -> bool {
        matches!(self, Self::GuildVoice | Self::GuildStageVoice)
    }
}

/// Individual intent bit flags.
pub mod intents {
    pub const GUILDS: u64 = 1 << 0;
    pub const GUILD_MEMBERS: u64 = 1 << 1;
    pub const GUILD_MODERATION: u64 = 1 << 2;
    pub const GUILD_EXPRESSIONS: u64 = 1 << 3;
    pub const GUILD_INTEGRATIONS: u64 = 1 << 4;
    pub const GUILD_WEBHOOKS: u64 = 1 << 5;
    pub const GUILD_INVITES: u64 = 1 << 6;
    pub const GUILD_VOICE_STATES: u64 = 1 << 7;
    pub const GUILD_PRESENCES: u64 = 1 << 8;
    pub const GUILD_MESSAGES: u64 = 1 << 9;
    pub const GUILD_MESSAGE_REACTIONS: u64 = 1 << 10;
    pub const GUILD_MESSAGE_TYPING: u64 = 1 << 11;
    pub const DIRECT_MESSAGES: u64 = 1 << 12;
    pub const DIRECT_MESSAGE_REACTIONS: u64 = 1 << 13;
    pub const DIRECT_MESSAGE_TYPING: u64 = 1 << 14;
    pub const MESSAGE_CONTENT: u64 = 1 << 15;
    pub const GUILD_SCHEDULED_EVENTS: u64 = 1 << 16;
    pub const AUTO_MODERATION_CONFIGURATION: u64 = 1 << 20;
    pub const AUTO_MODERATION_EXECUTION: u64 = 1 << 21;
}

/// Activity type for bot presence display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ActivityType {
    Playing = 0,
    Streaming = 1,
    Listening = 2,
    Watching = 3,
    Custom = 4,
    Competing = 5,
}

impl ActivityType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Playing),
            1 => Some(Self::Streaming),
            2 => Some(Self::Listening),
            3 => Some(Self::Watching),
            4 => Some(Self::Custom),
            5 => Some(Self::Competing),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_intents_match_expected_flags() {
        assert_ne!(DEFAULT_INTENTS & intents::GUILDS, 0, "GUILDS");
        assert_ne!(
            DEFAULT_INTENTS & intents::GUILD_MESSAGES,
            0,
            "GUILD_MESSAGES"
        );
        assert_ne!(
            DEFAULT_INTENTS & intents::GUILD_MESSAGE_REACTIONS,
            0,
            "GUILD_MESSAGE_REACTIONS"
        );
        assert_ne!(
            DEFAULT_INTENTS & intents::DIRECT_MESSAGES,
            0,
            "DIRECT_MESSAGES"
        );
        assert_ne!(
            DEFAULT_INTENTS & intents::MESSAGE_CONTENT,
            0,
            "MESSAGE_CONTENT"
        );
    }

    #[test]
    fn opcode_roundtrip() {
        for v in [0, 1, 2, 3, 4, 6, 7, 8, 9, 10, 11] {
            assert!(GatewayOpcode::from_u64(v).is_some(), "opcode {v}");
        }
        assert!(GatewayOpcode::from_u64(5).is_none());
        assert!(GatewayOpcode::from_u64(99).is_none());
    }

    #[test]
    fn channel_type_thread_detection() {
        assert!(DiscordChannelType::PublicThread.is_thread());
        assert!(DiscordChannelType::PrivateThread.is_thread());
        assert!(DiscordChannelType::AnnouncementThread.is_thread());
        assert!(!DiscordChannelType::GuildText.is_thread());
        assert!(!DiscordChannelType::Dm.is_thread());
    }

    #[test]
    fn channel_type_voice_detection() {
        assert!(DiscordChannelType::GuildVoice.is_voice());
        assert!(DiscordChannelType::GuildStageVoice.is_voice());
        assert!(!DiscordChannelType::GuildText.is_voice());
        assert!(!DiscordChannelType::PublicThread.is_voice());
    }

    #[test]
    fn interaction_type_roundtrip() {
        assert_eq!(
            InteractionType::from_u64(2),
            Some(InteractionType::ApplicationCommand)
        );
        assert!(InteractionType::from_u64(0).is_none());
        assert!(InteractionType::from_u64(99).is_none());
    }

    #[test]
    fn activity_type_roundtrip() {
        assert_eq!(ActivityType::from_u8(0), Some(ActivityType::Playing));
        assert_eq!(ActivityType::from_u8(3), Some(ActivityType::Watching));
        assert!(ActivityType::from_u8(99).is_none());
    }
}
