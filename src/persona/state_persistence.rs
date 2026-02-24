use super::person_identity::{person_entity_id, sanitize_person_id};
use super::state_header::StateHeader;
use crate::config::PersonaConfig;
use crate::memory::traits::Memory;
use crate::memory::types::{
    MemoryEventInput, MemoryEventType, MemoryProvenance, MemorySource, PrivacyLevel, SourceKind,
};
use anyhow::{Context, Result};
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const CANONICAL_STATE_HEADER_KEY: &str = "persona/state_header/v1";

const STATE_HEADER_MIRROR_HEADER: &str = "# Persona State Header\n\nbackend_canonical: true\n\n";

pub struct BackendCanonicalStateHeaderPersistence {
    memory: Arc<dyn Memory>,
    workspace_dir: PathBuf,
    persona: PersonaConfig,
    person_id: String,
}

impl BackendCanonicalStateHeaderPersistence {
    pub fn new(
        memory: Arc<dyn Memory>,
        workspace_dir: PathBuf,
        persona: PersonaConfig,
        person_id: impl Into<String>,
    ) -> Self {
        Self {
            memory,
            workspace_dir,
            persona,
            person_id: sanitize_person_id(&person_id.into()),
        }
    }

    fn person_entity_id(&self) -> String {
        person_entity_id(self.person_id_or_default())
    }

    fn person_canonical_key(&self) -> String {
        format!(
            "persona/{}/state_header/v1",
            self.person_id_or_default().replace(':', "_")
        )
    }

    fn person_id_or_default(&self) -> &str {
        if self.person_id.is_empty() {
            "local-default"
        } else {
            &self.person_id
        }
    }

    pub async fn load_backend_canonical(&self) -> Result<Option<StateHeader>> {
        let person_entity_id = self.person_entity_id();
        let person_slot_key = self.person_canonical_key();

        let Some(entry) = self
            .memory
            .resolve_slot(&person_entity_id, &person_slot_key)
            .await?
        else {
            return Ok(None);
        };

        let parsed: StateHeader = serde_json::from_str(&entry.value).with_context(|| {
            format!(
                "failed to parse backend canonical state header key: {CANONICAL_STATE_HEADER_KEY}"
            )
        })?;
        parsed.validate(&self.persona)?;

        Ok(Some(parsed))
    }

    pub async fn reconcile_mirror_from_backend_on_startup(&self) -> Result<Option<StateHeader>> {
        let canonical = if let Some(existing) = self.load_backend_canonical().await? {
            existing
        } else {
            let seeded = Self::seed_minimal_backend_canonical();
            self.persist_backend_canonical_and_sync_mirror(&seeded)
                .await?;
            seeded
        };

        self.sync_mirror_from_backend_canonical(&canonical)?;
        Ok(Some(canonical))
    }

    pub async fn persist_backend_canonical_and_sync_mirror(
        &self,
        state: &StateHeader,
    ) -> Result<()> {
        state.validate(&self.persona)?;

        let person_entity_id = self.person_entity_id();
        let person_slot_key = self.person_canonical_key();

        let serialized = serde_json::to_string(state)?;
        let input = MemoryEventInput::new(
            person_entity_id,
            person_slot_key,
            MemoryEventType::FactUpdated,
            serialized,
            MemorySource::System,
            PrivacyLevel::Private,
        )
        .with_confidence(0.95)
        .with_importance(1.0)
        .with_source_kind(SourceKind::Manual)
        .with_source_ref(format!("persona-state-writeback:{}", state.last_updated_at))
        .with_provenance(MemoryProvenance::source_reference(
            MemorySource::System,
            "persona.state_header.writeback",
        ));
        self.memory.append_event(input).await?;

        self.sync_mirror_from_backend_canonical(state)
    }

