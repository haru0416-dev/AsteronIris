use super::batch::{build_row_batch, parse_entries_and_score, parse_rows};
use super::{
    BackfillJob, EMBEDDING_STATUS_READY, LANCE_DISTANCE_COL, LANCE_SCORE_COL, LanceDbMemory,
    ProjectionEntry, StoredRow,
};

use anyhow::Context;
use arrow_array::RecordBatchIterator;
use futures_util::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use std::collections::HashMap;

#[allow(
    clippy::unused_self,
    clippy::unused_async,
    clippy::trivially_copy_pass_by_ref
)]
impl LanceDbMemory {
    pub(super) async fn get_row_by_key(&self, key: &str) -> anyhow::Result<Option<StoredRow>> {
        let table = self.inner.table().await?;
        let filter = Self::sql_eq("key", key);
        let mut stream = table
            .query()
            .only_if(filter)
            .limit(1)
            .select(Select::columns(&[
                "id",
                "key",
                "content",
                "category",
                "source",
                "confidence",
                "importance",
                "privacy_level",
                "occurred_at",
                "layer",
                "provenance_source_class",
                "provenance_reference",
                "provenance_evidence_uri",
                "created_at",
                "updated_at",
                "embedding_status",
            ]))
            .execute()
            .await
            .context("LanceDB get query failed")?;

        while let Some(batch) = stream.try_next().await? {
            if let Some(row) = parse_rows(&batch).into_iter().next() {
                return Ok(Some(row));
            }
        }
        Ok(None)
    }

    pub(super) async fn upsert_row(
        &self,
        row: &StoredRow,
        embedding: Option<&[f32]>,
    ) -> anyhow::Result<()> {
        let table = self.inner.table().await?;
        let batch = build_row_batch(self.inner.schema.clone(), row, embedding)?;

        let schema = batch.schema();
        let reader = RecordBatchIterator::new([Ok(batch)].into_iter(), schema);

        let mut merge_insert = table.merge_insert(&["key"]);
        merge_insert
            .when_matched_update_all(None)
            .when_not_matched_insert_all();
        merge_insert
            .execute(Box::new(reader))
            .await
            .context("LanceDB merge_insert failed")?;
        Ok(())
    }

    pub(super) fn enqueue_backfill(&self, key: &str) {
        let job = BackfillJob {
            key: key.to_string(),
        };
        if let Err(_e) = self.backfill_tx.try_send(job) {
            tracing::warn!("lancedb backfill queue full; dropping job");
        }
    }

    pub(super) async fn fts_search(
        &self,
        query: &str,
        limit: usize,
        entries: &mut HashMap<String, ProjectionEntry>,
    ) -> anyhow::Result<Vec<(String, f32)>> {
        use lancedb::index::scalar::FullTextSearchQuery;

        let table = self.inner.table().await?;
        let mut stream = table
            .query()
            .full_text_search(FullTextSearchQuery::new(query.to_string()))
            .limit(limit)
            .select(Select::columns(&[
                "id",
                "key",
                "content",
                "category",
                "source",
                "confidence",
                "importance",
                "privacy_level",
                "occurred_at",
                "created_at",
                LANCE_SCORE_COL,
            ]))
            .execute()
            .await
            .context("LanceDB full_text_search failed")?;

        let mut scored = Vec::new();
        while let Some(batch) = stream.try_next().await? {
            let (rows, row_scores) = parse_entries_and_score(&batch, LANCE_SCORE_COL);
            for (row, score) in rows.into_iter().zip(row_scores.into_iter()) {
                let id = row.id.clone();
                entries.insert(id.clone(), row);
                scored.push((id, score));
            }
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }

    pub(super) async fn vector_search(
        &self,
        query_embedding: &[f32],
        limit: usize,
        entries: &mut HashMap<String, ProjectionEntry>,
    ) -> anyhow::Result<Vec<(String, f32)>> {
        let table = self.inner.table().await?;
        let mut stream = table
            .query()
            .only_if(Self::sql_eq("embedding_status", EMBEDDING_STATUS_READY))
            .nearest_to(query_embedding)?
            .column("embedding")
            .distance_type(lancedb::DistanceType::Cosine)
            .limit(limit)
            .select(Select::columns(&[
                "id",
                "key",
                "content",
                "category",
                "source",
                "confidence",
                "importance",
                "privacy_level",
                "occurred_at",
                "created_at",
                LANCE_DISTANCE_COL,
            ]))
            .execute()
            .await
            .context("LanceDB vector search failed")?;

        let mut scored = Vec::new();
        while let Some(batch) = stream.try_next().await? {
            let (rows, dists) = parse_entries_and_score(&batch, LANCE_DISTANCE_COL);
            for (row, dist) in rows.into_iter().zip(dists.into_iter()) {
                let id = row.id.clone();
                entries.insert(id.clone(), row);
                let sim = (1.0 - dist).clamp(0.0, 1.0);
                scored.push((id, sim));
            }
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }
}
