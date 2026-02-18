use super::traits::{
    BeliefSlot, ForgetArtifact, ForgetArtifactCheck, ForgetArtifactObservation,
    ForgetArtifactRequirement, ForgetMode, ForgetOutcome, Memory, MemoryCategory, MemoryEntry,
    MemoryEvent, MemoryEventInput, MemoryLayer, MemoryProvenance, MemoryRecallItem, MemorySource,
    PrivacyLevel, RecallQuery,
};
use async_trait::async_trait;
use chrono::Local;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;

/// Markdown-based memory — plain files as source of truth
///
/// Layout:
///   workspace/MEMORY.md          — curated long-term memory (core)
///   workspace/memory/YYYY-MM-DD.md — daily logs (append-only)
pub struct MarkdownMemory {
    workspace_dir: PathBuf,
}

#[derive(Debug)]
struct ParsedMarkdownLine {
    key: String,
    content: String,
    layer: Option<MemoryLayer>,
    provenance: Option<MemoryProvenance>,
}

#[allow(
    clippy::unused_self,
    clippy::unused_async,
    clippy::trivially_copy_pass_by_ref
)]
impl MarkdownMemory {
    pub fn new(workspace_dir: &Path) -> Self {
        Self {
            workspace_dir: workspace_dir.to_path_buf(),
        }
    }

    fn memory_dir(&self) -> PathBuf {
        self.workspace_dir.join("memory")
    }

    fn core_path(&self) -> PathBuf {
        self.workspace_dir.join("MEMORY.md")
    }

    fn daily_path(&self) -> PathBuf {
        let date = Local::now().format("%Y-%m-%d").to_string();
        self.memory_dir().join(format!("{date}.md"))
    }

    async fn ensure_dirs(&self) -> anyhow::Result<()> {
        fs::create_dir_all(self.memory_dir()).await?;
        Ok(())
    }

    async fn append_to_file(&self, path: &Path, content: &str) -> anyhow::Result<()> {
        self.ensure_dirs().await?;

        let existing = if path.exists() {
            fs::read_to_string(path).await.unwrap_or_default()
        } else {
            String::new()
        };

        let updated = if existing.is_empty() {
            let header = if path == self.core_path() {
                "# Long-Term Memory\n\n"
            } else {
                let date = Local::now().format("%Y-%m-%d").to_string();
                &format!("# Daily Log — {date}\n\n")
            };
            format!("{header}{content}\n")
        } else {
            format!("{existing}\n{content}\n")
        };

        fs::write(path, updated).await?;
        Ok(())
    }

