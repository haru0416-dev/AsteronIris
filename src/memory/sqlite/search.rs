use crate::memory::vector;
use anyhow::Context;
use sqlx::SqlitePool;

/// FTS5 search scoped to a specific `entity_id`.
///
/// Returns `(unit_id, bm25_score)` pairs sorted by relevance descending.
pub(super) async fn fts5_search_scoped(
    pool: &SqlitePool,
    entity_id: &str,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<(String, f32)>> {
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut fts_query = String::with_capacity(words.len() * 20);
    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            fts_query.push_str(" OR ");
        }
        fts_query.push('"');
        fts_query.push_str(w);
        fts_query.push('"');
    }

    if fts_query.is_empty() {
        return Ok(Vec::new());
    }

    #[allow(clippy::cast_possible_wrap)]
    let limit_i64 = limit as i64;

    let rows: Vec<(String, f64)> = sqlx::query_as(
        "SELECT ru.unit_id, bm25(retrieval_fts) as score
         FROM retrieval_fts f
         JOIN retrieval_units ru ON ru.rowid = f.rowid
         WHERE retrieval_fts MATCH ?1
           AND ru.entity_id = ?2
           AND ru.visibility != 'secret'
           AND ru.promotion_status IN ('promoted', 'candidate')
         ORDER BY score
         LIMIT ?3",
    )
    .bind(&fts_query)
    .bind(entity_id)
    .bind(limit_i64)
    .fetch_all(pool)
    .await
    .context("FTS5 search query")?;

    #[allow(clippy::cast_possible_truncation)]
    let results = rows
        .into_iter()
        .map(|(id, score)| (id, (-score) as f32))
        .collect();

    Ok(results)
}

/// Vector search (brute-force cosine similarity) scoped to a specific `entity_id`.
///
/// Returns `(unit_id, similarity)` pairs sorted by similarity descending.
pub(super) async fn vector_search_scoped(
    pool: &SqlitePool,
    entity_id: &str,
    query_embedding: &[f32],
    limit: usize,
) -> anyhow::Result<Vec<(String, f32)>> {
    let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT unit_id, embedding FROM retrieval_units
         WHERE embedding IS NOT NULL AND entity_id = ?1
           AND visibility != 'secret'
           AND promotion_status IN ('promoted', 'candidate')",
    )
    .bind(entity_id)
    .fetch_all(pool)
    .await
    .context("vector search query")?;

    let mut scored: Vec<(String, f32)> = Vec::with_capacity(limit);
    for (id, blob) in &rows {
        let emb = vector::bytes_to_vec(blob);
        let sim = vector::cosine_similarity(query_embedding, &emb);
        if sim > 0.0 {
            scored.push((id.clone(), sim));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    Ok(scored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::sqlite::schema;

    async fn fresh_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("open in-memory SQLite");
        schema::init_schema(&pool).await.expect("init schema");
        pool
    }

    async fn insert_test_retrieval_unit(
        pool: &SqlitePool,
        id: &str,
        entity_id: &str,
        key: &str,
        content: &str,
        visibility: &str,
        promotion_status: &str,
        embedding: Option<&[f32]>,
    ) {
        let now = chrono::Utc::now().to_rfc3339();
        let emb_blob = embedding.map(vector::vec_to_bytes);
        sqlx::query(
            "INSERT INTO retrieval_units (
                unit_id, entity_id, slot_key, content, visibility,
                promotion_status, created_at, updated_at, embedding
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(id)
        .bind(entity_id)
        .bind(key)
        .bind(content)
        .bind(visibility)
        .bind(promotion_status)
        .bind(&now)
        .bind(&now)
        .bind(emb_blob)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn fts5_search_scoped_returns_only_target_entity() {
        let pool = fresh_pool().await;
        insert_test_retrieval_unit(
            &pool,
            "e1",
            "entity:one",
            "slot.one",
            "galaxy report",
            "public",
            "promoted",
            None,
        )
        .await;
        insert_test_retrieval_unit(
            &pool,
            "e2",
            "entity:two",
            "slot.two",
            "galaxy report",
            "public",
            "promoted",
            None,
        )
        .await;

        let results = fts5_search_scoped(&pool, "entity:one", "galaxy", 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "e1");
    }

    #[tokio::test]
    async fn vector_search_scoped_returns_only_target_entity() {
        let pool = fresh_pool().await;
        insert_test_retrieval_unit(
            &pool,
            "e1",
            "entity:one",
            "slot.one",
            "same",
            "public",
            "promoted",
            Some(&[1.0, 0.0]),
        )
        .await;
        insert_test_retrieval_unit(
            &pool,
            "e2",
            "entity:two",
            "slot.two",
            "same",
            "public",
            "promoted",
            Some(&[1.0, 0.0]),
        )
        .await;

        let results = vector_search_scoped(&pool, "entity:one", &[1.0, 0.0], 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "e1");
    }

    #[tokio::test]
    async fn scoped_search_respects_visibility_and_promotion_status() {
        let pool = fresh_pool().await;
        insert_test_retrieval_unit(
            &pool,
            "eligible",
            "entity:scope",
            "slot.ok",
            "trend signal spike",
            "public",
            "candidate",
            Some(&[1.0, 0.0]),
        )
        .await;
        insert_test_retrieval_unit(
            &pool,
            "secret",
            "entity:scope",
            "slot.secret",
            "trend signal spike",
            "secret",
            "promoted",
            Some(&[1.0, 0.0]),
        )
        .await;
        insert_test_retrieval_unit(
            &pool,
            "raw",
            "entity:scope",
            "slot.raw",
            "trend signal spike",
            "public",
            "raw",
            Some(&[1.0, 0.0]),
        )
        .await;

        let fts = fts5_search_scoped(&pool, "entity:scope", "trend", 10)
            .await
            .unwrap();
        assert_eq!(fts.len(), 1);
        assert_eq!(fts[0].0, "eligible");

        let vec = vector_search_scoped(&pool, "entity:scope", &[1.0, 0.0], 10)
            .await
            .unwrap();
        assert_eq!(vec.len(), 1);
        assert_eq!(vec[0].0, "eligible");
    }

    #[tokio::test]
    async fn fts5_search_scoped_matches_japanese_with_trigram() {
        let pool = fresh_pool().await;
        insert_test_retrieval_unit(
            &pool,
            "jp1",
            "entity:jp",
            "profile.language",
            "\u{30c6}\u{30b9}\u{30c8}\u{592a}\u{90ce}\u{306e}\u{597d}\u{304d}\u{306a}\u{8a00}\u{8a9e}\u{306f}Rust\u{3067}\u{3059}",
            "public",
            "promoted",
            None,
        )
        .await;

        let results = fts5_search_scoped(
            &pool,
            "entity:jp",
            "\u{30c6}\u{30b9}\u{30c8}\u{592a}\u{90ce}",
            10,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "jp1");
        assert!(results[0].1 > 0.0);
    }
}
