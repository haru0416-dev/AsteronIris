// Vector operations — cosine similarity, normalization, hybrid merge.

/// Cosine similarity between two vectors. Returns 0.0–1.0.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;

    for (x, y) in a.iter().zip(b.iter()) {
        let x = f64::from(*x);
        let y = f64::from(*y);
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if !denom.is_finite() || denom < f64::EPSILON {
        return 0.0;
    }

    let raw = dot / denom;
    if !raw.is_finite() {
        return 0.0;
    }

    #[allow(clippy::cast_possible_truncation)]
    let sim = raw.clamp(0.0, 1.0) as f32;
    sim
}

/// Serialize f32 vector to bytes (little-endian)
pub fn vec_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for &f in v {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    bytes
}

/// Deserialize bytes to f32 vector (little-endian)
pub fn bytes_to_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| {
            let arr: [u8; 4] = chunk.try_into().unwrap_or([0; 4]);
            f32::from_le_bytes(arr)
        })
        .collect()
}

/// A scored result for hybrid merging
#[derive(Debug, Clone)]
pub struct ScoredResult {
    pub id: String,
    pub vector_score: Option<f32>,
    pub keyword_score: Option<f32>,
    pub final_score: f32,
}

/// Hybrid merge: combine vector and keyword results with weighted fusion.
pub fn hybrid_merge(
    vector_results: &[(String, f32)],
    keyword_results: &[(String, f32)],
    vector_weight: f32,
    keyword_weight: f32,
    limit: usize,
) -> Vec<ScoredResult> {
    use std::collections::HashMap;

    let mut map: HashMap<String, (Option<f32>, Option<f32>)> = HashMap::new();

    for (id, score) in vector_results {
        map.entry(id.clone())
            .and_modify(|(vector_score, _)| *vector_score = Some(*score))
            .or_insert((Some(*score), None));
    }

    let max_kw = keyword_results
        .iter()
        .map(|(_, s)| *s)
        .fold(0.0_f32, f32::max);
    let max_kw = if max_kw < f32::EPSILON { 1.0 } else { max_kw };

    for (id, score) in keyword_results {
        let normalized = score / max_kw;
        map.entry(id.clone())
            .and_modify(|(_, keyword_score)| *keyword_score = Some(normalized))
            .or_insert((None, Some(normalized)));
    }

    let mut results: Vec<ScoredResult> = map
        .into_iter()
        .map(|(id, (vector_score, keyword_score))| {
            let vs = vector_score.unwrap_or(0.0);
            let ks = keyword_score.unwrap_or(0.0);
            ScoredResult {
                id,
                vector_score,
                keyword_score,
                final_score: vector_weight * vs + keyword_weight * ks,
            }
        })
        .collect();

    results.sort_by(|a, b| {
        b.final_score
            .partial_cmp(&a.final_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit);
    results
}

/// Reciprocal Rank Fusion: combine ranked lists using position-based scoring.
pub fn rrf_merge(
    vector_results: &[(String, f32)],
    keyword_results: &[(String, f32)],
    limit: usize,
) -> Vec<ScoredResult> {
    use std::collections::HashMap;

    const K: f32 = 60.0;

    let mut scores: HashMap<String, (f32, Option<f32>, Option<f32>)> = HashMap::new();

    for (rank, (id, score)) in vector_results.iter().enumerate() {
        let rank_1based = u16::try_from(rank.saturating_add(1)).unwrap_or(u16::MAX);
        let rrf_score = 1.0 / (K + f32::from(rank_1based));
        scores
            .entry(id.clone())
            .and_modify(|(total, vector_score, _)| {
                *total += rrf_score;
                *vector_score = Some(*score);
            })
            .or_insert((rrf_score, Some(*score), None));
    }

    for (rank, (id, score)) in keyword_results.iter().enumerate() {
        let rank_1based = u16::try_from(rank.saturating_add(1)).unwrap_or(u16::MAX);
        let rrf_score = 1.0 / (K + f32::from(rank_1based));
        scores
            .entry(id.clone())
            .and_modify(|(total, _, keyword_score)| {
                *total += rrf_score;
                *keyword_score = Some(*score);
            })
            .or_insert((rrf_score, None, Some(*score)));
    }

    let mut results: Vec<ScoredResult> = scores
        .into_iter()
        .map(|(id, (total, vector_score, keyword_score))| ScoredResult {
            id,
            vector_score,
            keyword_score,
            final_score: total,
        })
        .collect();

    results.sort_by(|a, b| {
        b.final_score
            .partial_cmp(&a.final_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit);
    results
}

pub fn final_score(
    vector_score: f32,
    bm25_score: f32,
    recency_score: f32,
    importance: f32,
    reliability: f32,
    contradiction_penalty: f32,
) -> f32 {
    let vector = vector_score.clamp(0.0, 1.0);
    let bm25 = bm25_score.clamp(0.0, 1.0);
    let recency = recency_score.clamp(0.0, 1.0);
    let imp = importance.clamp(0.0, 1.0);
    let rel = reliability.clamp(0.0, 1.0);
    let penalty = contradiction_penalty.max(0.0);
    0.35 * vector + 0.25 * bm25 + 0.20 * recency + 0.10 * imp + 0.10 * rel - penalty
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::approx_constant,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical_vectors() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 0.001);
    }

    #[test]
    fn cosine_orthogonal_vectors() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!(cosine_similarity(&a, &b).abs() < 0.001);
    }

    #[test]
    fn cosine_empty_returns_zero() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn vec_bytes_roundtrip() {
        let original = vec![1.0_f32, -2.5, 3.14, 0.0, f32::MAX];
        let bytes = vec_to_bytes(&original);
        let restored = bytes_to_vec(&bytes);
        assert_eq!(original, restored);
    }

    #[test]
    fn hybrid_merge_deduplicates() {
        let vec_results = vec![("a".into(), 0.9)];
        let kw_results = vec![("a".into(), 10.0)];
        let merged = hybrid_merge(&vec_results, &kw_results, 0.7, 0.3, 10);
        assert_eq!(merged.len(), 1);
        assert!(merged[0].vector_score.is_some());
        assert!(merged[0].keyword_score.is_some());
    }

    #[test]
    fn rrf_merge_basic_fusion() {
        let vector_results = vec![("a".into(), 0.95), ("b".into(), 0.90)];
        let keyword_results = vec![("b".into(), 3.0), ("a".into(), 2.5)];
        let merged = rrf_merge(&vector_results, &keyword_results, 10);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn final_score_matches_weights() {
        let score = final_score(1.0, 0.5, 1.0, 0.5, 0.8, 0.1);
        assert!((score - 0.705).abs() < 0.0001);
    }
}
