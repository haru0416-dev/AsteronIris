use super::types::{UsageRecord, UsageSummary};
use anyhow::Result;
use rusqlite::{Connection, Error as SqlError, params, types::Type};
use std::path::Path;

pub trait UsageTracker {
    fn record(&self, record: &UsageRecord) -> Result<()>;
    fn summarize(&self, since: Option<&str>) -> Result<UsageSummary>;
}

pub struct SqliteUsageTracker {
    conn: Connection,
}

impl SqliteUsageTracker {
    pub fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS usage_records (
                id TEXT PRIMARY KEY,
                session_id TEXT,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER,
                output_tokens INTEGER,
                estimated_cost_micros INTEGER,
                created_at TEXT NOT NULL
            );",
        )?;
        Ok(Self { conn })
    }
}

impl UsageTracker for SqliteUsageTracker {
    fn record(&self, record: &UsageRecord) -> Result<()> {
        let input_tokens = record.input_tokens.map(i64::try_from).transpose()?;
        let output_tokens = record.output_tokens.map(i64::try_from).transpose()?;

        self.conn.execute(
            "INSERT INTO usage_records (id, session_id, provider, model, input_tokens, output_tokens, estimated_cost_micros, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                record.id,
                record.session_id,
                record.provider,
                record.model,
                input_tokens,
                output_tokens,
                record.estimated_cost_micros,
                record.created_at
            ],
        )?;
        Ok(())
    }

    fn summarize(&self, since: Option<&str>) -> Result<UsageSummary> {
        let summary = if let Some(since_ts) = since {
            let mut stmt = self.conn.prepare_cached(
                "SELECT
                    COALESCE(SUM(input_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(estimated_cost_micros), 0),
                    COUNT(*)
                 FROM usage_records
                 WHERE created_at >= ?1",
            )?;
            stmt.query_row([since_ts], |row| {
                let total_input_tokens = i64_to_u64(row.get::<_, i64>(0)?, 0)?;
                let total_output_tokens = i64_to_u64(row.get::<_, i64>(1)?, 1)?;
                let record_count = i64_to_u64(row.get::<_, i64>(3)?, 3)?;
                Ok(UsageSummary {
                    total_input_tokens,
                    total_output_tokens,
                    total_estimated_cost_micros: row.get(2)?,
                    record_count,
                })
            })?
        } else {
            let mut stmt = self.conn.prepare_cached(
                "SELECT
                    COALESCE(SUM(input_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(estimated_cost_micros), 0),
                    COUNT(*)
                 FROM usage_records",
            )?;
            stmt.query_row([], |row| {
                let total_input_tokens = i64_to_u64(row.get::<_, i64>(0)?, 0)?;
                let total_output_tokens = i64_to_u64(row.get::<_, i64>(1)?, 1)?;
                let record_count = i64_to_u64(row.get::<_, i64>(3)?, 3)?;
                Ok(UsageSummary {
                    total_input_tokens,
                    total_output_tokens,
                    total_estimated_cost_micros: row.get(2)?,
                    record_count,
                })
            })?
        };

        Ok(summary)
    }
}

fn i64_to_u64(value: i64, column_index: usize) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|error| {
        SqlError::FromSqlConversionFailure(column_index, Type::Integer, Box::new(error))
    })
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

    #[test]
    fn create_tracker_with_temp_file_succeeds() {
        let file = NamedTempFile::new().unwrap();
        let tracker = SqliteUsageTracker::new(file.path());
        assert!(tracker.is_ok());
    }

    #[test]
    fn record_usage_entry_and_summarize() {
        let file = NamedTempFile::new().unwrap();
        let tracker = SqliteUsageTracker::new(file.path()).unwrap();

        tracker
            .record(&sample_record(
                "id-1",
                "2026-02-20T10:00:00Z",
                120,
                80,
                1_000,
            ))
            .unwrap();

        let summary = tracker.summarize(None).unwrap();
        assert_eq!(summary.total_input_tokens, 120);
        assert_eq!(summary.total_output_tokens, 80);
        assert_eq!(summary.total_estimated_cost_micros, 1_000);
        assert_eq!(summary.record_count, 1);
    }

    #[test]
    fn summarize_empty_database_returns_zeros() {
        let file = NamedTempFile::new().unwrap();
        let tracker = SqliteUsageTracker::new(file.path()).unwrap();

        let summary = tracker.summarize(None).unwrap();
        assert_eq!(summary.total_input_tokens, 0);
        assert_eq!(summary.total_output_tokens, 0);
        assert_eq!(summary.total_estimated_cost_micros, 0);
        assert_eq!(summary.record_count, 0);
    }

    #[test]
    fn summarize_with_since_filter_respects_date_filter() {
        let file = NamedTempFile::new().unwrap();
        let tracker = SqliteUsageTracker::new(file.path()).unwrap();

        tracker
            .record(&sample_record("id-1", "2026-02-19T10:00:00Z", 100, 50, 900))
            .unwrap();
        tracker
            .record(&sample_record(
                "id-2",
                "2026-02-20T10:00:00Z",
                200,
                80,
                1_700,
            ))
            .unwrap();

        let summary = tracker.summarize(Some("2026-02-20T00:00:00Z")).unwrap();
        assert_eq!(summary.total_input_tokens, 200);
        assert_eq!(summary.total_output_tokens, 80);
        assert_eq!(summary.total_estimated_cost_micros, 1_700);
        assert_eq!(summary.record_count, 1);
    }

    #[test]
    fn multiple_records_aggregate_correctly() {
        let file = NamedTempFile::new().unwrap();
        let tracker = SqliteUsageTracker::new(file.path()).unwrap();

        tracker
            .record(&sample_record(
                "id-1",
                "2026-02-20T10:00:00Z",
                100,
                50,
                1_000,
            ))
            .unwrap();
        tracker
            .record(&sample_record(
                "id-2",
                "2026-02-20T11:00:00Z",
                150,
                75,
                1_500,
            ))
            .unwrap();
        tracker
            .record(&sample_record(
                "id-3",
                "2026-02-20T12:00:00Z",
                200,
                100,
                2_000,
            ))
            .unwrap();

        let summary = tracker.summarize(None).unwrap();
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
