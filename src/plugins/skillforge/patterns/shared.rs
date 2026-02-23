pub(super) const ENTROPY_THRESHOLD: f64 = 4.5;

pub(super) fn contains_word(haystack: &str, word: &str) -> bool {
    for (index, _) in haystack.match_indices(word) {
        let before_ok = index == 0 || !haystack.as_bytes()[index - 1].is_ascii_alphanumeric();
        let after = index + word.len();
        let after_ok =
            after >= haystack.len() || !haystack.as_bytes()[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

pub(super) fn levenshtein(a: &str, b: &str) -> usize {
    let a_len = a.len();
    let b_len = b.len();
    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0_usize; b_len + 1];

    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

pub(super) fn has_high_entropy_strings(code: &str) -> bool {
    for segment in code.split('"') {
        if segment.len() >= 20 && shannon_entropy(segment) > ENTROPY_THRESHOLD {
            return true;
        }
    }
    false
}

#[allow(clippy::cast_precision_loss)]
pub(super) fn shannon_entropy(value: &str) -> f64 {
    if value.is_empty() {
        return 0.0;
    }

    let len = value.len() as f64;
    let mut freq = [0_u32; 256];
    for &byte in value.as_bytes() {
        freq[byte as usize] += 1;
    }

    let mut entropy = 0.0_f64;
    for &count in &freq {
        if count > 0 {
            let probability = f64::from(count) / len;
            entropy -= probability * probability.log2();
        }
    }

    entropy
}
