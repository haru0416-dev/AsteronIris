use crate::memory::{Memory, MemoryRecallItem, RecallQuery};
use crate::security::external_content::{
    ExternalAction, decide_external_action, detect_injection_signals, sanitize_marker_collision,
    wrap_external_content,
};
use crate::security::policy::TenantPolicyContext;
use anyhow::Result;
use std::fmt::Write;

fn sanitize_external_fragment_for_context(slot_key: &str, value: &str) -> String {
    if !value.contains("digest_sha256=") {
        return "[external payload omitted by replay-ban policy]".to_string();
    }

    let signals = detect_injection_signals(value);
    let action = decide_external_action(&signals);
    match action {
        ExternalAction::Allow => wrap_external_content(slot_key, value),
        ExternalAction::Sanitize => {
            let sanitized = sanitize_marker_collision(value);
            wrap_external_content(slot_key, &sanitized)
        }
        ExternalAction::Block => {
            "[external summary blocked by policy during context replay]".to_string()
        }
    }
}

const CONTEXT_REPLAY_REVOKED_MARKERS: [&str; 2] = [
    "__LANCEDB_DEGRADED_SOFT_FORGET_MARKER__",
    "__LANCEDB_DEGRADED_TOMBSTONE_MARKER__",
];

fn is_revocation_marker_payload(value: &str) -> bool {
    CONTEXT_REPLAY_REVOKED_MARKERS
        .iter()
        .any(|marker| value.contains(marker))
}

async fn allow_context_replay_item(mem: &dyn Memory, entry: &MemoryRecallItem) -> bool {
    if is_revocation_marker_payload(&entry.value) {
        return false;
    }

    let resolved = mem.resolve_slot(&entry.entity_id, &entry.slot_key).await;
    matches!(resolved, Ok(Some(slot)) if slot.value == entry.value)
}

#[cfg(test)]
async fn build_context(mem: &dyn Memory, user_msg: &str) -> String {
    build_context_with_policy(mem, "default", user_msg, TenantPolicyContext::disabled())
        .await
        .unwrap_or_default()
}

fn build_context_recall_query(
    entity_id: &str,
    user_msg: &str,
    policy_context: TenantPolicyContext,
) -> Result<RecallQuery> {
    let query = RecallQuery::new(entity_id, user_msg, 8).with_policy_context(policy_context);
    query.enforce_policy()?;
    Ok(query)
}

pub(super) async fn build_context_with_policy(
    mem: &dyn Memory,
    entity_id: &str,
    user_msg: &str,
    policy_context: TenantPolicyContext,
) -> Result<String> {
    let mut context = String::new();

    // Pull relevant memories for this message
    let query = build_context_recall_query(entity_id, user_msg, policy_context)?;
    let entries = mem.recall_scoped(query).await?;
    let mut replayable_entries = Vec::with_capacity(entries.len());
    for entry in entries {
        if allow_context_replay_item(mem, &entry).await {
            replayable_entries.push(entry);
        }
    }

    if !replayable_entries.is_empty() {
        context.push_str("[Memory context]\n");
        for entry in &replayable_entries {
            if entry.slot_key.starts_with("external.") {
                let value = sanitize_external_fragment_for_context(&entry.slot_key, &entry.value);
                let _ = writeln!(context, "- {}: {}", entry.slot_key, value);
            } else {
                let _ = writeln!(context, "- {}: {}", entry.slot_key, entry.value);
            }
        }
        context.push('\n');
    }

    Ok(context)
}

pub async fn build_context_for_integration(
    mem: &dyn Memory,
    entity_id: &str,
    user_msg: &str,
    policy_context: TenantPolicyContext,
) -> Result<String> {
    build_context_with_policy(mem, entity_id, user_msg, policy_context).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{
        MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, SqliteMemory,
    };
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    async fn build_context_replay_ban_hides_raw_external_payload() {
        let temp = TempDir::new().unwrap();
        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).await.unwrap());

        mem.append_event(
            MemoryEventInput::new(
                "default",
                "external.gateway.webhook",
                MemoryEventType::FactAdded,
                "ATTACK_PAYLOAD_ALPHA",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.95)
            .with_importance(0.7),
        )
        .await
        .unwrap();

        let context = build_context(mem.as_ref(), "ATTACK_PAYLOAD_ALPHA").await;
        assert!(context.contains("external.gateway.webhook"));
        assert!(!context.contains("ATTACK_PAYLOAD_ALPHA"));
    }
}
