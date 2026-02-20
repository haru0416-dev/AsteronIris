use crossterm::event::KeyCode;

use crate::onboard::tui::state::{ChannelSubStep, ProviderSubStep, WizardState, WizardStep};

use super::data::{
    model_id_for_selection, model_list_for_provider, provider_id_for_selection,
    provider_list_for_tier,
};

pub(super) fn is_text_input_active(state: &WizardState) -> bool {
    match state.current_step {
        WizardStep::Workspace => !state.workspace_use_default.value,
        WizardStep::Provider => matches!(
            state.provider_sub_step,
            ProviderSubStep::ApiKey
                | ProviderSubStep::CustomBaseUrl
                | ProviderSubStep::CustomApiKey
                | ProviderSubStep::CustomModel
        ),
        WizardStep::Channels => !matches!(state.channel_sub_step, ChannelSubStep::Picker),
        WizardStep::Tunnel => matches!(state.tunnel_select.selected, 1 | 3 | 4),
        WizardStep::ToolMode => state.tool_mode_select.selected == 1,
        WizardStep::Context => matches!(state.context_sub_field, 0 | 2 | 4),
        _ => false,
    }
}

pub(super) fn handle_key(state: &mut WizardState, key: KeyCode) {
    match state.current_step {
        WizardStep::Workspace => handle_workspace_key(state, key),
        WizardStep::Provider => handle_provider_key(state, key),
        WizardStep::Channels => handle_channels_key(state, key),
        WizardStep::Tunnel => handle_tunnel_key(state, key),
        WizardStep::ToolMode => handle_tool_mode_key(state, key),
        WizardStep::Memory => handle_memory_key(state, key),
        WizardStep::Context => handle_context_key(state, key),
        WizardStep::Summary => handle_summary_key(state, key),
    }
}

fn handle_workspace_key(state: &mut WizardState, key: KeyCode) {
    if state.workspace_use_default.value {
        // Toggle mode — arrows/space/enter
        match key {
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                state.workspace_use_default.toggle();
            }
            KeyCode::Enter => state.next_step(),
            KeyCode::Esc => state.prev_step(),
            _ => {}
        }
    } else {
        // Text input mode
        match key {
            KeyCode::Enter => {
                if !state.workspace_custom_path.is_empty() {
                    let path = state.workspace_custom_path.value.clone();
                    let expanded = shellexpand::tilde(&path).to_string();
                    state.workspace_dir = format!("{expanded}/workspace");
                    state.config_path = format!("{expanded}/config.toml");
                    state.next_step();
                }
            }
            KeyCode::Esc => {
                state.workspace_use_default.value = true;
            }
            KeyCode::Char(c) => state.workspace_custom_path.insert(c),
            KeyCode::Backspace => state.workspace_custom_path.backspace(),
            KeyCode::Delete => state.workspace_custom_path.delete(),
            KeyCode::Left => state.workspace_custom_path.move_left(),
            KeyCode::Right => state.workspace_custom_path.move_right(),
            KeyCode::Home => state.workspace_custom_path.home(),
            KeyCode::End => state.workspace_custom_path.end(),
            _ => {}
        }
    }
}

