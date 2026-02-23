use crate::config::Config;
use crate::core::memory::create_memory;
use crate::core::persona::state_persistence::BackendCanonicalStateHeaderPersistence;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task::JoinHandle;

mod heartbeat_worker;
mod state;
mod supervisor;

use state::spawn_state_writer;
use supervisor::spawn_supervised_components;

const STATUS_FLUSH_SECONDS: u64 = 5;

pub async fn run(config: Arc<Config>, host: String, port: u16) -> Result<()> {
    let initial_backoff = config.reliability.channel_initial_backoff_secs.max(1);
    let max_backoff = config
        .reliability
        .channel_max_backoff_secs
        .max(initial_backoff);

    crate::runtime::diagnostics::health::mark_component_ok("daemon");

    if config.heartbeat.enabled
        && let Err(error) =
            crate::runtime::diagnostics::heartbeat::engine::HeartbeatEngine::ensure_heartbeat_file(
                &config.workspace_dir,
            )
            .await
    {
        tracing::warn!(%error, "failed to ensure heartbeat file");
    }

    if let Err(error) = initialize_persona_startup_state(&config).await {
        tracing::warn!(%error, "failed to initialize persona startup state");
    }

    let mut handles: Vec<JoinHandle<()>> = vec![spawn_state_writer(Arc::clone(&config))];
    handles.extend(spawn_supervised_components(
        Arc::clone(&config),
        host.clone(),
        port,
        initial_backoff,
        max_backoff,
        has_supervised_channels(&config),
    ));

    println!("◆ {}", t!("daemon.started"));
    println!("   {}", t!("daemon.gateway_addr", host = host, port = port));
    println!("   {}", t!("daemon.components"));
    println!("   {}", t!("daemon.stop_hint"));

    tokio::signal::ctrl_c().await?;
    crate::runtime::diagnostics::health::mark_component_error("daemon", "shutdown requested");

    for handle in &handles {
        handle.abort();
    }
    for handle in handles {
        if let Err(error) = handle.await {
            tracing::warn!(%error, "daemon task panicked during shutdown");
        }
    }

    Ok(())
}

async fn initialize_persona_startup_state(config: &Config) -> Result<()> {
    if !config.persona.enabled_main_session {
        return Ok(());
    }

    let memory = create_memory(&config.memory, &config.workspace_dir, None)?;
    let person_id = config
        .identity
        .person_id
        .clone()
        .unwrap_or_else(|| "local-default".to_string());
    let persistence = BackendCanonicalStateHeaderPersistence::new(
        Arc::from(memory),
        config.workspace_dir.clone(),
        config.persona.clone(),
        person_id,
    );
    let _ = persistence
        .reconcile_mirror_from_backend_on_startup()
        .await?;
    Ok(())
}

pub fn state_file_path(config: &Config) -> PathBuf {
    state::state_file_path(config)
}

fn has_supervised_channels(config: &Config) -> bool {
    config.channels_config.telegram.is_some()
        || config.channels_config.discord.is_some()
        || config.channels_config.slack.is_some()
        || config.channels_config.imessage.is_some()
        || config.channels_config.matrix.is_some()
        || config.channels_config.whatsapp.is_some()
        || config.channels_config.email.is_some()
}

