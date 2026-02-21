use crate::config::{ChannelsConfig, ComposioConfig, MemoryConfig, SecretsConfig, TunnelConfig};

use super::widgets::{SelectList, TextInput, Toggle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    Workspace,
    Provider,
    Channels,
    Tunnel,
    ToolMode,
    Memory,
    Context,
    Summary,
}

impl WizardStep {
    pub const ALL: [Self; 8] = [
        Self::Workspace,
        Self::Provider,
        Self::Channels,
        Self::Tunnel,
        Self::ToolMode,
        Self::Memory,
        Self::Context,
        Self::Summary,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&s| s == self).unwrap_or(0)
    }

    pub fn label(self) -> String {
        match self {
            Self::Workspace => t!("onboard.step.workspace"),
            Self::Provider => t!("onboard.step.provider"),
            Self::Channels => t!("onboard.step.channels"),
            Self::Tunnel => t!("onboard.step.tunnel"),
            Self::ToolMode => t!("onboard.step.tool_mode"),
            Self::Memory => t!("onboard.step.memory"),
            Self::Context => t!("onboard.step.context"),
            Self::Summary => t!("onboard.step.scaffold"),
        }
        .to_string()
    }

    pub fn short(self) -> &'static str {
        match self {
            Self::Workspace => "WS",
            Self::Provider => "AI",
            Self::Channels => "Ch",
            Self::Tunnel => "Tu",
            Self::ToolMode => "TM",
            Self::Memory => "Me",
            Self::Context => "Cx",
            Self::Summary => "Sc",
        }
    }
}

/// Sub-step within channels setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelSubStep {
    Picker,
    TelegramToken,
    TelegramAllowlist,
    DiscordToken,
    DiscordGuild,
    DiscordAllowlist,
    SlackToken,
    SlackAppToken,
    SlackChannel,
    SlackAllowlist,
    MatrixHomeserver,
    MatrixToken,
    MatrixRoom,
    MatrixAllowlist,
    WhatsAppToken,
    WhatsAppPhone,
    WhatsAppVerify,
    WhatsAppAllowlist,
    IrcServer,
    IrcPort,
    IrcNick,
    IrcChannels,
    IrcAllowlist,
    WebhookPort,
    WebhookSecret,
    IMessageContacts,
}

/// Sub-step within provider setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSubStep {
    TierSelect,
    ProviderSelect,
    ApiKey,
    ModelSelect,
    CustomBaseUrl,
    CustomApiKey,
    CustomModel,
}

/// The full wizard state machine.
pub struct WizardState {
    pub current_step: WizardStep,
    pub completed_steps: Vec<WizardStep>,
    pub should_quit: bool,
    pub status_message: Option<String>,

    // ── Step 1: Workspace ──
    pub workspace_use_default: Toggle,
    pub workspace_custom_path: TextInput,
    pub workspace_dir: String,
    pub config_path: String,

    // ── Step 2: Provider ──
    pub provider_sub_step: ProviderSubStep,
    pub provider_tier_select: SelectList,
    pub provider_select: SelectList,
    pub provider_api_key: TextInput,
    pub provider_model_select: SelectList,
    pub provider_custom_base_url: TextInput,
    pub provider_custom_api_key: TextInput,
    pub provider_custom_model: TextInput,
    pub selected_provider: String,
    pub selected_api_key: String,
    pub selected_model: String,

    // ── Step 3: Channels ──
    pub channel_sub_step: ChannelSubStep,
    pub channel_picker: SelectList,
    pub channels_config: ChannelsConfig,
    // Temporary inputs for in-progress channel setup
    pub channel_text_input: TextInput,
    pub channel_connection_testing: bool,
    pub channel_connection_result: Option<Result<String, String>>,

    // ── Step 4: Tunnel ──
    pub tunnel_select: SelectList,
    pub tunnel_token: TextInput,
    pub tunnel_funnel: Toggle,
    pub tunnel_domain: TextInput,
    pub tunnel_command: TextInput,
    pub tunnel_config: TunnelConfig,

    // ── Step 5: Tool Mode ──
    pub tool_mode_select: SelectList,
    pub composio_api_key: TextInput,
    pub encrypt_toggle: Toggle,
    pub composio_config: ComposioConfig,
    pub secrets_config: SecretsConfig,

    // ── Step 6: Memory ──
    pub memory_select: SelectList,
    pub memory_auto_save: Toggle,
    pub memory_config: MemoryConfig,

    // ── Step 7: Context ──
    pub context_name: TextInput,
    pub context_tz_select: SelectList,
    pub context_tz_custom: TextInput,
    pub context_agent_name: TextInput,
    pub context_style_select: SelectList,
    pub context_style_custom: TextInput,
    pub context_sub_field: u8, // 0=name, 1=tz, 2=agent, 3=style, 4=done

    // ── Step 8: Summary ──
    pub summary_confirmed: bool,
}