#[allow(clippy::too_many_lines)]
fn handle_provider_key(state: &mut WizardState, key: KeyCode) {
    match state.provider_sub_step {
        ProviderSubStep::TierSelect => match key {
            KeyCode::Up => state.provider_tier_select.up(),
            KeyCode::Down => state.provider_tier_select.down(),
            KeyCode::Enter => {
                let tier = state.provider_tier_select.selected;
                if tier == 5 {
                    // Custom
                    state.provider_sub_step = ProviderSubStep::CustomBaseUrl;
                } else {
                    let providers = provider_list_for_tier(tier);
                    state.provider_select.set_items(providers);
                    state.provider_sub_step = ProviderSubStep::ProviderSelect;
                }
            }
            KeyCode::Esc => state.prev_step(),
            _ => {}
        },
        ProviderSubStep::ProviderSelect => match key {
            KeyCode::Up => state.provider_select.up(),
            KeyCode::Down => state.provider_select.down(),
            KeyCode::Enter => {
                let provider_name = provider_id_for_selection(
                    state.provider_tier_select.selected,
                    state.provider_select.selected,
                );
                state.selected_provider.clone_from(&provider_name);

                if provider_name == "ollama" {
                    state.selected_api_key.clear();
                    let models = model_list_for_provider(&provider_name);
                    state.provider_model_select.set_items(models);
                    state.provider_sub_step = ProviderSubStep::ModelSelect;
                } else {
                    state.provider_sub_step = ProviderSubStep::ApiKey;
                }
            }
            KeyCode::Esc => state.provider_sub_step = ProviderSubStep::TierSelect,
            _ => {}
        },
        ProviderSubStep::ApiKey => match key {
            KeyCode::Enter => {
                state.selected_api_key = state.provider_api_key.value.clone();
                let models = model_list_for_provider(&state.selected_provider);
                state.provider_model_select.set_items(models);
                state.provider_sub_step = ProviderSubStep::ModelSelect;
            }
            KeyCode::Esc => state.provider_sub_step = ProviderSubStep::ProviderSelect,
            KeyCode::Char(c) => state.provider_api_key.insert(c),
            KeyCode::Backspace => state.provider_api_key.backspace(),
            KeyCode::Delete => state.provider_api_key.delete(),
            KeyCode::Left => state.provider_api_key.move_left(),
            KeyCode::Right => state.provider_api_key.move_right(),
            _ => {}
        },
        ProviderSubStep::ModelSelect => match key {
            KeyCode::Up => state.provider_model_select.up(),
            KeyCode::Down => state.provider_model_select.down(),
            KeyCode::Enter => {
                let model_id = model_id_for_selection(
                    &state.selected_provider,
                    state.provider_model_select.selected,
                );
                state.selected_model = model_id;
                state.next_step();
            }
            KeyCode::Esc => state.provider_sub_step = ProviderSubStep::ApiKey,
            _ => {}
        },
        ProviderSubStep::CustomBaseUrl => match key {
            KeyCode::Enter => {
                if !state.provider_custom_base_url.is_empty() {
                    state.provider_sub_step = ProviderSubStep::CustomApiKey;
                }
            }
            KeyCode::Esc => state.provider_sub_step = ProviderSubStep::TierSelect,
            KeyCode::Char(c) => state.provider_custom_base_url.insert(c),
            KeyCode::Backspace => state.provider_custom_base_url.backspace(),
            KeyCode::Delete => state.provider_custom_base_url.delete(),
            KeyCode::Left => state.provider_custom_base_url.move_left(),
            KeyCode::Right => state.provider_custom_base_url.move_right(),
            _ => {}
        },
        ProviderSubStep::CustomApiKey => match key {
            KeyCode::Enter => {
                state.selected_api_key = state.provider_custom_api_key.value.clone();
                state.provider_sub_step = ProviderSubStep::CustomModel;
            }
            KeyCode::Esc => state.provider_sub_step = ProviderSubStep::CustomBaseUrl,
            KeyCode::Char(c) => state.provider_custom_api_key.insert(c),
            KeyCode::Backspace => state.provider_custom_api_key.backspace(),
            KeyCode::Delete => state.provider_custom_api_key.delete(),
            KeyCode::Left => state.provider_custom_api_key.move_left(),
            KeyCode::Right => state.provider_custom_api_key.move_right(),
            _ => {}
        },
        ProviderSubStep::CustomModel => match key {
            KeyCode::Enter => {
                let base = state.provider_custom_base_url.value.clone();
                state.selected_provider = format!("custom:{base}");
                state.selected_model = state.provider_custom_model.value.clone();
                state.next_step();
            }
            KeyCode::Esc => state.provider_sub_step = ProviderSubStep::CustomApiKey,
            KeyCode::Char(c) => state.provider_custom_model.insert(c),
            KeyCode::Backspace => state.provider_custom_model.backspace(),
            KeyCode::Delete => state.provider_custom_model.delete(),
            KeyCode::Left => state.provider_custom_model.move_left(),
            KeyCode::Right => state.provider_custom_model.move_right(),
            _ => {}
        },
    }
}