    fn parse_entries_from_file(
        path: &Path,
        content: &str,
        category: &MemoryCategory,
    ) -> Vec<MemoryEntry> {
        let filename = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        content
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty() && !trimmed.starts_with('#')
            })
            .enumerate()
            .filter_map(|(i, line)| Self::parse_markdown_entry_line(line).map(|entry| (i, entry)))
            .map(|(i, entry)| MemoryEntry {
                id: format!("{filename}:{i}"),
                key: entry.key,
                content: entry.content,
                category: category.clone(),
                timestamp: filename.to_string(),
                session_id: None,
                score: None,
            })
            .collect()
    }

    fn encode_tag_value(value: &str) -> String {
        value
            .chars()
            .flat_map(|ch| match ch {
                '%' => "%25".chars().collect::<Vec<_>>(),
                ';' => "%3B".chars().collect::<Vec<_>>(),
                '=' => "%3D".chars().collect::<Vec<_>>(),
                '&' => "%26".chars().collect::<Vec<_>>(),
                '\\' => "%5C".chars().collect::<Vec<_>>(),
                '[' => "%5B".chars().collect::<Vec<_>>(),
                ']' => "%5D".chars().collect::<Vec<_>>(),
                _ => vec![ch],
            })
            .collect()
    }

    fn decode_tag_value(value: &str) -> Option<String> {
        let mut decoded = String::new();
        let mut chars = value.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '%' {
                decoded.push(ch);
                continue;
            }

            let a = chars.next()?;
            let b = chars.next()?;
            let hex = format!("{a}{b}");
            let decoded_byte = u8::from_str_radix(&hex, 16).ok()?;
            decoded.push(decoded_byte as char);
        }
        Some(decoded)
    }

    fn parse_markdown_tags(raw: &str) -> HashMap<String, String> {
        raw.split(';')
            .filter_map(|chunk| {
                let (k, v) = chunk.split_once('=')?;
                if k.is_empty() || v.is_empty() {
                    return None;
                }
                let value = Self::decode_tag_value(v)?;
                Some((k.to_string(), value))
            })
            .collect()
    }

    fn memory_layer_to_str(layer: &MemoryLayer) -> &'static str {
        match layer {
            MemoryLayer::Working => "working",
            MemoryLayer::Episodic => "episodic",
            MemoryLayer::Semantic => "semantic",
            MemoryLayer::Procedural => "procedural",
            MemoryLayer::Identity => "identity",
        }
    }

    fn parse_memory_layer(raw: &str) -> Option<MemoryLayer> {
        match raw {
            "working" => Some(MemoryLayer::Working),
            "episodic" => Some(MemoryLayer::Episodic),
            "semantic" => Some(MemoryLayer::Semantic),
            "procedural" => Some(MemoryLayer::Procedural),
            "identity" => Some(MemoryLayer::Identity),
            _ => None,
        }
    }

    fn memory_source_to_str(source: &MemorySource) -> &'static str {
        match source {
            MemorySource::ExplicitUser => "explicit_user",
            MemorySource::ToolVerified => "tool_verified",
            MemorySource::System => "system",
            MemorySource::Inferred => "inferred",
        }
    }

    fn parse_memory_source(raw: &str) -> Option<MemorySource> {
        match raw {
            "explicit_user" => Some(MemorySource::ExplicitUser),
            "tool_verified" => Some(MemorySource::ToolVerified),
            "system" => Some(MemorySource::System),
            "inferred" => Some(MemorySource::Inferred),
            _ => None,
        }
    }

    fn format_tagged_line(
        key: &str,
        value: &str,
        layer: &MemoryLayer,
        provenance: Option<&MemoryProvenance>,
    ) -> String {
        let mut tag_fields = vec![format!("layer={}", Self::memory_layer_to_str(layer))];

        if let Some(provenance) = provenance {
            tag_fields.push(format!(
                "provenance_source_class={}",
                Self::memory_source_to_str(&provenance.source_class)
            ));
            tag_fields.push(format!(
                "provenance_reference={}",
                Self::encode_tag_value(&provenance.reference)
            ));

            if let Some(uri) = &provenance.evidence_uri {
                tag_fields.push(format!(
                    "provenance_evidence_uri={}",
                    Self::encode_tag_value(uri)
                ));
            }
        }

        let tagged = format!("[md:{}]", tag_fields.join(";"));
        format!("- **{key}** {tagged}: {value}")
    }

    fn parse_markdown_entry_line(line: &str) -> Option<ParsedMarkdownLine> {
        let line = line.trim();
        let without_bullet = line.strip_prefix("- ")?;
        let without_key = without_bullet.strip_prefix("**")?;
        let end_key = without_key.find("**")?;
        let (key, rest) = without_key.split_at(end_key);
        let rest = rest.strip_prefix("**").unwrap_or("").trim_start();

        if let Some(content) = rest.strip_prefix(": ") {
            return Some(ParsedMarkdownLine {
                key: key.to_string(),
                content: content.to_string(),
                layer: None,
                provenance: None,
            });
        }

        if let Some(rest_after_marker) = rest.strip_prefix("[md:") {
            let tag_end = rest_after_marker.find("]: ");
            let Some(tag_end) = tag_end else {
                return Some(ParsedMarkdownLine {
                    key: key.to_string(),
                    content: format!("[md:{rest_after_marker}"),
                    layer: None,
                    provenance: None,
                });
            };

            let raw_tags = &rest_after_marker[..tag_end];
            let content = &rest_after_marker[(tag_end + 3)..];
            let tags = Self::parse_markdown_tags(raw_tags);

            let layer = tags
                .get("layer")
                .and_then(|value| Self::parse_memory_layer(value))
                .unwrap_or(MemoryLayer::Working);

            let provenance = tags.get("provenance_source_class").and_then(|source_raw| {
                let source = Self::parse_memory_source(source_raw)?;
                let reference = tags.get("provenance_reference")?.clone();
                Some(MemoryProvenance {
                    source_class: source,
                    reference,
                    evidence_uri: tags
                        .get("provenance_evidence_uri")
                        .map(std::string::ToString::to_string),
                })
            });

            return Some(ParsedMarkdownLine {
                key: key.to_string(),
                content: content.to_string(),
                layer: Some(layer),
                provenance,
            });
        }

        None
    }

    #[allow(dead_code)]
    fn parse_markdown_entry_metadata(
        line: &str,
    ) -> Option<(Option<MemoryLayer>, Option<MemoryProvenance>)> {
        Self::parse_markdown_entry_line(line).map(|entry| (entry.layer, entry.provenance))
    }

    async fn read_all_entries(&self) -> anyhow::Result<Vec<MemoryEntry>> {
        let mut entries = Vec::new();

        // Read MEMORY.md (core)
        let core_path = self.core_path();
        if core_path.exists() {
            let content = fs::read_to_string(&core_path).await?;
            entries.extend(Self::parse_entries_from_file(
                &core_path,
                &content,
                &MemoryCategory::Core,
            ));
        }

        // Read daily logs
        let mem_dir = self.memory_dir();
        if mem_dir.exists() {
            let mut dir = fs::read_dir(&mem_dir).await?;
            while let Some(entry) = dir.next_entry().await? {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    let content = fs::read_to_string(&path).await?;
                    entries.extend(Self::parse_entries_from_file(
                        &path,
                        &content,
                        &MemoryCategory::Daily,
                    ));
                }
            }
        }

        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(entries)
    }
}

