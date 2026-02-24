use super::{MemoryEventInput, MemoryProvenance, MemorySource};

pub(super) fn normalize_memory_event_input(input: &mut MemoryEventInput) -> anyhow::Result<()> {
    input.entity_id = normalize_entity_id(&input.entity_id)?;
    input.slot_key = normalize_slot_key(&input.slot_key)?;
    input.confidence = normalize_score(input.confidence, "memory_event_input.confidence")?;
    input.importance = normalize_score(input.importance, "memory_event_input.importance")?;
    if let Some(provenance) = &input.provenance {
        validate_provenance(input.source, provenance)?;
    }
    Ok(())
}

fn normalize_score(score: f64, field: &str) -> anyhow::Result<f64> {
    if !score.is_finite() {
        anyhow::bail!("{field} must be finite");
    }
    Ok(score.clamp(0.0, 1.0))
}

fn normalize_entity_id(raw: &str) -> anyhow::Result<String> {
    let normalized = normalize_identifier(raw, false);
    if normalized.is_empty() {
        anyhow::bail!("memory_event_input.entity_id must not be empty");
    }
    if normalized.len() > 128 {
        anyhow::bail!("memory_event_input.entity_id must be <= 128 chars");
    }
    Ok(normalized)
}

fn normalize_slot_key(raw: &str) -> anyhow::Result<String> {
    let normalized = normalize_identifier(raw, true);
    if normalized.is_empty() {
        anyhow::bail!("memory_event_input.slot_key must not be empty");
    }
    if normalized.len() > 256 {
        anyhow::bail!("memory_event_input.slot_key must be <= 256 chars");
    }
    if !is_valid_slot_key_pattern(&normalized) {
        anyhow::bail!("memory_event_input.slot_key must match taxonomy pattern");
    }
    Ok(normalized)
}

fn is_valid_slot_key_pattern(slot_key: &str) -> bool {
    let mut chars = slot_key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() {
        return false;
    }

    chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ':' | '/'))
}

fn normalize_identifier(raw: &str, allow_slash: bool) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_underscore = false;

    for ch in raw.trim().chars() {
        let allowed = ch.is_ascii_alphanumeric()
            || matches!(ch, '.' | '_' | '-' | ':')
            || (allow_slash && ch == '/');
        if allowed {
            out.push(ch);
            last_underscore = false;
        } else if !last_underscore {
            out.push('_');
            last_underscore = true;
        }
    }

    out.trim_matches('_').to_string()
}

fn validate_provenance(source: MemorySource, provenance: &MemoryProvenance) -> anyhow::Result<()> {
    if provenance.source_class != source {
        anyhow::bail!(
            "memory_event_input.provenance.source_class must match memory_event_input.source"
        );
    }

    if provenance.reference.trim().is_empty() {
        anyhow::bail!("memory_event_input.provenance.reference must not be empty");
    }

    if provenance.reference.len() > 256 {
        anyhow::bail!("memory_event_input.provenance.reference must be <= 256 chars");
    }

    if let Some(uri) = &provenance.evidence_uri
        && uri.trim().is_empty()
    {
        anyhow::bail!("memory_event_input.provenance.evidence_uri must not be empty");
    }

    Ok(())
}
