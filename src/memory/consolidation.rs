use crate::memory::traits::MemoryLayer;
use crate::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemoryProvenance, MemorySource, PrivacyLevel,
};
use crate::observability::Observer;
use crate::observability::traits::MemoryLifecycleSignal;
use crate::util::truncate_with_ellipsis;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

const STATE_FILE: &str = "memory_consolidation_state.json";
pub const CONSOLIDATION_SLOT_KEY: &str = "consolidation.semantic.latest";
const CONSOLIDATION_PROVENANCE_REF: &str = "memory.consolidation.session_to_semantic";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ConsolidationState {
    watermarks: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsolidationInput {
    pub entity_id: String,
    pub checkpoint_event_count: usize,
    pub user_message: String,
    pub assistant_response: String,
}

impl ConsolidationInput {
    #[must_use]
    pub fn new(
        entity_id: impl Into<String>,
        checkpoint_event_count: usize,
        user_message: impl Into<String>,
        assistant_response: impl Into<String>,
    ) -> Self {
        Self {
            entity_id: entity_id.into(),
            checkpoint_event_count,
            user_message: user_message.into(),
            assistant_response: assistant_response.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsolidationDisposition {
    Consolidated,
    SkippedNoSignal,
    SkippedCheckpoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsolidationOutput {
    pub disposition: ConsolidationDisposition,
    pub previous_watermark: usize,
    pub applied_watermark: usize,
}

fn state_path(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("state").join(STATE_FILE)
}

fn load_state(workspace_dir: &Path) -> Result<ConsolidationState> {
    let path = state_path(workspace_dir);
    if !path.exists() {
        return Ok(ConsolidationState::default());
    }

    let raw = fs::read_to_string(path)?;
    let state = serde_json::from_str(&raw).unwrap_or_default();
    Ok(state)
}

fn ensure_state_parent(workspace_dir: &Path) -> Result<()> {
    let path = state_path(workspace_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn save_state(workspace_dir: &Path, state: &ConsolidationState) -> Result<()> {
    ensure_state_parent(workspace_dir)?;
    let payload = serde_json::to_vec_pretty(state)?;
    fs::write(state_path(workspace_dir), payload)?;
    Ok(())
}

fn build_consolidation_value(input: &ConsolidationInput) -> String {
    let user = input
        .user_message
        .split_whitespace()
        .fold(String::new(), |mut acc, w| {
            if !acc.is_empty() {
                acc.push(' ');
            }
            acc.push_str(w);
            acc
        });
    let assistant =
        input
            .assistant_response
            .split_whitespace()
            .fold(String::new(), |mut acc, w| {
                if !acc.is_empty() {
                    acc.push(' ');
                }
                acc.push_str(w);
                acc
            });
    let user = truncate_with_ellipsis(&user, 120);
    let assistant = truncate_with_ellipsis(&assistant, 240);
    format!(
        "checkpoint={} | user={} | assistant={assistant}",
        input.checkpoint_event_count, user
    )
}

fn consolidation_lock(entity_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let locks = LOCKS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut guard = locks
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard
        .entry(entity_id.to_owned())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

pub async fn run_consolidation_once(
    memory: &dyn Memory,
    workspace_dir: &Path,
    input: &ConsolidationInput,
) -> Result<ConsolidationOutput> {
    if input.user_message.trim().is_empty() && input.assistant_response.trim().is_empty() {
        return Ok(ConsolidationOutput {
            disposition: ConsolidationDisposition::SkippedNoSignal,
            previous_watermark: 0,
            applied_watermark: 0,
        });
    }

    let entity_lock = consolidation_lock(&input.entity_id);
    let _guard = entity_lock.lock().await;
    ensure_state_parent(workspace_dir)?;
    let mut state = load_state(workspace_dir)?;
    let previous_watermark = state
        .watermarks
        .get(&input.entity_id)
        .copied()
        .unwrap_or_default();

    if input.checkpoint_event_count <= previous_watermark {
        return Ok(ConsolidationOutput {
            disposition: ConsolidationDisposition::SkippedCheckpoint,
            previous_watermark,
            applied_watermark: previous_watermark,
        });
    }

    let compacted = build_consolidation_value(input);
    memory
        .append_event(
            MemoryEventInput::new(
                &input.entity_id,
                CONSOLIDATION_SLOT_KEY,
                MemoryEventType::SummaryCompacted,
                compacted,
                MemorySource::System,
                PrivacyLevel::Private,
            )
            .with_layer(MemoryLayer::Semantic)
            .with_confidence(0.85)
            .with_importance(0.65)
            .with_provenance(MemoryProvenance::source_reference(
                MemorySource::System,
                CONSOLIDATION_PROVENANCE_REF,
            )),
        )
        .await?;

    state
        .watermarks
        .insert(input.entity_id.clone(), input.checkpoint_event_count);
    save_state(workspace_dir, &state)?;

    Ok(ConsolidationOutput {
        disposition: ConsolidationDisposition::Consolidated,
        previous_watermark,
        applied_watermark: input.checkpoint_event_count,
    })
}

pub fn enqueue_consolidation_task(
    memory: Arc<dyn Memory>,
    workspace_dir: PathBuf,
    input: ConsolidationInput,
    observer: Arc<dyn Observer>,
) {
    tokio::spawn(async move {
        observer.record_memory_lifecycle(MemoryLifecycleSignal::ConsolidationStarted);
        if let Err(error) = run_consolidation_once(memory.as_ref(), &workspace_dir, &input).await {
            tracing::warn!(error = %error, "post-turn consolidation task failed; answer path preserved");
        } else {
            observer.record_memory_lifecycle(MemoryLifecycleSignal::ConsolidationCompleted);
        }
    });
}
