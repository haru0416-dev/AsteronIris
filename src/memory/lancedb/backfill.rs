use super::batch::{build_row_batch, parse_rows};
use super::{
    LanceDbInner, LanceDbMemory, EMBEDDING_STATUS_FAILED, EMBEDDING_STATUS_READY,
    MAX_BACKFILL_RETRIES, BASE_BACKOFF_MS, MAX_BACKOFF_MS,
};

use arrow_array::RecordBatchIterator;
use futures_util::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

pub(super) async fn run_backfill_worker(
    inner: Arc<LanceDbInner>,
    mut rx: mpsc::Receiver<super::BackfillJob>,
) {
    while let Some(job) = rx.recv().await {
        if let Err(e) = backfill_one(&inner, &job.key).await {
            let _ = e;
            tracing::debug!("lancedb backfill failed");
        }
    }
}

async fn backfill_one(inner: &LanceDbInner, key: &str) -> anyhow::Result<()> {
    let table = inner.table().await?;
    let filter = LanceDbMemory::sql_eq("key", key);

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
        .await?;

    let mut row = None;
    while let Some(batch) = stream.try_next().await? {
        if let Some(r) = parse_rows(&batch).into_iter().next() {
            row = Some(r);
            break;
        }
    }

    let Some(mut row) = row else {
        return Ok(());
    };

    if row.embedding_status == EMBEDDING_STATUS_READY {
        return Ok(());
    }

    let mut backoff_ms = BASE_BACKOFF_MS;
    for attempt in 0..=MAX_BACKFILL_RETRIES {
        let embed_res = inner.embedder.embed_one(&row.content).await;
        match embed_res {
            Ok(embedding) => {
                row.embedding_status = EMBEDDING_STATUS_READY.to_string();
                row.updated_at = chrono::Local::now().to_rfc3339();

                let batch = build_row_batch(inner.schema.clone(), &row, Some(&embedding))?;
                let schema = batch.schema();
                let reader = RecordBatchIterator::new([Ok(batch)].into_iter(), schema);
                let mut merge_insert = table.merge_insert(&["key"]);
                merge_insert
                    .when_matched_update_all(None)
                    .when_not_matched_insert_all();
                merge_insert.execute(Box::new(reader)).await?;
                return Ok(());
            }
            Err(e) => {
                if attempt >= MAX_BACKFILL_RETRIES {
                    let _ = e;
                    tracing::warn!("lancedb backfill exhausted retries for one item");
                    row.embedding_status = EMBEDDING_STATUS_FAILED.to_string();
                    row.updated_at = chrono::Local::now().to_rfc3339();
                    let batch = build_row_batch(inner.schema.clone(), &row, None)?;
                    let schema = batch.schema();
                    let reader = RecordBatchIterator::new([Ok(batch)].into_iter(), schema);
                    let mut merge_insert = table.merge_insert(&["key"]);
                    merge_insert
                        .when_matched_update_all(None)
                        .when_not_matched_insert_all();
                    merge_insert.execute(Box::new(reader)).await?;
                    return Ok(());
                }

                let jitter_ms = u64::from(chrono::Utc::now().timestamp_subsec_millis() % 250);
                sleep(Duration::from_millis(backoff_ms + jitter_ms)).await;
                backoff_ms = (backoff_ms.saturating_mul(2)).min(MAX_BACKOFF_MS);
            }
        }
    }

    Ok(())
}
