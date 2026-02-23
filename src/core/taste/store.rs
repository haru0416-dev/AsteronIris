#![allow(dead_code)]

use super::types::{Domain, PairComparison, TasteContext, Winner};
use anyhow::Context as _;
use rusqlite::{Connection, params};
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

/// Rating for an item based on preference comparisons.
pub struct ItemRating {
    pub item_id: String,
    pub domain: Domain,
    pub rating: f64,
    pub n_comparisons: u32,
    pub updated_at: String,
}

pub(crate) trait TasteStore: Send + Sync {
    fn save_comparison<'a>(
        &'a self,
        comparison: &'a PairComparison,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>;

    fn get_comparisons_for_item<'a>(
        &'a self,
        item_id: &'a str,
        domain: &'a Domain,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<PairComparison>>> + Send + 'a>>;

    fn get_rating<'a>(
        &'a self,
        item_id: &'a str,
        domain: &'a Domain,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Option<ItemRating>>> + Send + 'a>>;

    fn update_rating(
        &self,
        rating: ItemRating,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>>;

    fn get_all_ratings<'a>(
        &'a self,
        domain: &'a Domain,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<ItemRating>>> + Send + 'a>>;
}

pub(crate) struct SqliteTasteStore {
    conn: Mutex<Connection>,
}

impl SqliteTasteStore {
    pub(crate) fn new(conn: Connection) -> anyhow::Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS taste_comparisons (
                id TEXT PRIMARY KEY,
                domain TEXT NOT NULL,
                left_id TEXT NOT NULL,
                right_id TEXT NOT NULL,
                winner TEXT NOT NULL,
                rationale TEXT,
                context_json TEXT,
                created_at_ms INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS taste_ratings (
                item_id TEXT NOT NULL,
                domain TEXT NOT NULL,
                rating REAL NOT NULL,
                n_comparisons INTEGER NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY(item_id, domain)
            );",
        )
        .context("create taste tables")?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

impl TasteStore for SqliteTasteStore {
    fn save_comparison<'a>(
        &'a self,
        comparison: &'a PairComparison,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {e}"))?;
            let id = uuid::Uuid::new_v4().to_string();
            let domain = comparison.domain.to_string();
            let winner = serde_json::to_value(&comparison.winner)?
                .as_str()
                .map(String::from)
                .unwrap_or_default();
            let context_json = serde_json::to_string(&comparison.ctx)?;

            #[allow(clippy::cast_possible_wrap)]
            let created_at_ms = comparison.created_at_ms as i64;

            conn.execute(
                "INSERT INTO taste_comparisons \
                 (id, domain, left_id, right_id, winner, rationale, context_json, created_at_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    domain,
                    comparison.left_id,
                    comparison.right_id,
                    winner,
                    comparison.rationale,
                    context_json,
                    created_at_ms,
                ],
            )
            .context("insert taste comparison")?;

