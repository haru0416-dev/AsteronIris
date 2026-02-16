use crate::config::PersonaConfig;
use crate::memory::{Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel};
use crate::persona::state_header::StateHeaderV1;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const CANONICAL_STATE_HEADER_KEY: &str = "persona/state_header/v1";

const STATE_HEADER_MIRROR_HEADER: &str = "# Persona State Header\n\nbackend_canonical: true\n\n";

pub struct BackendCanonicalStateHeaderPersistence {
    memory: Arc<dyn Memory>,
    workspace_dir: PathBuf,
    persona: PersonaConfig,
}

impl BackendCanonicalStateHeaderPersistence {
    pub fn new(memory: Arc<dyn Memory>, workspace_dir: PathBuf, persona: PersonaConfig) -> Self {
        Self {
            memory,
            workspace_dir,
            persona,
        }
    }

    pub async fn load_backend_canonical(&self) -> Result<Option<StateHeaderV1>> {
        let Some(entry) = self
            .memory
            .resolve_slot("default", CANONICAL_STATE_HEADER_KEY)
            .await?
        else {
            return Ok(None);
        };

        let parsed: StateHeaderV1 = serde_json::from_str(&entry.value).with_context(|| {
            format!(
                "failed to parse backend canonical state header key: {CANONICAL_STATE_HEADER_KEY}"
            )
        })?;
        parsed.validate(&self.persona)?;

        Ok(Some(parsed))
    }

    pub async fn reconcile_mirror_from_backend_on_startup(&self) -> Result<Option<StateHeaderV1>> {
        let Some(canonical) = self.load_backend_canonical().await? else {
            return Ok(None);
        };

        self.sync_mirror_from_backend_canonical(&canonical)?;
        Ok(Some(canonical))
    }

    pub async fn persist_backend_canonical_and_sync_mirror(
        &self,
        state: &StateHeaderV1,
    ) -> Result<()> {
        state.validate(&self.persona)?;

        let serialized = serde_json::to_string(state)?;
        self.memory
            .append_event(
                MemoryEventInput::new(
                    "default",
                    CANONICAL_STATE_HEADER_KEY,
                    MemoryEventType::FactUpdated,
                    serialized,
                    MemorySource::System,
                    PrivacyLevel::Private,
                )
                .with_confidence(0.95)
                .with_importance(1.0),
            )
            .await?;

        self.sync_mirror_from_backend_canonical(state)
    }

    pub fn read_mirror_state(&self) -> Result<Option<StateHeaderV1>> {
        let mirror_path = self.state_mirror_path();
        if !mirror_path.exists() {
            return Ok(None);
        }

        let raw = fs::read_to_string(&mirror_path)
            .with_context(|| format!("failed reading state mirror: {}", mirror_path.display()))?;
        let parsed = parse_state_header_mirror_markdown(&raw)?;
        parsed.validate(&self.persona)?;

        Ok(Some(parsed))
    }

    fn state_mirror_path(&self) -> PathBuf {
        self.workspace_dir.join(&self.persona.state_mirror_filename)
    }

    fn sync_mirror_from_backend_canonical(&self, state: &StateHeaderV1) -> Result<()> {
        let mirror_path = self.state_mirror_path();
        let content = render_state_header_mirror_markdown(state)?;
        write_atomic(&mirror_path, &content)
    }
}

fn render_state_header_mirror_markdown(state: &StateHeaderV1) -> Result<String> {
    let json = serde_json::to_string_pretty(state)?;
    Ok(format!(
        "{STATE_HEADER_MIRROR_HEADER}```json\n{json}\n```\n"
    ))
}

