use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::super::state::{ChannelSubStep, WizardState};
use super::super::theme;
use super::super::widgets::{SelectListWidget, Spinner, SpinnerWidget, TextInputWidget};

pub struct ChannelsStep<'a> {
    pub state: &'a WizardState,
    pub spinner: &'a Spinner,
}

impl Widget for ChannelsStep<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 {
            return;
        }

        if self.state.channel_sub_step == ChannelSubStep::Picker {
            let header = Line::from(vec![
                Span::styled("  ", theme::dim_style()),
                Span::styled(t!("onboard.channels.intro").to_string(), theme::dim_style()),
            ]);
            header.render(Rect::new(area.x, area.y, area.width, 1), buf);

            let list_area = Rect::new(
                area.x,
                area.y + 2,
                area.width,
                area.height.saturating_sub(2),
            );
            SelectListWidget::new(&self.state.channel_picker, true).render(list_area, buf);
        } else {
            // Per-channel configuration screens
            let (title, prompt) = channel_sub_step_info(self.state.channel_sub_step);

            let header = Line::from(Span::styled(format!("  {title}"), theme::heading_style()));
            header.render(Rect::new(area.x, area.y, area.width, 1), buf);

            // Connection testing state
            if self.state.channel_connection_testing {
                let spinner_area = Rect::new(area.x, area.y + 2, area.width, 1);
                SpinnerWidget::new(self.spinner, &t!("onboard.channels.testing"))
                    .render(spinner_area, buf);
                return;
            }

            // Connection result
            if let Some(ref result) = self.state.channel_connection_result {
                let y = area.y + 2;
                let (msg, style) = match result {
                    Ok(name) => (
                        t!("onboard.channels.test_success", name = name).to_string(),
                        theme::success_style(),
                    ),
                    Err(e) => (e.clone(), theme::error_style()),
                };
                let result_line = Line::from(Span::styled(format!("  {msg}"), style));
                result_line.render(Rect::new(area.x, y, area.width, 1), buf);

                let input_area = Rect::new(area.x, y + 2, area.width, 1);
                TextInputWidget::new(&self.state.channel_text_input, prompt, true)
                    .render(input_area, buf);
            } else {
                let input_area = Rect::new(area.x, area.y + 2, area.width, 1);
                TextInputWidget::new(&self.state.channel_text_input, prompt, true)
                    .render(input_area, buf);
            }
        }
    }
}

fn channel_sub_step_info(sub: ChannelSubStep) -> (&'static str, &'static str) {
    match sub {
        ChannelSubStep::Picker => ("", ""),
        ChannelSubStep::TelegramToken => ("Telegram", "Bot token"),
        ChannelSubStep::TelegramAllowlist => ("Telegram", "Allowed users"),
        ChannelSubStep::DiscordToken => ("Discord", "Bot token"),
        ChannelSubStep::DiscordGuild => ("Discord", "Guild ID"),
        ChannelSubStep::DiscordAllowlist => ("Discord", "Allowed users"),
        ChannelSubStep::SlackToken => ("Slack", "Bot token"),
        ChannelSubStep::SlackAppToken => ("Slack", "App token"),
        ChannelSubStep::SlackChannel => ("Slack", "Channel ID"),
        ChannelSubStep::SlackAllowlist => ("Slack", "Allowed users"),
        ChannelSubStep::MatrixHomeserver => ("Matrix", "Homeserver URL"),
        ChannelSubStep::MatrixToken => ("Matrix", "Access token"),
        ChannelSubStep::MatrixRoom => ("Matrix", "Room ID"),
        ChannelSubStep::MatrixAllowlist => ("Matrix", "Allowed users"),
        ChannelSubStep::WhatsAppToken => ("WhatsApp", "Access token"),
        ChannelSubStep::WhatsAppPhone => ("WhatsApp", "Phone number ID"),
        ChannelSubStep::WhatsAppVerify => ("WhatsApp", "Verify token"),
        ChannelSubStep::WhatsAppAllowlist => ("WhatsApp", "Allowed numbers"),
        ChannelSubStep::IrcServer => ("IRC", "Server"),
        ChannelSubStep::IrcPort => ("IRC", "Port"),
        ChannelSubStep::IrcNick => ("IRC", "Nickname"),
        ChannelSubStep::IrcChannels => ("IRC", "Channels"),
        ChannelSubStep::IrcAllowlist => ("IRC", "Allowed users"),
        ChannelSubStep::WebhookPort => ("Webhook", "Port"),
        ChannelSubStep::WebhookSecret => ("Webhook", "Secret"),
        ChannelSubStep::IMessageContacts => ("iMessage", "Contacts"),
    }
}