            Ok(())
        })
    }

    fn get_comparisons_for_item<'a>(
        &'a self,
        item_id: &'a str,
        domain: &'a Domain,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<PairComparison>>> + Send + 'a>> {
        Box::pin(async move {
            let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {e}"))?;
            let domain_str = domain.to_string();

            let mut stmt = conn
                .prepare(
                    "SELECT domain, left_id, right_id, winner, rationale, context_json, created_at_ms \
                     FROM taste_comparisons \
                     WHERE (left_id = ?1 OR right_id = ?1) AND domain = ?2",
                )
                .context("prepare get_comparisons_for_item")?;

            let rows = stmt
                .query_map(params![item_id, domain_str], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, i64>(6)?,
                    ))
                })
                .context("query comparisons")?;

            let mut comparisons = Vec::new();
            for row in rows {
                let (domain_s, left_id, right_id, winner_s, rationale, ctx_json, created_ms) =
                    row.context("read comparison row")?;

                let domain: Domain = serde_json::from_value(serde_json::Value::String(domain_s))
                    .context("deserialize domain")?;
                let winner: Winner = serde_json::from_value(serde_json::Value::String(winner_s))
                    .context("deserialize winner")?;
                let ctx: TasteContext = ctx_json
                    .as_deref()
                    .map(serde_json::from_str)
                    .transpose()
                    .context("deserialize context")?
                    .unwrap_or_default();

                #[allow(clippy::cast_sign_loss)]
                comparisons.push(PairComparison {
                    domain,
                    ctx,
                    left_id,
                    right_id,
                    winner,
                    rationale,
                    created_at_ms: created_ms as u64,
                });
            }

            Ok(comparisons)
        })
    }

    fn get_rating<'a>(
        &'a self,
        item_id: &'a str,
        domain: &'a Domain,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Option<ItemRating>>> + Send + 'a>> {
        Box::pin(async move {
            let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {e}"))?;
            let domain_str = domain.to_string();

            let mut stmt = conn
                .prepare(
                    "SELECT item_id, domain, rating, n_comparisons, updated_at \
                     FROM taste_ratings \
                     WHERE item_id = ?1 AND domain = ?2",
                )
                .context("prepare get_rating")?;

            let result = stmt.query_row(params![item_id, domain_str], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            });

            match result {
                Ok((id, dom_s, rating, n_comp, updated)) => {
                    let domain: Domain = serde_json::from_value(serde_json::Value::String(dom_s))
                        .context("deserialize domain")?;
                    Ok(Some(ItemRating {
                        item_id: id,
                        domain,
                        rating,
                        n_comparisons: u32::try_from(n_comp).context("n_comparisons overflow")?,
                        updated_at: updated,
                    }))
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e).context("query rating"),
            }
        })
    }

    fn update_rating(
        &self,
        rating: ItemRating,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
        Box::pin(async move {
            let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {e}"))?;
            let domain_str = rating.domain.to_string();

            conn.execute(
                "INSERT OR REPLACE INTO taste_ratings \
                 (item_id, domain, rating, n_comparisons, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    rating.item_id,
                    domain_str,
                    rating.rating,
                    i64::from(rating.n_comparisons),
                    rating.updated_at,
                ],
            )
            .context("upsert taste rating")?;

            Ok(())
        })
    }

    fn get_all_ratings<'a>(
        &'a self,
        domain: &'a Domain,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<ItemRating>>> + Send + 'a>> {
        Box::pin(async move {
            let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {e}"))?;
            let domain_str = domain.to_string();

            let mut stmt = conn
                .prepare(
                    "SELECT item_id, domain, rating, n_comparisons, updated_at \
                     FROM taste_ratings \
                     WHERE domain = ?1",
                )
                .context("prepare get_all_ratings")?;

            let rows = stmt
                .query_map(params![domain_str], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, f64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                })
                .context("query all ratings")?;

            let mut ratings = Vec::new();
            for row in rows {
                let (item_id, dom_s, rating, n_comp, updated) = row.context("read rating row")?;
                let domain: Domain = serde_json::from_value(serde_json::Value::String(dom_s))
                    .context("deserialize domain")?;
                ratings.push(ItemRating {
                    item_id,
                    domain,
                    rating,
                    n_comparisons: u32::try_from(n_comp).context("n_comparisons overflow")?,
                    updated_at: updated,
                });
            }

            Ok(ratings)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{Domain, PairComparison, TasteContext, Winner};
    use super::*;
    use rusqlite::Connection;

    fn fresh_store() -> SqliteTasteStore {
        let conn = Connection::open_in_memory().unwrap();
        SqliteTasteStore::new(conn).unwrap()
    }

    fn sample_comparison(domain: Domain, left: &str, right: &str) -> PairComparison {
        PairComparison {
            domain,
            ctx: TasteContext::default(),
            left_id: left.into(),
            right_id: right.into(),
            winner: Winner::Left,
            rationale: Some("better".into()),
            created_at_ms: 1000,
        }
    }

    #[tokio::test]
    async fn append_only_returns_all_comparisons() {
        let store = fresh_store();
        let c1 = sample_comparison(Domain::Text, "a", "b");
        let c2 = sample_comparison(Domain::Text, "a", "c");

        store.save_comparison(&c1).await.unwrap();
        store.save_comparison(&c2).await.unwrap();

        let results = store
            .get_comparisons_for_item("a", &Domain::Text)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn rating_upsert_keeps_single_row() {
        let store = fresh_store();

        store
            .update_rating(ItemRating {
                item_id: "x".into(),
                domain: Domain::Text,
                rating: 1500.0,
                n_comparisons: 5,
                updated_at: "2025-01-01".into(),
            })
            .await
            .unwrap();

        store
            .update_rating(ItemRating {
                item_id: "x".into(),
                domain: Domain::Text,
                rating: 1600.0,
                n_comparisons: 10,
                updated_at: "2025-01-02".into(),
            })
            .await
            .unwrap();

        let all = store.get_all_ratings(&Domain::Text).await.unwrap();
        assert_eq!(all.len(), 1);
        assert!((all[0].rating - 1600.0).abs() < f64::EPSILON);
        assert_eq!(all[0].n_comparisons, 10);
    }

    #[tokio::test]
    async fn domain_scoping_isolates_ratings() {
        let store = fresh_store();

        store
            .update_rating(ItemRating {
                item_id: "y".into(),
                domain: Domain::Text,
                rating: 1500.0,
                n_comparisons: 3,
                updated_at: "2025-01-01".into(),
            })
            .await
            .unwrap();

        store
            .update_rating(ItemRating {
                item_id: "z".into(),
                domain: Domain::Ui,
                rating: 1400.0,
                n_comparisons: 2,
                updated_at: "2025-01-01".into(),
            })
            .await
            .unwrap();

        let text_ratings = store.get_all_ratings(&Domain::Text).await.unwrap();
        assert_eq!(text_ratings.len(), 1);
        assert_eq!(text_ratings[0].item_id, "y");

        let ui_ratings = store.get_all_ratings(&Domain::Ui).await.unwrap();
        assert_eq!(ui_ratings.len(), 1);
        assert_eq!(ui_ratings[0].item_id, "z");
    }
}