fn parse_state_header_mirror_markdown(raw: &str) -> Result<StateHeaderV1> {
    if let Some(start) = raw.find("```json") {
        let after_start = &raw[start + "```json".len()..];
        if let Some(end) = after_start.find("```") {
            let json_block = after_start[..end].trim();
            let parsed: StateHeaderV1 = serde_json::from_str(json_block)
                .context("failed parsing json block from state mirror")?;
            return Ok(parsed);
        }
    }

    let parsed: StateHeaderV1 =
        serde_json::from_str(raw.trim()).context("failed parsing raw state mirror as json")?;
    Ok(parsed)
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed creating mirror parent: {}", parent.display()))?;
    }

    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, content)
        .with_context(|| format!("failed writing mirror temp file: {}", temp_path.display()))?;

    if let Err(rename_error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(rename_error).with_context(|| {
            format!(
                "failed replacing mirror file atomically: {}",
                path.display()
            )
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{Memory, SqliteMemory};
    use chrono::Utc;
    use tempfile::TempDir;

    fn sample_state(objective: &str, summary: &str) -> StateHeaderV1 {
        StateHeaderV1 {
            schema_version: 1,
            identity_principles_hash: "identity-v1-abcd1234".to_string(),
            safety_posture: "strict".to_string(),
            current_objective: objective.to_string(),
            open_loops: vec!["Ship startup reconciliation".to_string()],
            next_actions: vec!["Sync backend to mirror".to_string()],
            commitments: vec!["Backend is canonical".to_string()],
            recent_context_summary: summary.to_string(),
            last_updated_at: Utc::now().to_rfc3339(),
        }
    }

    fn service_with_sqlite(
        tmp: &TempDir,
        mirror_filename: &str,
    ) -> BackendCanonicalStateHeaderPersistence {
        let memory: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).unwrap());
        let persona = PersonaConfig {
            state_mirror_filename: mirror_filename.to_string(),
            ..PersonaConfig::default()
        };

        BackendCanonicalStateHeaderPersistence::new(memory, tmp.path().to_path_buf(), persona)
    }

    #[tokio::test]
    async fn state_header_repairs_divergence() {
        let tmp = TempDir::new().unwrap();
        let service = service_with_sqlite(&tmp, "STATE.md");

        let backend_state = sample_state(
            "Ship deterministic persistence",
            "Backend snapshot is the canonical source of truth.",
        );
        service
            .persist_backend_canonical_and_sync_mirror(&backend_state)
            .await
            .unwrap();

        let divergent_mirror = sample_state(
            "Divergent mirror objective",
            "This should be repaired from backend on startup.",
        );
        let mirror_path = tmp.path().join("STATE.md");
        fs::write(
            &mirror_path,
            render_state_header_mirror_markdown(&divergent_mirror).unwrap(),
        )
        .unwrap();

        let reconciled = service
            .reconcile_mirror_from_backend_on_startup()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reconciled, backend_state);

        let repaired_mirror = service.read_mirror_state().unwrap().unwrap();
        assert_eq!(repaired_mirror, backend_state);
    }

    #[tokio::test]
    async fn state_header_post_write_syncs_mirror() {
        let tmp = TempDir::new().unwrap();
        let service = service_with_sqlite(&tmp, "STATE.md");

        let initial = sample_state("Objective A", "Summary A");
        service
            .persist_backend_canonical_and_sync_mirror(&initial)
            .await
            .unwrap();

        let updated = sample_state("Objective B", "Summary B");
        service
            .persist_backend_canonical_and_sync_mirror(&updated)
            .await
            .unwrap();

        let backend = service.load_backend_canonical().await.unwrap().unwrap();
        let mirror = service.read_mirror_state().unwrap().unwrap();
        assert_eq!(backend, updated);
        assert_eq!(mirror, updated);
    }

    #[tokio::test]
    async fn state_header_writeback_atomicity() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("STATE.md")).unwrap();

        let service = service_with_sqlite(&tmp, "STATE.md");
        let state = sample_state(
            "Preserve canonical write under mirror failure",
            "Mirror sync failure must not corrupt backend canonical state.",
        );

        let err = service
            .persist_backend_canonical_and_sync_mirror(&state)
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("failed replacing mirror file atomically"),
            "unexpected error: {err}"
        );

        let backend = service.load_backend_canonical().await.unwrap().unwrap();
        assert_eq!(backend, state);
    }
}