#[cfg(test)]
mod tests {
    use super::initialize_persona_startup_state;
    use crate::config::Config;
    use crate::core::memory::create_memory;
    use crate::core::persona::state_header::StateHeader;
    use crate::core::persona::state_persistence::BackendCanonicalStateHeaderPersistence;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn custom_state() -> StateHeader {
        StateHeader {
            identity_principles_hash: "identity-v1-abcd1234".to_string(),
            safety_posture: "strict".to_string(),
            current_objective: "reconcile from backend canonical".to_string(),
            open_loops: vec!["startup reconcile".to_string()],
            next_actions: vec!["repair mirror".to_string()],
            commitments: vec!["preserve canonical source".to_string()],
            recent_context_summary: "daemon startup test".to_string(),
            last_updated_at: "2026-02-23T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn initialize_persona_startup_state_seeds_when_enabled() {
        let tmp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();
        config.persona.enabled_main_session = true;

        initialize_persona_startup_state(&config)
            .await
            .expect("startup reconcile should succeed");

        let mirror_path = config
            .workspace_dir
            .join(&config.persona.state_mirror_filename);
        assert!(mirror_path.exists());

        let memory = create_memory(&config.memory, &config.workspace_dir, None).unwrap();
        let slot = memory
            .resolve_slot(
                "person:local-default",
                "persona/local-default/state_header/v1",
            )
            .await
            .unwrap();
        assert!(slot.is_some());
    }

    #[tokio::test]
    async fn initialize_persona_startup_state_noop_when_disabled() {
        let tmp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();
        config.persona.enabled_main_session = false;

        initialize_persona_startup_state(&config)
            .await
            .expect("disabled path should no-op");

        let mirror_path = config
            .workspace_dir
            .join(&config.persona.state_mirror_filename);
        assert!(!mirror_path.exists());
    }

    #[tokio::test]
    async fn initialize_persona_startup_state_disabled_preserves_existing_mirror() {
        let tmp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();
        config.persona.enabled_main_session = false;

        let mirror_path = config
            .workspace_dir
            .join(&config.persona.state_mirror_filename);
        std::fs::write(&mirror_path, "{\"state_header\":\"existing\"}").unwrap();

        initialize_persona_startup_state(&config)
            .await
            .expect("disabled path should preserve existing mirror");

        let mirror_raw = std::fs::read_to_string(&mirror_path).unwrap();
        assert_eq!(mirror_raw, "{\"state_header\":\"existing\"}");
    }

    #[tokio::test]
    async fn initialize_persona_startup_state_repairs_corrupt_mirror_from_backend() {
        let tmp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();
        config.persona.enabled_main_session = true;

        let memory = create_memory(&config.memory, &config.workspace_dir, None).unwrap();
        let persistence = BackendCanonicalStateHeaderPersistence::new(
            Arc::from(memory),
            config.workspace_dir.clone(),
            config.persona.clone(),
            "local-default",
        );
        persistence
            .persist_backend_canonical_and_sync_mirror(&custom_state())
            .await
            .expect("seed custom canonical state");

        let mirror_path = config
            .workspace_dir
            .join(&config.persona.state_mirror_filename);
        std::fs::write(&mirror_path, "{\"state_header\":\"corrupt\"}").unwrap();

        initialize_persona_startup_state(&config)
            .await
            .expect("startup reconcile should repair mirror");

        let mirror_raw = std::fs::read_to_string(&mirror_path).unwrap();
        assert!(mirror_raw.contains("reconcile from backend canonical"));
        assert!(!mirror_raw.contains("\"state_header\":\"corrupt\""));
    }

    #[tokio::test]
    async fn initialize_persona_startup_state_recreates_missing_mirror_from_backend() {
        let tmp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();
        config.persona.enabled_main_session = true;

        let memory = create_memory(&config.memory, &config.workspace_dir, None).unwrap();
        let persistence = BackendCanonicalStateHeaderPersistence::new(
            Arc::from(memory),
            config.workspace_dir.clone(),
            config.persona.clone(),
            "local-default",
        );
        persistence
            .persist_backend_canonical_and_sync_mirror(&custom_state())
            .await
            .expect("seed custom canonical state");

        let mirror_path = config
            .workspace_dir
            .join(&config.persona.state_mirror_filename);
        std::fs::remove_file(&mirror_path).unwrap();
        assert!(!mirror_path.exists());

        initialize_persona_startup_state(&config)
            .await
            .expect("startup reconcile should recreate missing mirror");

        let mirror_raw = std::fs::read_to_string(&mirror_path).unwrap();
        assert!(mirror_raw.contains("reconcile from backend canonical"));
    }
}
