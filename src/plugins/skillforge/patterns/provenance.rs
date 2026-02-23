use super::ReasonCode;

pub fn detect_provenance_reasons(
    commit_sha: Option<&str>,
    stored_content_hash: Option<&str>,
    computed_content_hash: Option<&str>,
) -> Vec<ReasonCode> {
    let mut reasons = Vec::new();

    match commit_sha {
        None => reasons.push(ReasonCode::MissingProvenance),
        Some(sha) => {
            if crate::plugins::skillforge::provenance::is_mutable_ref(sha) {
                reasons.push(ReasonCode::MutableRef);
            }
        }
    }

    if let (Some(stored), Some(computed)) = (stored_content_hash, computed_content_hash)
        && stored != computed
    {
        reasons.push(ReasonCode::HashMismatch);
    }

    reasons
}
