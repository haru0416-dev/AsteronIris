use super::SqliteMemory;
use crate::memory::traits::MemoryLayer;
use crate::memory::vector;
use crate::memory::{MemoryCategory, MemoryEntry};
use chrono::Local;
use rusqlite::{params, ToSql};
use uuid::Uuid;

#[allow(clippy::too_many_lines, clippy::unused_async)]
impl SqliteMemory {
    pub(super) async fn upsert_projection_entry(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
    ) -> anyhow::Result<()> {
        let embedding_bytes = self
            .get_or_compute_embedding(content)
            .await?
            .map(|emb| vector::vec_to_bytes(&emb));

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let now = Local::now().to_rfc3339();
        let cat = Self::category_to_str(&category);
        let layer = Self::layer_to_str(&MemoryLayer::Working);
        let retention_tier = Self::retention_tier_for_layer(&MemoryLayer::Working);
        let id = Uuid::new_v4().to_string();

        conn.execute(
            "INSERT INTO memories (
                id, key, content, category, layer,
                provenance_source_class, provenance_reference, provenance_evidence_uri,
                retention_tier, retention_expires_at,
                embedding, created_at, updated_at
            )
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, ?6, NULL, ?7, ?8, ?9)
             ON CONFLICT(key) DO UPDATE SET
                content = excluded.content,
                category = excluded.category,
                layer = excluded.layer,
                provenance_source_class = excluded.provenance_source_class,
                provenance_reference = excluded.provenance_reference,
                provenance_evidence_uri = excluded.provenance_evidence_uri,
                retention_tier = excluded.retention_tier,
                retention_expires_at = excluded.retention_expires_at,
                embedding = excluded.embedding,
                updated_at = excluded.updated_at",
            params![
                id,
                key,
                content,
                cat,
                layer,
                retention_tier,
                embedding_bytes,
                now,
                now
            ],
        )?;

        Ok(())
    }

    pub(super) async fn search_projection(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        if query.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let query_embedding = self.get_or_compute_embedding(query).await?;

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let search_limit = limit.saturating_mul(2);
        let keyword_results = Self::fts5_search(&conn, query, search_limit).unwrap_or_default();

        let vector_results = if let Some(ref qe) = query_embedding {
            Self::vector_search(&conn, qe, search_limit).unwrap_or_default()
        } else {
            Vec::new()
        };

        let merged = if vector_results.is_empty() {
            keyword_results
                .iter()
                .map(|(id, score)| vector::ScoredResult {
                    id: id.clone(),
                    vector_score: None,
                    keyword_score: Some(*score),
                    final_score: *score,
                })
                .collect::<Vec<_>>()
        } else {
            vector::hybrid_merge(
                &vector_results,
                &keyword_results,
                self.vector_weight,
                self.keyword_weight,
                limit,
            )
        };

        let mut results = Vec::new();
        let mut by_id_stmt = conn
            .prepare("SELECT id, key, content, category, created_at FROM memories WHERE id = ?1")?;
        for scored in &merged {
            if let Ok(entry) = by_id_stmt.query_row(params![scored.id], |row| {
                Ok(MemoryEntry {
                    id: row.get(0)?,
                    key: row.get(1)?,
                    content: row.get(2)?,
                    category: Self::str_to_category(&row.get::<_, String>(3)?),
                    timestamp: row.get(4)?,
                    session_id: None,
                    score: Some(f64::from(scored.final_score)),
                })
            }) {
                results.push(entry);
            }
        }

        if results.is_empty() {
            let keywords: Vec<String> =
                query.split_whitespace().map(|w| format!("%{w}%")).collect();
            if !keywords.is_empty() {
                let conditions: Vec<String> = keywords
                    .iter()
                    .enumerate()
                    .map(|(i, _)| {
                        format!("(content LIKE ?{} OR key LIKE ?{})", i * 2 + 1, i * 2 + 2)
                    })
                    .collect();
                let where_clause = conditions.join(" OR ");
                let sql = format!(
                    "SELECT id, key, content, category, created_at FROM memories
                     WHERE {where_clause}
                     ORDER BY updated_at DESC
                     LIMIT ?{}",
                    keywords.len() * 2 + 1
                );
                let mut stmt = conn.prepare(&sql)?;
                let mut param_values: Vec<&dyn ToSql> = Vec::with_capacity(keywords.len() * 2 + 1);
                for kw in &keywords {
                    param_values.push(kw);
                    param_values.push(kw);
                }
                #[allow(clippy::cast_possible_wrap)]
                let limit_i64 = limit as i64;
                param_values.push(&limit_i64);
                let rows = stmt.query_map(param_values.as_slice(), |row| {
                    Ok(MemoryEntry {
                        id: row.get(0)?,
                        key: row.get(1)?,
                        content: row.get(2)?,
                        category: Self::str_to_category(&row.get::<_, String>(3)?),
                        timestamp: row.get(4)?,
                        session_id: None,
                        score: Some(1.0),
                    })
                })?;
                for row in rows {
                    results.push(row?);
                }
            }
        }

        results.truncate(limit);
        Ok(results)
    }

    pub(super) async fn fetch_projection_entry(
        &self,
        key: &str,
    ) -> anyhow::Result<Option<MemoryEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let mut stmt = conn.prepare(
            "SELECT id, key, content, category, created_at FROM memories WHERE key = ?1",
        )?;

        let mut rows = stmt.query_map(params![key], |row| {
            Ok(MemoryEntry {
                id: row.get(0)?,
                key: row.get(1)?,
                content: row.get(2)?,
                category: Self::str_to_category(&row.get::<_, String>(3)?),
                timestamp: row.get(4)?,
                session_id: None,
                score: None,
            })
        })?;

        match rows.next() {
            Some(Ok(entry)) => Ok(Some(entry)),
            _ => Ok(None),
        }
    }

    pub(super) async fn list_projection_entries(
        &self,
        category: Option<&MemoryCategory>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let mut results = Vec::new();

        let row_mapper = |row: &rusqlite::Row| -> rusqlite::Result<MemoryEntry> {
            Ok(MemoryEntry {
                id: row.get(0)?,
                key: row.get(1)?,
                content: row.get(2)?,
                category: Self::str_to_category(&row.get::<_, String>(3)?),
                timestamp: row.get(4)?,
                session_id: None,
                score: None,
            })
        };

        if let Some(cat) = category {
            let cat_str = Self::category_to_str(cat);
            let mut stmt = conn.prepare(
                "SELECT id, key, content, category, created_at FROM memories
                 WHERE category = ?1 ORDER BY updated_at DESC",
            )?;
            let rows = stmt.query_map(params![cat_str], row_mapper)?;
            for row in rows {
                results.push(row?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, key, content, category, created_at FROM memories
                 ORDER BY updated_at DESC",
            )?;
            let rows = stmt.query_map([], row_mapper)?;
            for row in rows {
                results.push(row?);
            }
        }

        Ok(results)
    }

    pub(super) async fn delete_projection_entry(&self, key: &str) -> anyhow::Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let affected = conn.execute("DELETE FROM memories WHERE key = ?1", params![key])?;
        Ok(affected > 0)
    }

    pub(super) async fn count_projection_entries(&self) -> anyhow::Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        Ok(count as usize)
    }

    pub(super) async fn health_check(&self) -> bool {
        self.conn
            .lock()
            .map(|c| c.execute_batch("SELECT 1").is_ok())
            .unwrap_or(false)
    }
}
