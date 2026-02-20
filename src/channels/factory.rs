#[cfg(feature = "email")]
use crate::channels::EmailChannel;
use crate::channels::traits::Channel;
use crate::channels::{
    DiscordChannel, IMessageChannel, IrcChannel, MatrixChannel, SlackChannel, TelegramChannel,
    WhatsAppChannel,
};
use crate::config::ChannelsConfig;
use std::sync::Arc;

/// Build channel instances from configuration, returning each with its display name.
///
/// Callers that only need the channels (without names) can discard the labels
/// via `.into_iter().map(|(_, ch)| ch).collect()`.
pub fn build_channels(channels_config: &ChannelsConfig) -> Vec<(&'static str, Arc<dyn Channel>)> {
    let mut channels: Vec<(&'static str, Arc<dyn Channel>)> = Vec::with_capacity(8);

    if let Some(ref tg) = channels_config.telegram {
        channels.push((
            "Telegram",
            Arc::new(TelegramChannel::new(
                tg.bot_token.clone(),
                tg.allowed_users.clone(),
            )),
        ));
    }

    if let Some(ref dc) = channels_config.discord {
        channels.push((
            "Discord",
            Arc::new(DiscordChannel::new(
                dc.bot_token.clone(),
                dc.guild_id.clone(),
                dc.allowed_users.clone(),
            )),
        ));
    }

    if let Some(ref sl) = channels_config.slack {
        channels.push((
            "Slack",
            Arc::new(SlackChannel::new(
                sl.bot_token.clone(),
                sl.channel_id.clone(),
                sl.allowed_users.clone(),
            )),
        ));
    }

    if let Some(ref im) = channels_config.imessage {
        channels.push((
            "iMessage",
            Arc::new(IMessageChannel::new(im.allowed_contacts.clone())),
        ));
    }

    if let Some(ref mx) = channels_config.matrix {
        channels.push((
            "Matrix",
            Arc::new(MatrixChannel::new(
                mx.homeserver.clone(),
                mx.access_token.clone(),
                mx.room_id.clone(),
                mx.allowed_users.clone(),
            )),
        ));
    }

    if let Some(ref wa) = channels_config.whatsapp {
        channels.push((
            "WhatsApp",
            Arc::new(WhatsAppChannel::new(
                wa.access_token.clone(),
                wa.phone_number_id.clone(),
                wa.verify_token.clone(),
                wa.allowed_numbers.clone(),
            )),
        ));
    }

    #[cfg(feature = "email")]
    if let Some(ref email_cfg) = channels_config.email {
        channels.push(("Email", Arc::new(EmailChannel::new(email_cfg.clone()))));
    }

    if let Some(ref irc) = channels_config.irc {
        channels.push((
            "IRC",
            Arc::new(IrcChannel::new(
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
        ));
    }

    channels
}
