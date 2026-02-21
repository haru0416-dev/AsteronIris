use super::Config;
use anyhow::{Context, Result};
use directories::UserDirs;
use std::fs;

impl Config {
    pub fn load_or_init() -> Result<Self> {
        let home = UserDirs::new()
            .map(|u| u.home_dir().to_path_buf())
            .context("Could not find home directory")?;
        let asteroniris_dir = home.join(".asteroniris");
        let config_path = asteroniris_dir.join("config.toml");

        if !asteroniris_dir.exists() {
            fs::create_dir_all(&asteroniris_dir)
                .context("Failed to create .asteroniris directory")?;
            fs::create_dir_all(asteroniris_dir.join("workspace"))
                .context("Failed to create workspace directory")?;
        }

        if config_path.exists() {
            let contents =
                fs::read_to_string(&config_path).context("Failed to read config file")?;
            let mut config: Config =
                toml::from_str(&contents).context("Failed to parse config file")?;
            config.config_path.clone_from(&config_path);
            config.workspace_dir = asteroniris_dir.join("workspace");

            let secrets_need_persist = config.decrypt_config_secrets_in_place()?;
            if secrets_need_persist {
                config.save()?;
            }

            config.validate_autonomy_controls()?;
            Ok(config)
        } else {
            let config = Self {
                config_path: config_path.clone(),
                workspace_dir: asteroniris_dir.join("workspace"),
                ..Self::default()
            };
            config.validate_autonomy_controls()?;
            config.save()?;
            Ok(config)
        }
    }

    pub fn save(&self) -> Result<()> {
        let persisted = self.config_for_persistence()?;
        let toml_str = toml::to_string_pretty(&persisted).context("Failed to serialize config")?;
        fs::write(&self.config_path, toml_str).context("Failed to write config file")?;
        Ok(())
    }
}