    pub fn read_mirror_state(&self) -> Result<Option<StateHeader>> {
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

    fn sync_mirror_from_backend_canonical(&self, state: &StateHeader) -> Result<()> {
        let mirror_path = self.state_mirror_path();
        let content = render_state_header_mirror_markdown(state)?;
        write_atomic(&mirror_path, &content)
    }

    fn seed_minimal_backend_canonical() -> StateHeader {
        StateHeader {
            identity_principles_hash: "bootstrap-minimal-v1".to_string(),
            safety_posture: "strict".to_string(),
            current_objective: "Initialize persona state continuity from backend canonical."
                .to_string(),
            open_loops: Vec::new(),
            next_actions: Vec::new(),
            commitments: Vec::new(),
            recent_context_summary:
                "Seeded minimal valid state because canonical backend entry was missing at startup."
                    .to_string(),
            last_updated_at: Utc::now().to_rfc3339(),
        }
    }
}

fn render_state_header_mirror_markdown(state: &StateHeader) -> Result<String> {
    let json = serde_json::to_string_pretty(state)?;
    Ok(format!(
        "{STATE_HEADER_MIRROR_HEADER}```json\n{json}\n```\n"
    ))
}

fn parse_state_header_mirror_markdown(raw: &str) -> Result<StateHeader> {
    if let Some(start) = raw.find("```json") {
        let after_start = &raw[start + "```json".len()..];
        if let Some(end) = after_start.find("```") {
            let json_block = after_start[..end].trim();
            let parsed: StateHeader = serde_json::from_str(json_block)
                .context("failed parsing json block from state mirror")?;
            return Ok(parsed);
        }
    }

    let parsed: StateHeader =
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
    use crate::memory::sqlite::SqliteMemory;
    use crate::memory::traits::Memory;
    use chrono::Utc;
    use tempfile::TempDir;

    fn sample_state(objective: &str, summary: &str) -> StateHeader {
        StateHeader {
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

    async fn service_with_sqlite(
        tmp: &TempDir,
        mirror_filename: &str,
    ) -> BackendCanonicalStateHeaderPersistence {
        let memory: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).await.unwrap());
        let persona = PersonaConfig {
            state_mirror_filename: mirror_filename.to_string(),
            ..PersonaConfig::default()
        };

        BackendCanonicalStateHeaderPersistence::new(
            memory,
            tmp.path().to_path_buf(),
            persona,
            "person-test",
        )
    }

    #[tokio::test]
    async fn state_header_person_namespace_persists_under_person_slot() {
        let tmp = TempDir::new().unwrap();
        let memory: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).await.unwrap());
        let persona = PersonaConfig::default();
        let service = BackendCanonicalStateHeaderPersistence::new(
            Arc::clone(&memory),
            tmp.path().to_path_buf(),
            persona,
            "alice",
        );

        let state = sample_state("Person objective", "Person summary");
        service
            .persist_backend_canonical_and_sync_mirror(&state)
            .await
            .unwrap();

        let slot = memory
            .resolve_slot("person:alice", "persona/alice/state_header/v1")
            .await
            .unwrap()
            .unwrap();
        let parsed: StateHeader = serde_json::from_str(&slot.value).unwrap();
        assert_eq!(parsed, state);
    }

    #[tokio::test]
    async fn persona_bootstrap_seeds_minimal_state() {
        let tmp = TempDir::new().unwrap();
        let service = service_with_sqlite(&tmp, "STATE.md").await;

        let seeded = service
            .reconcile_mirror_from_backend_on_startup()
            .await
            .unwrap()
            .unwrap();

        assert!(!seeded.identity_principles_hash.trim().is_empty());
        assert!(!seeded.safety_posture.trim().is_empty());
        assert!(!seeded.current_objective.trim().is_empty());
        assert!(!seeded.recent_context_summary.trim().is_empty());

        let backend = service.load_backend_canonical().await.unwrap().unwrap();
        let mirror = service.read_mirror_state().unwrap().unwrap();
        assert_eq!(backend, seeded);
        assert_eq!(mirror, seeded);
    }

    #[tokio::test]
    async fn state_header_repairs_divergence() {
        let tmp = TempDir::new().unwrap();
        let service = service_with_sqlite(&tmp, "STATE.md").await;

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
        let service = service_with_sqlite(&tmp, "STATE.md").await;

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
}
