use crate::config::ChannelsConfig;
#[cfg(feature = "discord")]
use crate::transport::channels::DiscordChannel;
#[cfg(feature = "email")]
use crate::transport::channels::EmailChannel;
use crate::transport::channels::policy::{ChannelEntry, ChannelPolicy};
use crate::transport::channels::{
    IMessageChannel, IrcChannel, IrcChannelConfig, MatrixChannel, SlackChannel, TelegramChannel,
    WhatsAppChannel,
};
use std::collections::HashSet;
use std::sync::Arc;

fn build_policy(
    autonomy_level: Option<crate::security::AutonomyLevel>,
    tool_allowlist: Option<Vec<String>>,
) -> ChannelPolicy {
    ChannelPolicy {
        autonomy_level,
        tool_allowlist: tool_allowlist.map(|tools| tools.into_iter().collect::<HashSet<_>>()),
    }
}

pub fn build_channels(channels_config: ChannelsConfig) -> Vec<ChannelEntry> {
    let mut channels = Vec::with_capacity(8);

    if let Some(tg) = channels_config.telegram {
        channels.push(ChannelEntry {
            name: "Telegram",
            channel: Arc::new(TelegramChannel::new(tg.bot_token, tg.allowed_users)),
            policy: build_policy(tg.autonomy_level, tg.tool_allowlist),
        });
    }

    #[cfg(feature = "discord")]
    if let Some(dc) = channels_config.discord {
        let policy = build_policy(dc.autonomy_level, dc.tool_allowlist.clone());
        channels.push(ChannelEntry {
            name: "Discord",
            channel: Arc::new(DiscordChannel::new(dc)),
            policy,
        });
    }

    if let Some(sl) = channels_config.slack {
        channels.push(ChannelEntry {
            name: "Slack",
            channel: Arc::new(SlackChannel::new(
                sl.bot_token,
                sl.channel_id,
                sl.allowed_users,
            )),
            policy: build_policy(sl.autonomy_level, sl.tool_allowlist),
        });
    }

    if let Some(im) = channels_config.imessage {
        channels.push(ChannelEntry {
            name: "iMessage",
            channel: Arc::new(IMessageChannel::new(im.allowed_contacts)),
            policy: build_policy(im.autonomy_level, im.tool_allowlist),
        });
    }

    if let Some(mx) = channels_config.matrix {
        channels.push(ChannelEntry {
            name: "Matrix",
            channel: Arc::new(MatrixChannel::new(
                mx.homeserver,
                mx.access_token,
                mx.room_id,
                mx.allowed_users,
            )),
            policy: build_policy(mx.autonomy_level, mx.tool_allowlist),
        });
    }

    if let Some(wa) = channels_config.whatsapp {
        channels.push(ChannelEntry {
            name: "WhatsApp",
            channel: Arc::new(WhatsAppChannel::new(
                wa.access_token,
                wa.phone_number_id,
                wa.verify_token,
                wa.allowed_numbers,
            )),
            policy: build_policy(wa.autonomy_level, wa.tool_allowlist),
        });
    }

    #[cfg(feature = "email")]
    if let Some(email_cfg) = channels_config.email {
        channels.push(ChannelEntry {
            name: "Email",
            channel: Arc::new(EmailChannel::new(email_cfg)),
            policy: build_policy(None, None),
        });
    }

    if let Some(irc) = channels_config.irc {
        channels.push(ChannelEntry {
            name: "IRC",
            channel: Arc::new(IrcChannel::new(IrcChannelConfig {
                server: irc.server,
                port: irc.port,
                nickname: irc.nickname,
                username: irc.username,
                channels: irc.channels,
                allowed_users: irc.allowed_users,
                server_password: irc.server_password,
                nickserv_password: irc.nickserv_password,
                sasl_password: irc.sasl_password,
                verify_tls: irc.verify_tls.unwrap_or(true),
            })),
            policy: build_policy(irc.autonomy_level, irc.tool_allowlist),
        });
    }

    channels
}