impl MarkdownMemory {
    #[allow(clippy::unused_self)]
    fn name(&self) -> &str {
        "markdown"
    }

    async fn upsert_projection_entry(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
        layer: MemoryLayer,
        provenance: Option<&MemoryProvenance>,
    ) -> anyhow::Result<()> {
        let entry = Self::format_tagged_line(key, content, &layer, provenance);
        let path = match category {
            MemoryCategory::Core => self.core_path(),
            _ => self.daily_path(),
        };
        self.append_to_file(&path, &entry).await
    }

    async fn search_projection(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let all = self.read_all_entries().await?;
        let query_lower = query.to_lowercase();
        let keywords: Vec<&str> = query_lower.split_whitespace().collect();

        let mut scored: Vec<MemoryEntry> = all
            .into_iter()
            .filter_map(|mut entry| {
                let content_lower = entry.content.to_lowercase();
                let matched = keywords
                    .iter()
                    .filter(|kw| content_lower.contains(**kw))
                    .count();
                if matched > 0 {
                    #[allow(clippy::cast_precision_loss)]
                    let score = matched as f64 / keywords.len() as f64;
                    entry.score = Some(score);
                    Some(entry)
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(limit);
        Ok(scored)
    }

    async fn fetch_projection_entry(&self, key: &str) -> anyhow::Result<Option<MemoryEntry>> {
        let all = self.read_all_entries().await?;
        Ok(all
            .into_iter()
            .find(|e| e.key == key || e.content.contains(key)))
    }

    async fn list_projection_entries(
        &self,
        category: Option<&MemoryCategory>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let all = self.read_all_entries().await?;
        match category {
            Some(cat) => Ok(all.into_iter().filter(|e| &e.category == cat).collect()),
            None => Ok(all),
        }
    }

    #[allow(clippy::unused_async)]
    async fn delete_projection_entry(&self, _key: &str) -> anyhow::Result<bool> {
        // Markdown memory is append-only by design (audit trail)
        // Return false to indicate the entry wasn't removed
        Ok(false)
    }

    async fn count_projection_entries(&self) -> anyhow::Result<usize> {
        let all = self.read_all_entries().await?;
        Ok(all.len())
    }

    async fn append_event(&self, input: MemoryEventInput) -> anyhow::Result<MemoryEvent> {
        let input = input.normalize_for_ingress()?;
        let key = format!("{}:{}", input.entity_id, input.slot_key);
        let category = match input.source {
            MemorySource::ExplicitUser | MemorySource::ToolVerified => MemoryCategory::Core,
            MemorySource::System => MemoryCategory::Daily,
            MemorySource::Inferred => MemoryCategory::Conversation,
        };
        self.upsert_projection_entry(
            &key,
            &input.value,
            category,
            input.layer,
            input.provenance.as_ref(),
        )
        .await?;

        Ok(MemoryEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            entity_id: input.entity_id,
            slot_key: input.slot_key,
            event_type: input.event_type,
            value: input.value,
            source: input.source,
            confidence: input.confidence,
            importance: input.importance,
            provenance: input.provenance,
            privacy_level: input.privacy_level,
            occurred_at: input.occurred_at,
            ingested_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    async fn recall_scoped(&self, query: RecallQuery) -> anyhow::Result<Vec<MemoryRecallItem>> {
        query.enforce_policy()?;

        let scoped = format!("{} {}", query.entity_id, query.query);
        let rows = self.search_projection(&scoped, query.limit).await?;
        Ok(rows
            .into_iter()
            .map(|entry| MemoryRecallItem {
                entity_id: query.entity_id.clone(),
                slot_key: entry.key,
                value: entry.content,
                source: MemorySource::System,
                confidence: 0.8,
                importance: 0.5,
                privacy_level: PrivacyLevel::Private,
                score: entry.score.unwrap_or(0.0),
                occurred_at: entry.timestamp,
            })
            .collect())
    }

    async fn resolve_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
    ) -> anyhow::Result<Option<BeliefSlot>> {
        let key = format!("{entity_id}:{slot_key}");
        let Some(entry) = self.fetch_projection_entry(&key).await? else {
            return Ok(None);
        };

        Ok(Some(BeliefSlot {
            entity_id: entity_id.to_string(),
            slot_key: slot_key.to_string(),
            value: entry.content,
            source: MemorySource::System,
            confidence: 0.8,
            importance: 0.5,
            privacy_level: PrivacyLevel::Private,
            updated_at: entry.timestamp,
        }))
    }

    async fn forget_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
        mode: ForgetMode,
        reason: &str,
    ) -> anyhow::Result<ForgetOutcome> {
        let key = format!("{entity_id}:{slot_key}");
        let _ = reason;
        let applied = self.delete_projection_entry(&key).await?;

        let slot_observed = if self.resolve_slot(entity_id, slot_key).await?.is_some() {
            ForgetArtifactObservation::PresentRetrievable
        } else {
            ForgetArtifactObservation::Absent
        };
        let projection_observed = if self.fetch_projection_entry(&key).await?.is_some() {
            ForgetArtifactObservation::PresentRetrievable
        } else {
            ForgetArtifactObservation::Absent
        };

        let slot_requirement = match mode {
            ForgetMode::Hard => ForgetArtifactRequirement::MustBeAbsent,
            ForgetMode::Soft | ForgetMode::Tombstone => {
                ForgetArtifactRequirement::MustBeNonRetrievable
            }
        };
        let projection_requirement = slot_requirement;

        let artifact_checks = vec![
            ForgetArtifactCheck::new(ForgetArtifact::Slot, slot_requirement, slot_observed),
            ForgetArtifactCheck::new(
                ForgetArtifact::RetrievalDocs,
                ForgetArtifactRequirement::NotGoverned,
                ForgetArtifactObservation::Absent,
            ),
            ForgetArtifactCheck::new(
                ForgetArtifact::ProjectionDocs,
                projection_requirement,
                projection_observed,
            ),
            ForgetArtifactCheck::new(
                ForgetArtifact::Caches,
                ForgetArtifactRequirement::NotGoverned,
                ForgetArtifactObservation::Absent,
            ),
            ForgetArtifactCheck::new(
                ForgetArtifact::Ledger,
                ForgetArtifactRequirement::NotGoverned,
                ForgetArtifactObservation::Absent,
            ),
        ];

        Ok(ForgetOutcome::from_checks(
            entity_id,
            slot_key,
            mode,
            applied,
            true,
            artifact_checks,
        ))
    }

    async fn count_events(&self, _entity_id: Option<&str>) -> anyhow::Result<usize> {
        self.count_projection_entries().await
    }

    #[allow(clippy::unused_async)]
    async fn health_check(&self) -> bool {
        self.workspace_dir.exists()
    }
}

#[async_trait]
impl Memory for MarkdownMemory {
    fn name(&self) -> &str {
        MarkdownMemory::name(self)
    }

    async fn health_check(&self) -> bool {
        MarkdownMemory::health_check(self).await
    }

    async fn append_event(&self, input: MemoryEventInput) -> anyhow::Result<MemoryEvent> {
        MarkdownMemory::append_event(self, input).await
    }

    async fn recall_scoped(&self, query: RecallQuery) -> anyhow::Result<Vec<MemoryRecallItem>> {
        MarkdownMemory::recall_scoped(self, query).await
    }

    async fn resolve_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
    ) -> anyhow::Result<Option<BeliefSlot>> {
        MarkdownMemory::resolve_slot(self, entity_id, slot_key).await
    }

    async fn forget_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
        mode: ForgetMode,
        reason: &str,
    ) -> anyhow::Result<ForgetOutcome> {
        MarkdownMemory::forget_slot(self, entity_id, slot_key, mode, reason).await
    }

    async fn count_events(&self, entity_id: Option<&str>) -> anyhow::Result<usize> {
        MarkdownMemory::count_events(self, entity_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as sync_fs;
    use tempfile::TempDir;

    fn temp_workspace() -> (TempDir, MarkdownMemory) {
        let tmp = TempDir::new().unwrap();
        let mem = MarkdownMemory::new(tmp.path());
        (tmp, mem)
    }

    #[tokio::test]
    async fn markdown_name() {
        let (_tmp, mem) = temp_workspace();
        assert_eq!(mem.name(), "markdown");
    }

    #[tokio::test]
    async fn markdown_health_check() {
        let (_tmp, mem) = temp_workspace();
        assert!(mem.health_check().await);
    }

    #[tokio::test]
    async fn markdown_store_core() {
        let (_tmp, mem) = temp_workspace();
        mem.upsert_projection_entry(
            "pref",
            "User likes Rust",
            MemoryCategory::Core,
            MemoryLayer::Working,
            None,
        )
        .await
        .unwrap();
        let content = sync_fs::read_to_string(mem.core_path()).unwrap();
        assert!(content.contains("User likes Rust"));
    }

    #[tokio::test]
    async fn markdown_store_daily() {
        let (_tmp, mem) = temp_workspace();
        mem.upsert_projection_entry(
            "note",
            "Finished tests",
            MemoryCategory::Daily,
            MemoryLayer::Working,
            None,
        )
        .await
        .unwrap();
        let path = mem.daily_path();
        let content = sync_fs::read_to_string(path).unwrap();
        assert!(content.contains("Finished tests"));
    }

    #[tokio::test]
    async fn markdown_recall_keyword() {
        let (_tmp, mem) = temp_workspace();
        mem.upsert_projection_entry(
            "a",
            "Rust is fast",
            MemoryCategory::Core,
            MemoryLayer::Working,
            None,
        )
        .await
        .unwrap();
        mem.upsert_projection_entry(
            "b",
            "Python is slow",
            MemoryCategory::Core,
            MemoryLayer::Working,
            None,
        )
        .await
        .unwrap();
        mem.upsert_projection_entry(
            "c",
            "Rust and safety",
            MemoryCategory::Core,
            MemoryLayer::Working,
            None,
        )
        .await
        .unwrap();

        let results = mem.search_projection("Rust", 10).await.unwrap();
        assert!(results.len() >= 2);
        assert!(results
            .iter()
            .all(|r| r.content.to_lowercase().contains("rust")));
    }

    #[tokio::test]
    async fn markdown_recall_no_match() {
        let (_tmp, mem) = temp_workspace();
        mem.upsert_projection_entry(
            "a",
            "Rust is great",
            MemoryCategory::Core,
            MemoryLayer::Working,
            None,
        )
        .await
        .unwrap();
        let results = mem.search_projection("javascript", 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn markdown_count() {
        let (_tmp, mem) = temp_workspace();
        mem.upsert_projection_entry(
            "a",
            "first",
            MemoryCategory::Core,
            MemoryLayer::Working,
            None,
        )
        .await
        .unwrap();
        mem.upsert_projection_entry(
            "b",
            "second",
            MemoryCategory::Core,
            MemoryLayer::Working,
            None,
        )
        .await
        .unwrap();
        let count = mem.count_projection_entries().await.unwrap();
        assert!(count >= 2);
    }

    #[tokio::test]
    async fn markdown_list_by_category() {
        let (_tmp, mem) = temp_workspace();
        mem.upsert_projection_entry(
            "a",
            "core fact",
            MemoryCategory::Core,
            MemoryLayer::Working,
            None,
        )
        .await
        .unwrap();
        mem.upsert_projection_entry(
            "b",
            "daily note",
            MemoryCategory::Daily,
            MemoryLayer::Working,
            None,
        )
        .await
        .unwrap();

        let core = mem
            .list_projection_entries(Some(&MemoryCategory::Core))
            .await
            .unwrap();
        assert!(core.iter().all(|e| e.category == MemoryCategory::Core));

        let daily = mem
            .list_projection_entries(Some(&MemoryCategory::Daily))
            .await
            .unwrap();
        assert!(daily.iter().all(|e| e.category == MemoryCategory::Daily));
    }

    #[tokio::test]
    async fn markdown_forget_is_noop() {
        let (_tmp, mem) = temp_workspace();
        mem.upsert_projection_entry(
            "a",
            "permanent",
            MemoryCategory::Core,
            MemoryLayer::Working,
            None,
        )
        .await
        .unwrap();
        let removed = mem.delete_projection_entry("a").await.unwrap();
        assert!(!removed, "Markdown memory is append-only");
    }

    #[tokio::test]
    async fn markdown_empty_recall() {
        let (_tmp, mem) = temp_workspace();
        let results = mem.search_projection("anything", 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn markdown_empty_count() {
        let (_tmp, mem) = temp_workspace();
        assert_eq!(mem.count_projection_entries().await.unwrap(), 0);
    }
}
