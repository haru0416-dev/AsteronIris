use super::SqliteMemory;
use crate::core::memory::vector;
use rusqlite::{Connection, params};

impl SqliteMemory {
    // Used by search_projection for FTS5/BM25 keyword search — projection layer currently dormant
    pub(super) fn fts5_search(
        conn: &Connection,
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

        let sql = "SELECT m.id, bm25(memories_fts) as score
                   FROM memories_fts f
                   JOIN memories m ON m.rowid = f.rowid
                   WHERE memories_fts MATCH ?1
                   ORDER BY score
                   LIMIT ?2";

        let mut stmt = conn.prepare_cached(sql)?;
        #[allow(clippy::cast_possible_wrap)]
        let limit_i64 = limit as i64;

        let rows = stmt.query_map(params![fts_query, limit_i64], |row| {
            let id: String = row.get(0)?;
            let score: f64 = row.get(1)?;
            // BM25 returns negative scores (lower = better), negate for ranking
            #[allow(clippy::cast_possible_truncation)]
            Ok((id, (-score) as f32))
        })?;

        let mut results = Vec::with_capacity(limit);
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    // Used by search_projection for cosine-similarity vector search — projection layer currently dormant
    pub(super) fn vector_search(
        conn: &Connection,
        query_embedding: &[f32],
        limit: usize,
    ) -> anyhow::Result<Vec<(String, f32)>> {
        let mut stmt =
            conn.prepare_cached("SELECT id, embedding FROM memories WHERE embedding IS NOT NULL")?;

        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((id, blob))
        })?;

        let mut scored: Vec<(String, f32)> = Vec::with_capacity(limit);
        for row in rows {
            let (id, blob) = row?;
            let emb = vector::bytes_to_vec(&blob);
            let sim = vector::cosine_similarity(query_embedding, &emb);
            if sim > 0.0 {
                scored.push((id, sim));
            }
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        SqliteMemory::init_schema(&conn).unwrap();
        conn
    }

    fn insert_test_memory(
        conn: &Connection,
        id: &str,
        key: &str,
        content: &str,
        embedding: Option<&[f32]>,
    ) {
        let now = chrono::Utc::now().to_rfc3339();
        let emb_blob = embedding.map(crate::core::memory::vector::vec_to_bytes);
        conn.execute(
            "INSERT INTO memories (id, key, content, created_at, updated_at, embedding) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, key, content, now, now, emb_blob],
        )
        .unwrap();
    }

    #[test]
    fn fts5_search_matching_query_returns_positive_scores() {
        let conn = fresh_db();
        insert_test_memory(
            &conn,
            "m1",
            "astronomy_note",
            "The galaxy has many bright stars",
            None,
        );
        insert_test_memory(
            &conn,
            "m2",
            "science_note",
            "A galaxy can contain black holes",
            None,
        );

        let results = SqliteMemory::fts5_search(&conn, "galaxy", 10).unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().all(|(_, score)| *score > 0.0));
    }

    #[test]
    fn fts5_search_empty_query_returns_empty_results() {
        let conn = fresh_db();
        insert_test_memory(&conn, "m1", "key", "content", None);

        let results = SqliteMemory::fts5_search(&conn, "", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn fts5_search_no_matches_returns_empty_results() {
        let conn = fresh_db();
        insert_test_memory(&conn, "m1", "alpha", "rust language", None);

        let results = SqliteMemory::fts5_search(&conn, "nonexistent_term", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn vector_search_matching_embeddings_are_sorted_by_similarity() {
        let conn = fresh_db();
        insert_test_memory(&conn, "closest", "k1", "high similarity", Some(&[1.0, 0.0]));
        insert_test_memory(&conn, "near", "k2", "medium similarity", Some(&[0.8, 0.2]));
        insert_test_memory(
            &conn,
            "orthogonal",
            "k3",
            "zero similarity",
            Some(&[0.0, 1.0]),
        );

        let results = SqliteMemory::vector_search(&conn, &[1.0, 0.0], 10).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "closest");
        assert_eq!(results[1].0, "near");
        assert!(results[0].1 > results[1].1);
    }

    #[test]
    fn vector_search_empty_table_returns_empty_results() {
        let conn = fresh_db();
        let results = SqliteMemory::vector_search(&conn, &[1.0, 0.0], 10).unwrap();
        assert!(results.is_empty());
    }
}