fn handle_channels_key(state: &mut WizardState, key: KeyCode) {
    match state.channel_sub_step {
        ChannelSubStep::Picker => match key {
            KeyCode::Up => state.channel_picker.up(),
            KeyCode::Down => state.channel_picker.down(),
            KeyCode::Enter => {
                let selected = state.channel_picker.selected;
                let sub = match selected {
                    0 => ChannelSubStep::TelegramToken,
                    1 => ChannelSubStep::DiscordToken,
                    2 => ChannelSubStep::SlackToken,
                    3 => ChannelSubStep::IMessageContacts,
                    4 => ChannelSubStep::MatrixHomeserver,
                    5 => ChannelSubStep::WhatsAppToken,
                    6 => ChannelSubStep::IrcServer,
                    7 => ChannelSubStep::WebhookPort,
                    _ => {
                        // Done
                        state.next_step();
                        return;
                    }
                };
                state.channel_sub_step = sub;
                state.channel_text_input.clear();
                state.channel_connection_result = None;
            }
            KeyCode::Esc => state.prev_step(),
            _ => {}
        },
        _ => {
            // Text input for channel configuration
            match key {
                KeyCode::Enter => {
                    // Advance to next sub-step or back to picker
                    advance_channel_sub_step(state);
                }
                KeyCode::Esc => {
                    state.channel_sub_step = ChannelSubStep::Picker;
                    state.channel_connection_result = None;
                }
                KeyCode::Char(c) => state.channel_text_input.insert(c),
                KeyCode::Backspace => state.channel_text_input.backspace(),
                KeyCode::Delete => state.channel_text_input.delete(),
                KeyCode::Left => state.channel_text_input.move_left(),
                KeyCode::Right => state.channel_text_input.move_right(),
                _ => {}
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn advance_channel_sub_step(state: &mut WizardState) {
    use crate::config::schema::{IrcConfig, WhatsAppConfig};
    use crate::config::{
        DiscordConfig, IMessageConfig, MatrixConfig, SlackConfig, TelegramConfig, WebhookConfig,
    };

    let val = state.channel_text_input.value.trim().to_string();

    match state.channel_sub_step {
        ChannelSubStep::TelegramToken => {
            if val.is_empty() {
                state.channel_sub_step = ChannelSubStep::Picker;
                return;
            }
            // Store token temporarily, move to allowlist
            state.channel_sub_step = ChannelSubStep::TelegramAllowlist;
            state.channel_text_input.clear();
        }
        ChannelSubStep::TelegramAllowlist => {
            let allowed = parse_allowlist(&val);
            // We need to get the token from the previous step
            // Store in channels_config
            state.channels_config.telegram = Some(TelegramConfig {
                bot_token: String::new(), // will be filled from connection test
                allowed_users: allowed,
            });
            state.channel_sub_step = ChannelSubStep::Picker;
        }
        ChannelSubStep::DiscordToken => {
            if val.is_empty() {
                state.channel_sub_step = ChannelSubStep::Picker;
                return;
            }
            state.channel_sub_step = ChannelSubStep::DiscordGuild;
            state.channel_text_input.clear();
        }
        ChannelSubStep::DiscordGuild => {
            state.channel_sub_step = ChannelSubStep::DiscordAllowlist;
            state.channel_text_input.clear();
        }
        ChannelSubStep::DiscordAllowlist => {
            let allowed = parse_allowlist(&val);
            state.channels_config.discord = Some(DiscordConfig {
                bot_token: String::new(),
                guild_id: None,
                allowed_users: allowed,
            });
            state.channel_sub_step = ChannelSubStep::Picker;
        }
        ChannelSubStep::SlackToken => {
            if val.is_empty() {
                state.channel_sub_step = ChannelSubStep::Picker;
                return;
            }
            state.channel_sub_step = ChannelSubStep::SlackAppToken;
            state.channel_text_input.clear();
        }
        ChannelSubStep::SlackAppToken => {
            state.channel_sub_step = ChannelSubStep::SlackChannel;
            state.channel_text_input.clear();
        }
        ChannelSubStep::SlackChannel => {
            state.channel_sub_step = ChannelSubStep::SlackAllowlist;
            state.channel_text_input.clear();
        }
        ChannelSubStep::SlackAllowlist => {
            let allowed = parse_allowlist(&val);
            state.channels_config.slack = Some(SlackConfig {
                bot_token: String::new(),
                app_token: None,
                channel_id: None,
                allowed_users: allowed,
            });
            state.channel_sub_step = ChannelSubStep::Picker;
        }
        ChannelSubStep::IMessageContacts => {
            let contacts = parse_allowlist(&val);
            state.channels_config.imessage = Some(IMessageConfig {
                allowed_contacts: if contacts.is_empty() {
                    vec!["*".into()]
                } else {
                    contacts
                },
            });
            state.channel_sub_step = ChannelSubStep::Picker;
        }
        ChannelSubStep::MatrixHomeserver => {
            if val.is_empty() {
                state.channel_sub_step = ChannelSubStep::Picker;
                return;
            }
            state.channel_sub_step = ChannelSubStep::MatrixToken;
            state.channel_text_input.clear();
        }
        ChannelSubStep::MatrixToken => {
            state.channel_sub_step = ChannelSubStep::MatrixRoom;
            state.channel_text_input.clear();
        }
        ChannelSubStep::MatrixRoom => {
            state.channel_sub_step = ChannelSubStep::MatrixAllowlist;
            state.channel_text_input.clear();
        }
        ChannelSubStep::MatrixAllowlist => {
            let allowed = parse_allowlist(&val);
            state.channels_config.matrix = Some(MatrixConfig {
                homeserver: String::new(),
                access_token: String::new(),
                room_id: String::new(),
                allowed_users: if allowed.is_empty() {
                    vec!["*".into()]
                } else {
                    allowed
                },
            });
            state.channel_sub_step = ChannelSubStep::Picker;
        }
        ChannelSubStep::WhatsAppToken => {
            if val.is_empty() {
                state.channel_sub_step = ChannelSubStep::Picker;
                return;
            }
            state.channel_sub_step = ChannelSubStep::WhatsAppPhone;
            state.channel_text_input.clear();
        }
        ChannelSubStep::WhatsAppPhone => {
            state.channel_sub_step = ChannelSubStep::WhatsAppVerify;
            state.channel_text_input.clear();
        }
        ChannelSubStep::WhatsAppVerify => {
            state.channel_sub_step = ChannelSubStep::WhatsAppAllowlist;
            state.channel_text_input.clear();
        }
        ChannelSubStep::WhatsAppAllowlist => {
            let allowed = parse_allowlist(&val);
            state.channels_config.whatsapp = Some(WhatsAppConfig {
                access_token: String::new(),
                phone_number_id: String::new(),
                verify_token: "asteroniris-whatsapp-verify".into(),
                allowed_numbers: if allowed.is_empty() {
                    vec!["*".into()]
                } else {
                    allowed
                },
                app_secret: None,
            });
            state.channel_sub_step = ChannelSubStep::Picker;
        }
        ChannelSubStep::IrcServer => {
            if val.is_empty() {
                state.channel_sub_step = ChannelSubStep::Picker;
                return;
            }
            state.channel_sub_step = ChannelSubStep::IrcPort;
            state.channel_text_input =
                crate::onboard::tui::widgets::text_input::TextInput::new("6697");
        }
        ChannelSubStep::IrcPort => {
            state.channel_sub_step = ChannelSubStep::IrcNick;
            state.channel_text_input.clear();
        }
        ChannelSubStep::IrcNick => {
            if val.is_empty() {
                state.channel_sub_step = ChannelSubStep::Picker;
                return;
            }
            state.channel_sub_step = ChannelSubStep::IrcChannels;
            state.channel_text_input.clear();
        }
        ChannelSubStep::IrcChannels => {
            state.channel_sub_step = ChannelSubStep::IrcAllowlist;
            state.channel_text_input.clear();
        }
        ChannelSubStep::IrcAllowlist => {
            let allowed = parse_allowlist(&val);
            state.channels_config.irc = Some(IrcConfig {
                server: String::new(),
                port: 6697,
                nickname: String::new(),
                username: None,
                channels: vec![],
                allowed_users: if allowed.is_empty() {
                    vec!["*".into()]
                } else {
                    allowed
                },
                server_password: None,
                nickserv_password: None,
                sasl_password: None,
                verify_tls: Some(true),
            });
            state.channel_sub_step = ChannelSubStep::Picker;
        }
        ChannelSubStep::WebhookPort => {
            state.channel_sub_step = ChannelSubStep::WebhookSecret;
            state.channel_text_input.clear();
        }
        ChannelSubStep::WebhookSecret => {
            let port: u16 = val.parse().unwrap_or(8080);
            state.channels_config.webhook = Some(WebhookConfig { port, secret: None });
            state.channel_sub_step = ChannelSubStep::Picker;
        }
        ChannelSubStep::Picker => {} // handled above
    }
}

fn handle_tunnel_key(state: &mut WizardState, key: KeyCode) {
    match key {
        KeyCode::Up => state.tunnel_select.up(),
        KeyCode::Down => state.tunnel_select.down(),
        KeyCode::Enter => state.next_step(),
        KeyCode::Esc => state.prev_step(),
        KeyCode::Char(c) => {
            // Route to the active text input based on selection
            match state.tunnel_select.selected {
                1 | 3 => state.tunnel_token.insert(c),
                4 => state.tunnel_command.insert(c),
                _ => {}
            }
        }
        KeyCode::Backspace => match state.tunnel_select.selected {
            1 | 3 => state.tunnel_token.backspace(),
            4 => state.tunnel_command.backspace(),
            _ => {}
        },
        _ => {}
    }
}

fn handle_tool_mode_key(state: &mut WizardState, key: KeyCode) {
    match key {
        KeyCode::Up => state.tool_mode_select.up(),
        KeyCode::Down => state.tool_mode_select.down(),
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
            state.encrypt_toggle.toggle();
        }
        KeyCode::Enter => state.next_step(),
        KeyCode::Esc => state.prev_step(),
        KeyCode::Char(c) => {
            if state.tool_mode_select.selected == 1 {
                state.composio_api_key.insert(c);
            }
        }
        KeyCode::Backspace => {
            if state.tool_mode_select.selected == 1 {
                state.composio_api_key.backspace();
            }
        }
        _ => {}
    }
}

fn handle_memory_key(state: &mut WizardState, key: KeyCode) {
    match key {
        KeyCode::Up => state.memory_select.up(),
        KeyCode::Down => state.memory_select.down(),
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
            if state.memory_select.selected != 2 {
                state.memory_auto_save.toggle();
            }
        }
        KeyCode::Enter => state.next_step(),
        KeyCode::Esc => state.prev_step(),
        _ => {}
    }
}

fn handle_context_key(state: &mut WizardState, key: KeyCode) {
    let sub = state.context_sub_field;

    match sub {
        0 => {
            // Name input
            match key {
                KeyCode::Enter => state.context_sub_field = 1,
                KeyCode::Esc => state.prev_step(),
                KeyCode::Char(c) => state.context_name.insert(c),
                KeyCode::Backspace => state.context_name.backspace(),
                KeyCode::Delete => state.context_name.delete(),
                KeyCode::Left => state.context_name.move_left(),
                KeyCode::Right => state.context_name.move_right(),
                _ => {}
            }
        }
        1 => {
            // Timezone select
            match key {
                KeyCode::Up => state.context_tz_select.up(),
                KeyCode::Down => state.context_tz_select.down(),
                KeyCode::Enter => {
                    // Last item = "Other"
                    if state.context_tz_select.selected == state.context_tz_select.items.len() - 1 {
                        // Custom timezone input — keep sub_field = 1, handled by custom text
                        state.context_sub_field = 2; // go to agent name for now (simplified)
                    } else {
                        state.context_sub_field = 2;
                    }
                }
                KeyCode::Esc => state.context_sub_field = 0,
                _ => {}
            }
        }
        2 => {
            // Agent name input
            match key {
                KeyCode::Enter => state.context_sub_field = 3,
                KeyCode::Esc => state.context_sub_field = 1,
                KeyCode::Char(c) => state.context_agent_name.insert(c),
                KeyCode::Backspace => state.context_agent_name.backspace(),
                KeyCode::Delete => state.context_agent_name.delete(),
                KeyCode::Left => state.context_agent_name.move_left(),
                KeyCode::Right => state.context_agent_name.move_right(),
                _ => {}
            }
        }
        3 => {
            // Style select
            match key {
                KeyCode::Up => state.context_style_select.up(),
                KeyCode::Down => state.context_style_select.down(),
                KeyCode::Enter => {
                    // Last item = "Custom"
                    if state.context_style_select.selected
                        == state.context_style_select.items.len() - 1
                    {
                        state.context_sub_field = 4;
                    } else {
                        state.next_step();
                    }
                }
                KeyCode::Esc => state.context_sub_field = 2,
                _ => {}
            }
        }
        4 => {
            // Custom style text input
            match key {
                KeyCode::Enter => state.next_step(),
                KeyCode::Esc => state.context_sub_field = 3,
                KeyCode::Char(c) => state.context_style_custom.insert(c),
                KeyCode::Backspace => state.context_style_custom.backspace(),
                KeyCode::Delete => state.context_style_custom.delete(),
                KeyCode::Left => state.context_style_custom.move_left(),
                KeyCode::Right => state.context_style_custom.move_right(),
                _ => {}
            }
        }
        _ => {}
    }
}

fn handle_summary_key(state: &mut WizardState, key: KeyCode) {
    match key {
        KeyCode::Enter => {
            state.summary_confirmed = true;
        }
        KeyCode::Esc => state.prev_step(),
        _ => {}
    }
}

fn parse_allowlist(input: &str) -> Vec<String> {
    if input.trim() == "*" {
        return vec!["*".into()];
    }
    input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
