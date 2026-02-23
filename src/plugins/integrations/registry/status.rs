use super::super::IntegrationStatus;
use crate::config::Config;

fn active_when(condition: bool) -> IntegrationStatus {
    if condition {
        IntegrationStatus::Active
    } else {
        IntegrationStatus::Available
    }
}

fn provider_status(config: &Config, provider: &str) -> IntegrationStatus {
    active_when(config.default_provider.as_deref() == Some(provider))
}

fn model_prefix_status(config: &Config, prefix: &str) -> IntegrationStatus {
    active_when(
        config
            .default_model
            .as_deref()
            .is_some_and(|model| model.starts_with(prefix)),
    )
}

macro_rules! channel_status {
    ($name:ident, $field:ident) => {
        pub(super) fn $name(config: &Config) -> IntegrationStatus {
            active_when(config.channels_config.$field.is_some())
        }
    };
}

macro_rules! provider_status_fn {
    ($name:ident, $provider:literal) => {
        pub(super) fn $name(config: &Config) -> IntegrationStatus {
            provider_status(config, $provider)
        }
    };
}

macro_rules! model_prefix_status_fn {
    ($name:ident, $prefix:literal) => {
        pub(super) fn $name(config: &Config) -> IntegrationStatus {
            model_prefix_status(config, $prefix)
        }
    };
}

channel_status!(telegram, telegram);
channel_status!(discord, discord);
channel_status!(slack, slack);
channel_status!(webhooks, webhook);
channel_status!(imessage, imessage);
channel_status!(matrix, matrix);

pub(super) fn openrouter(config: &Config) -> IntegrationStatus {
    active_when(
        config.default_provider.as_deref() == Some("openrouter") && config.api_key.is_some(),
    )
}

provider_status_fn!(anthropic, "anthropic");
provider_status_fn!(openai, "openai");
model_prefix_status_fn!(google, "google/");
model_prefix_status_fn!(deepseek, "deepseek/");
model_prefix_status_fn!(xai, "x-ai/");
model_prefix_status_fn!(mistral_model, "mistral");
provider_status_fn!(ollama, "ollama");
provider_status_fn!(perplexity, "perplexity");
provider_status_fn!(venice, "venice");
provider_status_fn!(vercel, "vercel");
provider_status_fn!(cloudflare, "cloudflare");
provider_status_fn!(moonshot, "moonshot");
provider_status_fn!(synthetic, "synthetic");
provider_status_fn!(opencode, "opencode");
provider_status_fn!(zai, "zai");
provider_status_fn!(glm, "glm");
provider_status_fn!(minimax, "minimax");
provider_status_fn!(bedrock, "bedrock");
provider_status_fn!(qianfan, "qianfan");
provider_status_fn!(groq, "groq");
provider_status_fn!(together, "together");
provider_status_fn!(fireworks, "fireworks");
provider_status_fn!(cohere, "cohere");

pub(super) fn macos(_: &Config) -> IntegrationStatus {
    if cfg!(target_os = "macos") {
        IntegrationStatus::Active
    } else {
        IntegrationStatus::Available
    }
}

pub(super) fn linux(_: &Config) -> IntegrationStatus {
    if cfg!(target_os = "linux") {
        IntegrationStatus::Active
    } else {
        IntegrationStatus::Available
    }
}

pub(super) fn coming_soon(_: &Config) -> IntegrationStatus {
    IntegrationStatus::ComingSoon
}

pub(super) fn available(_: &Config) -> IntegrationStatus {
    IntegrationStatus::Available
}

pub(super) fn active(_: &Config) -> IntegrationStatus {
    IntegrationStatus::Active
}
