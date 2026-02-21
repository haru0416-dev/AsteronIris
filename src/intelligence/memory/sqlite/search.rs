use super::SqliteMemory;
use crate::intelligence::memory::vector;
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
