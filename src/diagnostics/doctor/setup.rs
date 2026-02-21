use crate::config::Config;
use directories::UserDirs;

pub(crate) fn run_setup_checks(config: &Config) -> Vec<(bool, String)> {
    let mut checks: Vec<(bool, String)> = Vec::new();

    let config_exists = config.config_path.exists();
    checks.push((
        config_exists,
        format!(
            "Config file: {}",
            if config_exists {
                config.config_path.display().to_string()
            } else {
                format!("missing ({})", config.config_path.display())
            }
        ),
    ));

    let ws_exists = config.workspace_dir.exists();
    checks.push((
        ws_exists,
        format!(
            "Workspace: {}",
            if ws_exists {
                config.workspace_dir.display().to_string()
            } else {
                format!("missing ({})", config.workspace_dir.display())
            }
        ),
    ));

    let has_provider = config.default_provider.is_some();
    checks.push((
        has_provider,
        format!(
            "Provider: {}",
            config
                .default_provider
                .as_deref()
                .unwrap_or("not configured — run: asteroniris onboard")
        ),
    ));

    let has_api_key = config.api_key.is_some()
        || std::env::var("ASTERONIRIS_API_KEY").is_ok()
        || std::env::var("API_KEY").is_ok();
    checks.push((
        has_api_key,
        if has_api_key {
            "API key: configured".to_string()
        } else {
            "API key: not set — run: asteroniris onboard".to_string()
        },
    ));

    let memory_ok = config.memory.backend != "none";
    checks.push((
        memory_ok,
        format!(
            "Memory: {} (auto-save: {})",
            config.memory.backend,
            if config.memory.auto_save { "on" } else { "off" }
        ),
    ));

    let service_installed = check_service_installed();
    checks.push((
        service_installed,
        if service_installed {
            "OS service: installed".to_string()
        } else {
            "OS service: not installed — optional, run: asteroniris service install".to_string()
        },
    ));

    checks
}

fn check_service_installed() -> bool {
    if cfg!(target_os = "macos") {
        UserDirs::new().is_some_and(|u| {
            u.home_dir()
                .join("Library")
                .join("LaunchAgents")
                .join("com.asteroniris.daemon.plist")
                .exists()
        })
    } else if cfg!(target_os = "linux") {
        UserDirs::new().is_some_and(|u| {
            u.home_dir()
                .join(".config")
                .join("systemd")
                .join("user")
                .join("asteroniris.service")
                .exists()
        })
    } else {
        false
    }
}