impl WizardState {
    #[allow(clippy::too_many_lines)]
    pub fn new() -> Self {
        let home = directories::UserDirs::new()
            .map(|u| u.home_dir().to_path_buf())
            .unwrap_or_default();
        let default_dir = home.join(".asteroniris");

        Self {
            current_step: WizardStep::Workspace,
            completed_steps: Vec::new(),
            should_quit: false,
            status_message: None,

            workspace_use_default: Toggle::new(true),
            workspace_custom_path: TextInput::new(""),
            workspace_dir: default_dir.join("workspace").display().to_string(),
            config_path: default_dir.join("config.toml").display().to_string(),

            provider_sub_step: ProviderSubStep::TierSelect,
            provider_tier_select: SelectList::new(vec![
                t!("onboard.provider.tier_recommended").to_string(),
                t!("onboard.provider.tier_fast").to_string(),
                t!("onboard.provider.tier_gateway").to_string(),
                t!("onboard.provider.tier_specialized").to_string(),
                t!("onboard.provider.tier_local").to_string(),
                t!("onboard.provider.tier_custom").to_string(),
            ]),
            provider_select: SelectList::new(vec![]),
            provider_api_key: TextInput::new(""),
            provider_model_select: SelectList::new(vec![]),
            provider_custom_base_url: TextInput::new(""),
            provider_custom_api_key: TextInput::new(""),
            provider_custom_model: TextInput::new("default"),
            selected_provider: String::new(),
            selected_api_key: String::new(),
            selected_model: String::new(),

            channel_sub_step: ChannelSubStep::Picker,
            channel_picker: SelectList::new(vec![
                "Telegram".into(),
                "Discord".into(),
                "Slack".into(),
                "iMessage".into(),
                "Matrix".into(),
                "WhatsApp".into(),
                "IRC".into(),
                "Webhook".into(),
                t!("onboard.channels.done").to_string(),
            ]),
            channels_config: ChannelsConfig::default(),
            channel_text_input: TextInput::new(""),
            channel_connection_testing: false,
            channel_connection_result: None,

            tunnel_select: SelectList::new(vec![
                t!("onboard.tunnel.skip").to_string(),
                t!("onboard.tunnel.cloudflare").to_string(),
                t!("onboard.tunnel.tailscale").to_string(),
                t!("onboard.tunnel.ngrok").to_string(),
                t!("onboard.tunnel.custom").to_string(),
            ]),
            tunnel_token: TextInput::new(""),
            tunnel_funnel: Toggle::new(false),
            tunnel_domain: TextInput::new(""),
            tunnel_command: TextInput::new(""),
            tunnel_config: TunnelConfig::default(),

            tool_mode_select: SelectList::new(vec![
                t!("onboard.tool_mode.sovereign").to_string(),
                t!("onboard.tool_mode.composio").to_string(),
            ]),
            composio_api_key: TextInput::new(""),
            encrypt_toggle: Toggle::new(true),
            composio_config: ComposioConfig::default(),
            secrets_config: SecretsConfig::default(),

            memory_select: SelectList::new(vec![
                t!("onboard.memory.sqlite").to_string(),
                t!("onboard.memory.markdown").to_string(),
                t!("onboard.memory.none").to_string(),
            ]),
            memory_auto_save: Toggle::new(true),
            memory_config: MemoryConfig::default(),

            context_name: TextInput::new(&std::env::var("USER").unwrap_or_else(|_| "User".into())),
            context_tz_select: SelectList::new(vec![
                "US/Eastern (EST/EDT)".into(),
                "US/Central (CST/CDT)".into(),
                "US/Mountain (MST/MDT)".into(),
                "US/Pacific (PST/PDT)".into(),
                "Europe/London (GMT/BST)".into(),
                "Europe/Berlin (CET/CEST)".into(),
                "Asia/Tokyo (JST)".into(),
                "UTC".into(),
                t!("onboard.context.tz_other").to_string(),
            ]),
            context_tz_custom: TextInput::new("UTC"),
            context_agent_name: TextInput::new("AsteronIris"),
            context_style_select: SelectList::new(vec![
                t!("onboard.context.style_direct").to_string(),
                t!("onboard.context.style_friendly").to_string(),
                t!("onboard.context.style_professional").to_string(),
                t!("onboard.context.style_expressive").to_string(),
                t!("onboard.context.style_technical").to_string(),
                t!("onboard.context.style_balanced").to_string(),
                t!("onboard.context.style_custom").to_string(),
            ]),
            context_style_custom: TextInput::new(""),
            context_sub_field: 0,

            summary_confirmed: false,
        }
    }

    pub fn next_step(&mut self) {
        let idx = self.current_step.index();
        if !self.completed_steps.contains(&self.current_step) {
            self.completed_steps.push(self.current_step);
        }
        if idx + 1 < WizardStep::ALL.len() {
            self.current_step = WizardStep::ALL[idx + 1];
        }
    }

    pub fn prev_step(&mut self) {
        let idx = self.current_step.index();
        if idx > 0 {
            self.current_step = WizardStep::ALL[idx - 1];
        }
    }

    pub fn is_step_done(&self, step: WizardStep) -> bool {
        self.completed_steps.contains(&step)
    }
}
