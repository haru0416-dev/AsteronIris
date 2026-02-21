use super::Config;
use std::path::PathBuf;

impl Config {
    pub fn apply_env_overrides(&mut self) {
        if let Ok(key) = std::env::var("ASTERONIRIS_API_KEY").or_else(|_| std::env::var("API_KEY"))
            && !key.is_empty()
        {
            self.api_key = Some(key);
        }

        if let Ok(provider) =
            std::env::var("ASTERONIRIS_PROVIDER").or_else(|_| std::env::var("PROVIDER"))
            && !provider.is_empty()
        {
            self.default_provider = Some(provider);
        }

        if let Ok(model) = std::env::var("ASTERONIRIS_MODEL")
            && !model.is_empty()
        {
            self.default_model = Some(model);
        }

        if let Ok(workspace) = std::env::var("ASTERONIRIS_WORKSPACE")
            && !workspace.is_empty()
        {
            self.workspace_dir = PathBuf::from(workspace);
        }

        if let Ok(port_str) =
            std::env::var("ASTERONIRIS_GATEWAY_PORT").or_else(|_| std::env::var("PORT"))
            && let Ok(port) = port_str.parse::<u16>()
        {
            self.gateway.port = port;
        }

        if let Ok(host) =
            std::env::var("ASTERONIRIS_GATEWAY_HOST").or_else(|_| std::env::var("HOST"))
            && !host.is_empty()
        {
            self.gateway.host = host;
        }

        if let Ok(temp_str) = std::env::var("ASTERONIRIS_TEMPERATURE")
            && let Ok(temp) = temp_str.parse::<f64>()
            && (0.0..=2.0).contains(&temp)
        {
            self.default_temperature = temp;
        }
    }
}
