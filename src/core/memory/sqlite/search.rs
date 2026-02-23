use super::SqliteMemory;
use crate::core::memory::vector;
use rusqlite::{Connection, params};

impl SqliteMemory {
    /// FTS5 search scoped to a specific `entity_id`.
    pub(super) fn fts5_search_scoped(
        conn: &Connection,
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

        let sql = "SELECT ru.unit_id, bm25(retrieval_fts) as score
                   FROM retrieval_fts f
                   JOIN retrieval_units ru ON ru.rowid = f.rowid
                   WHERE retrieval_fts MATCH ?1
                     AND ru.entity_id = ?2
                     AND ru.visibility != 'secret'
                     AND ru.promotion_status IN ('promoted', 'candidate')
                   ORDER BY score
                   LIMIT ?3";

        let mut stmt = conn.prepare_cached(sql)?;
        #[allow(clippy::cast_possible_wrap)]
        let limit_i64 = limit as i64;

        let rows = stmt.query_map(params![fts_query, entity_id, limit_i64], |row| {
            let id: String = row.get(0)?;
            let score: f64 = row.get(1)?;
            #[allow(clippy::cast_possible_truncation)]
            Ok((id, (-score) as f32))
        })?;

        let mut results = Vec::with_capacity(limit);
        for row in rows {
            results.push(row?);
        }

        Ok(results)
    }

    /// Vector search scoped to a specific `entity_id`.
    pub(super) fn vector_search_scoped(
        conn: &Connection,
        entity_id: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> anyhow::Result<Vec<(String, f32)>> {
        let mut stmt = conn.prepare_cached(
            "SELECT unit_id, embedding FROM retrieval_units
             WHERE embedding IS NOT NULL AND entity_id = ?1
               AND visibility != 'secret'
               AND promotion_status IN ('promoted', 'candidate')",
        )?;

        let rows = stmt.query_map(params![entity_id], |row| {
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

    fn insert_test_retrieval_unit(
        conn: &Connection,
        id: &str,
        entity_id: &str,
        key: &str,
        content: &str,
        visibility: &str,
        promotion_status: &str,
        embedding: Option<&[f32]>,
    ) {
        let now = chrono::Utc::now().to_rfc3339();
        let emb_blob = embedding.map(crate::core::memory::vector::vec_to_bytes);
        conn.execute(
            "INSERT INTO retrieval_units (
                unit_id, entity_id, slot_key, content, visibility,
                promotion_status, created_at, updated_at, embedding
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                id,
                entity_id,
                key,
                content,
                visibility,
                promotion_status,
                now,
                now,
                emb_blob
            ],
        )
        .unwrap();
    }

    #[test]
    fn fts5_search_scoped_returns_only_target_entity() {
        let conn = fresh_db();
        insert_test_retrieval_unit(
            &conn,
            "e1",
            "entity:one",
            "slot.one",
            "galaxy report",
            "public",
            "promoted",
            None,
        );
        insert_test_retrieval_unit(
            &conn,
            "e2",
            "entity:two",
            "slot.two",
            "galaxy report",
            "public",
            "promoted",
            None,
        );

        let results = SqliteMemory::fts5_search_scoped(&conn, "entity:one", "galaxy", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "e1");
    }

    #[test]
    fn vector_search_scoped_returns_only_target_entity() {
        let conn = fresh_db();
        insert_test_retrieval_unit(
            &conn,
            "e1",
            "entity:one",
            "slot.one",
            "same",
            "public",
            "promoted",
            Some(&[1.0, 0.0]),
        );
        insert_test_retrieval_unit(
            &conn,
            "e2",
            "entity:two",
            "slot.two",
            "same",
            "public",
            "promoted",
            Some(&[1.0, 0.0]),
        );

        let results =
            SqliteMemory::vector_search_scoped(&conn, "entity:one", &[1.0, 0.0], 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "e1");
    }

    #[test]
    fn scoped_search_respects_visibility_and_promotion_status() {
        let conn = fresh_db();
        insert_test_retrieval_unit(
            &conn,
            "eligible",
            "entity:scope",
            "slot.ok",
            "trend signal spike",
            "public",
            "candidate",
            Some(&[1.0, 0.0]),
        );
        insert_test_retrieval_unit(
            &conn,
            "secret",
            "entity:scope",
            "slot.secret",
            "trend signal spike",
            "secret",
            "promoted",
            Some(&[1.0, 0.0]),
        );
        insert_test_retrieval_unit(
            &conn,
            "raw",
            "entity:scope",
            "slot.raw",
            "trend signal spike",
            "public",
            "raw",
            Some(&[1.0, 0.0]),
        );

        let fts = SqliteMemory::fts5_search_scoped(&conn, "entity:scope", "trend", 10).unwrap();
        assert_eq!(fts.len(), 1);
        assert_eq!(fts[0].0, "eligible");

        let vec =
            SqliteMemory::vector_search_scoped(&conn, "entity:scope", &[1.0, 0.0], 10).unwrap();
        assert_eq!(vec.len(), 1);
        assert_eq!(vec[0].0, "eligible");
    }

    #[test]
    fn fts5_search_scoped_matches_japanese_with_trigram() {
        let conn = fresh_db();
        insert_test_retrieval_unit(
            &conn,
            "jp1",
            "entity:jp",
            "profile.language",
            "テスト太郎の好きな言語はRustです",
            "public",
            "promoted",
            None,
        );

        let results =
            SqliteMemory::fts5_search_scoped(&conn, "entity:jp", "テスト太郎", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "jp1");
        assert!(results[0].1 > 0.0);
    }
}
