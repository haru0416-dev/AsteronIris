use super::types::{UsageRecord, UsageSummary};
use anyhow::Result;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};
use std::path::Path;

/// Async usage tracking trait.
pub trait UsageTracker: Send + Sync {
    fn record(
        &self,
        record: &UsageRecord,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;

    fn summarize(
        &self,
        since: Option<&str>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<UsageSummary>> + Send + '_>>;
}

/// SQLite-backed usage tracker using sqlx async pool.
pub struct SqliteUsageTracker {
    pool: SqlitePool,
}

impl SqliteUsageTracker {
    pub async fn new(db_path: &Path) -> Result<Self> {
        let url = format!("sqlite://{}?mode=rwc", db_path.display());
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS usage_records (
                id TEXT PRIMARY KEY,
                session_id TEXT,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER,
                output_tokens INTEGER,
                estimated_cost_micros INTEGER,
                created_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }
}

impl UsageTracker for SqliteUsageTracker {
    fn record(
        &self,
        record: &UsageRecord,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = record.id.clone();
        let session_id = record.session_id.clone();
        let provider = record.provider.clone();
        let model = record.model.clone();
        let input_tokens = record.input_tokens.map(u64::cast_signed);
        let output_tokens = record.output_tokens.map(u64::cast_signed);
        let cost = record.estimated_cost_micros;
        let created_at = record.created_at.clone();

        Box::pin(async move {
            sqlx::query(
                "INSERT INTO usage_records (id, session_id, provider, model, input_tokens, output_tokens, estimated_cost_micros, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(id)
            .bind(session_id)
            .bind(provider)
            .bind(model)
            .bind(input_tokens)
            .bind(output_tokens)
            .bind(cost)
            .bind(created_at)
            .execute(&self.pool)
            .await?;
            Ok(())
        })
    }

    fn summarize(
        &self,
        since: Option<&str>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<UsageSummary>> + Send + '_>>
    {
        let since_owned = since.map(ToString::to_string);

        Box::pin(async move {
            let row = if let Some(since_ts) = since_owned {
                sqlx::query(
                    "SELECT
                        COALESCE(SUM(input_tokens), 0) as ti,
                        COALESCE(SUM(output_tokens), 0) as to_,
                        COALESCE(SUM(estimated_cost_micros), 0) as tc,
                        COUNT(*) as rc
                     FROM usage_records
                     WHERE created_at >= ?",
                )
                .bind(since_ts)
                .fetch_one(&self.pool)
                .await?
            } else {
                sqlx::query(
                    "SELECT
                        COALESCE(SUM(input_tokens), 0) as ti,
                        COALESCE(SUM(output_tokens), 0) as to_,
                        COALESCE(SUM(estimated_cost_micros), 0) as tc,
                        COUNT(*) as rc
                     FROM usage_records",
                )
                .fetch_one(&self.pool)
                .await?
            };

            Ok(UsageSummary {
                total_input_tokens: i64_to_u64(row.get::<i64, _>("ti")),
                total_output_tokens: i64_to_u64(row.get::<i64, _>("to_")),
                total_estimated_cost_micros: row.get::<i64, _>("tc"),
                record_count: i64_to_u64(row.get::<i64, _>("rc")),
            })
        })
    }
}

fn i64_to_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{SqliteUsageTracker, UsageTracker};
    use crate::runtime::usage::types::{ModelPricing, UsageRecord};
    use tempfile::NamedTempFile;

    fn sample_record(
        id: &str,
        created_at: &str,
        input: u64,
        output: u64,
        cost: i64,
    ) -> UsageRecord {
        UsageRecord {
            id: id.to_string(),
            session_id: Some("session-1".to_string()),
            provider: "openrouter".to_string(),
            model: "anthropic/claude-sonnet-4-20250514".to_string(),
            input_tokens: Some(input),
            output_tokens: Some(output),
            estimated_cost_micros: Some(cost),
            created_at: created_at.to_string(),
        }
    }

    #[tokio::test]
    async fn create_tracker_with_temp_file_succeeds() {
        let file = NamedTempFile::new().unwrap();
        let tracker = SqliteUsageTracker::new(file.path()).await;
        assert!(tracker.is_ok());
    }

    #[tokio::test]
    async fn record_usage_entry_and_summarize() {
        let file = NamedTempFile::new().unwrap();
        let tracker = SqliteUsageTracker::new(file.path()).await.unwrap();

        tracker
            .record(&sample_record(
                "id-1",
                "2026-02-20T10:00:00Z",
                120,
                80,
                1_000,
            ))
            .await
            .unwrap();

        let summary = tracker.summarize(None).await.unwrap();
        assert_eq!(summary.total_input_tokens, 120);
        assert_eq!(summary.total_output_tokens, 80);
        assert_eq!(summary.total_estimated_cost_micros, 1_000);
        assert_eq!(summary.record_count, 1);
    }

    #[tokio::test]
    async fn summarize_empty_database_returns_zeros() {
        let file = NamedTempFile::new().unwrap();
        let tracker = SqliteUsageTracker::new(file.path()).await.unwrap();

        let summary = tracker.summarize(None).await.unwrap();
        assert_eq!(summary.total_input_tokens, 0);
        assert_eq!(summary.total_output_tokens, 0);
        assert_eq!(summary.total_estimated_cost_micros, 0);
        assert_eq!(summary.record_count, 0);
    }

    #[tokio::test]
    async fn summarize_with_since_filter_respects_date_filter() {
        let file = NamedTempFile::new().unwrap();
        let tracker = SqliteUsageTracker::new(file.path()).await.unwrap();

        tracker
            .record(&sample_record("id-1", "2026-02-19T10:00:00Z", 100, 50, 900))
            .await
            .unwrap();
        tracker
            .record(&sample_record(
                "id-2",
                "2026-02-20T10:00:00Z",
                200,
                80,
                1_700,
            ))
            .await
            .unwrap();

        let summary = tracker
            .summarize(Some("2026-02-20T00:00:00Z"))
            .await
            .unwrap();
        assert_eq!(summary.total_input_tokens, 200);
        assert_eq!(summary.total_output_tokens, 80);
        assert_eq!(summary.total_estimated_cost_micros, 1_700);
        assert_eq!(summary.record_count, 1);
    }

    #[tokio::test]
    async fn multiple_records_aggregate_correctly() {
        let file = NamedTempFile::new().unwrap();
        let tracker = SqliteUsageTracker::new(file.path()).await.unwrap();

        tracker
            .record(&sample_record(
                "id-1",
                "2026-02-20T10:00:00Z",
                100,
                50,
                1_000,
            ))
            .await
            .unwrap();
        tracker
            .record(&sample_record(
                "id-2",
                "2026-02-20T11:00:00Z",
                150,
                75,
                1_500,
            ))
            .await
            .unwrap();
        tracker
            .record(&sample_record(
                "id-3",
                "2026-02-20T12:00:00Z",
                200,
                100,
                2_000,
            ))
            .await
            .unwrap();

        let summary = tracker.summarize(None).await.unwrap();
        assert_eq!(summary.total_input_tokens, 450);
        assert_eq!(summary.total_output_tokens, 225);
        assert_eq!(summary.total_estimated_cost_micros, 4_500);
        assert_eq!(summary.record_count, 3);
    }

    #[test]
    fn pricing_estimation_returns_expected_micros() {
        let pricing = ModelPricing {
            model_pattern: "test".to_string(),
            input_cost_per_million: 2.5,
            output_cost_per_million: 10.0,
        };
        let estimate = pricing.estimate_cost_micros(2_000_000, 1_000_000);
        assert_eq!(estimate, 15_000_000);
    }
}
