#[cfg(feature = "email")]
use crate::channels::EmailChannel;
use crate::channels::policy::{ChannelEntry, ChannelPolicy};
use crate::channels::{
    DiscordChannel, IMessageChannel, IrcChannel, MatrixChannel, SlackChannel, TelegramChannel,
    WhatsAppChannel,
};
use crate::config::ChannelsConfig;
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

pub fn build_channels(channels_config: &ChannelsConfig) -> Vec<ChannelEntry> {
    let mut channels = Vec::with_capacity(8);

    if let Some(ref tg) = channels_config.telegram {
        channels.push(ChannelEntry {
            name: "Telegram",
            channel: Arc::new(TelegramChannel::new(
                tg.bot_token.clone(),
                tg.allowed_users.clone(),
            )),
            policy: build_policy(tg.autonomy_level, tg.tool_allowlist.clone()),
        });
    }

    if let Some(ref dc) = channels_config.discord {
        channels.push(ChannelEntry {
            name: "Discord",
            channel: Arc::new(DiscordChannel::new(
                dc.bot_token.clone(),
                dc.guild_id.clone(),
                dc.allowed_users.clone(),
            )),
            policy: build_policy(dc.autonomy_level, dc.tool_allowlist.clone()),
        });
    }

    if let Some(ref sl) = channels_config.slack {
        channels.push(ChannelEntry {
            name: "Slack",
            channel: Arc::new(SlackChannel::new(
                sl.bot_token.clone(),
                sl.channel_id.clone(),
                sl.allowed_users.clone(),
            )),
            policy: build_policy(sl.autonomy_level, sl.tool_allowlist.clone()),
        });
    }

    if let Some(ref im) = channels_config.imessage {
        channels.push(ChannelEntry {
            name: "iMessage",
            channel: Arc::new(IMessageChannel::new(im.allowed_contacts.clone())),
            policy: build_policy(im.autonomy_level, im.tool_allowlist.clone()),
        });
    }

    if let Some(ref mx) = channels_config.matrix {
        channels.push(ChannelEntry {
            name: "Matrix",
            channel: Arc::new(MatrixChannel::new(
                mx.homeserver.clone(),
                mx.access_token.clone(),
                mx.room_id.clone(),
                mx.allowed_users.clone(),
            )),
            policy: build_policy(mx.autonomy_level, mx.tool_allowlist.clone()),
        });
    }

    if let Some(ref wa) = channels_config.whatsapp {
        channels.push(ChannelEntry {
            name: "WhatsApp",
            channel: Arc::new(WhatsAppChannel::new(
                wa.access_token.clone(),
                wa.phone_number_id.clone(),
                wa.verify_token.clone(),
                wa.allowed_numbers.clone(),
            )),
            policy: build_policy(wa.autonomy_level, wa.tool_allowlist.clone()),
        });
    }

    #[cfg(feature = "email")]
    if let Some(ref email_cfg) = channels_config.email {
        channels.push(ChannelEntry {
            name: "Email",
            channel: Arc::new(EmailChannel::new(email_cfg.clone())),
            policy: build_policy(None, None),
        });
    }

    if let Some(ref irc) = channels_config.irc {
        channels.push(ChannelEntry {
            name: "IRC",
            channel: Arc::new(IrcChannel::new(
                irc.server.clone(),
                irc.port,
                irc.nickname.clone(),
                irc.username.clone(),
                irc.channels.clone(),
                irc.allowed_users.clone(),
                irc.server_password.clone(),
                irc.nickserv_password.clone(),
                irc.sasl_password.clone(),
                irc.verify_tls.unwrap_or(true),
            )),
            policy: build_policy(irc.autonomy_level, irc.tool_allowlist.clone()),
        });
    }

    channels
}
