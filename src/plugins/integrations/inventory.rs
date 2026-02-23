use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::config::Config;
use crate::plugins::integrations::IntegrationEntry;

const REGISTRY_SOURCE: &str = include_str!("registry.rs");
const SKILLFORGE_SOURCE: &str = include_str!("../skillforge/mod.rs");
const SCOPE_LOCK_BASELINE: &str = include_str!("inventory_scope_lock.json");
const CAPABILITY_MATRIX_SOURCE: &str = include_str!("integration_capability_matrix.json");

const INTEGRATION_SOURCE: &str = "integrations";
const SKILLFORGE_SOURCE_NAME: &str = "skillforge";

const INTEGRATION_PRIORITY: u8 = 1;
const SKILLFORGE_PRIORITY: u8 = 2;

const INTEGRATION_HEADER: &str = "coming_soon_count";
const SKILLFORGE_HEADER: &str = "skillforge_unimplemented";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScopeLockInventory {
    pub coming_soon_count: usize,
    pub skillforge_unimplemented: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnimplementedInventoryEntry {
    pub source: String,
    pub category: String,
    pub status: String,
    pub priority: u8,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegrationCapabilityMatrix {
    pub schema_version: String,
    pub source_file: String,
    pub entries: Vec<IntegrationCapabilityMatrixEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegrationCapabilityMatrixEntry {
    pub name: String,
    pub implemented: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationCapabilityDrift {
    pub name: String,
    pub kind: String,
    pub status: String,
    pub expectation: String,
}

impl ScopeLockInventory {
    pub fn to_json_pretty(&self) -> String {
        // Derive(Serialize) structs with only primitive/string fields cannot fail.
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

impl IntegrationCapabilityMatrix {
    pub fn to_json_pretty(&self) -> String {
        // Derive(Serialize) structs with only primitive/string fields cannot fail.
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

pub fn load_integration_capability_matrix() -> Result<IntegrationCapabilityMatrix> {
    parse_integration_capability_matrix(CAPABILITY_MATRIX_SOURCE)
}

pub fn parse_integration_capability_matrix(
    matrix_source: &str,
) -> Result<IntegrationCapabilityMatrix> {
    serde_json::from_str(matrix_source)
        .map_err(|error| anyhow!("invalid integration capability matrix artifact: {error}"))
}

pub fn validate_integration_status_against_matrix(
    matrix: &IntegrationCapabilityMatrix,
    entries: &[IntegrationEntry],
    config: &Config,
) -> std::result::Result<(), Vec<IntegrationCapabilityDrift>> {
    let normalized = normalize_capability_matrix_entries(matrix.entries.clone());
    let mut drifts = Vec::new();

    let mut by_name: HashMap<&str, bool> = HashMap::new();
    for entry in &normalized {
        by_name.insert(entry.name.as_str(), entry.implemented);
    }

    let registry_names: HashSet<&str> = entries.iter().map(|entry| entry.name).collect();

    for name in by_name.keys() {
        if !registry_names.contains(name) {
            drifts.push(IntegrationCapabilityDrift {
                name: (*name).to_string(),
                kind: "stale_matrix_entry".to_string(),
                status: "unknown".to_string(),
                expectation: "remove or rename artifact entry".to_string(),
            });
        }
    }

    for entry in entries {
        let status = (entry.status_fn)(config);
        let matrix_implemented = by_name.get(entry.name).copied().unwrap_or(false);

        match (status, matrix_implemented) {
            (crate::plugins::integrations::IntegrationStatus::ComingSoon, true) => {
                drifts.push(IntegrationCapabilityDrift {
                    name: entry.name.to_string(),
                    kind: "unbacked_status".to_string(),
                    status: "ComingSoon".to_string(),
                    expectation: "not implemented in matrix".to_string(),
                });
            }
            (
                crate::plugins::integrations::IntegrationStatus::Active
                | crate::plugins::integrations::IntegrationStatus::Available,
                false,
            ) => {
                drifts.push(IntegrationCapabilityDrift {
                    name: entry.name.to_string(),
                    kind: "unsupported_status_claim".to_string(),
                    status: format!("{status:?}"),
                    expectation: "listed as implemented in matrix".to_string(),
                });
            }
            _ => {}
        }
    }

    if drifts.is_empty() {
        Ok(())
    } else {
        drifts.sort_by(|a, b| a.name.cmp(&b.name).then(a.kind.cmp(&b.kind)));
        Err(drifts)
    }
}

fn normalize_capability_matrix_entries(
    mut entries: Vec<IntegrationCapabilityMatrixEntry>,
) -> Vec<IntegrationCapabilityMatrixEntry> {
    entries.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.implemented.cmp(&right.implemented).reverse())
    });
    entries.dedup_by_key(|entry| entry.name.clone());
    entries
}

pub fn build_scope_lock_inventory() -> Result<ScopeLockInventory> {
    build_scope_lock_inventory_from_sources(REGISTRY_SOURCE, SKILLFORGE_SOURCE)
}

pub fn load_scope_lock_baseline_inventory() -> Result<ScopeLockInventory> {
    serde_json::from_str(SCOPE_LOCK_BASELINE)
        .map_err(|error| anyhow!("invalid scope-lock baseline artifact: {error}"))
}

pub fn normalize_unimplemented_inventory() -> Result<Vec<UnimplementedInventoryEntry>> {
    normalize_unimplemented_inventory_from_sources(REGISTRY_SOURCE, SKILLFORGE_SOURCE)
}

pub fn normalize_unimplemented_inventory_from_sources(
    registry_source: &str,
    skillforge_source: &str,
) -> Result<Vec<UnimplementedInventoryEntry>> {
    let mut entries = Vec::new();

    entries.extend(parse_registry_coming_soon_entries(registry_source)?);
    entries.extend(parse_skillforge_unimplemented_entries(skillforge_source)?);

    Ok(normalize_and_deduplicate(entries))
}

pub fn build_scope_lock_inventory_from_sources(
    registry_source: &str,
    skillforge_source: &str,
) -> Result<ScopeLockInventory> {
    let normalized =
        normalize_unimplemented_inventory_from_sources(registry_source, skillforge_source)?;

    let coming_soon_count = normalized
        .iter()
        .filter(|entry| entry.source == INTEGRATION_SOURCE && entry.status == "ComingSoon")
        .count();

    let skillforge_unimplemented = normalized
        .into_iter()
        .filter(|entry| entry.source == SKILLFORGE_SOURCE_NAME)
        .map(|entry| entry.name)
        .collect();

    Ok(ScopeLockInventory {
        coming_soon_count,
        skillforge_unimplemented,
    })
}

pub fn validate_inventory_against_sources(
    expected: &ScopeLockInventory,
    registry_source: &str,
    skillforge_source: &str,
) -> std::result::Result<(), String> {
    let actual = build_scope_lock_inventory_from_sources(registry_source, skillforge_source)
        .map_err(|error| format!("failed to parse inventory sources: {error}"))?;

    let mut drifts = Vec::new();

    if expected.coming_soon_count != actual.coming_soon_count {
        drifts.push(format!(
            "{} mismatch: expected={}, actual={}",
            INTEGRATION_HEADER, expected.coming_soon_count, actual.coming_soon_count
        ));
    }

    let mut expected_skillforge_unimplemented = expected.skillforge_unimplemented.clone();
    expected_skillforge_unimplemented.sort_unstable();

    let mut actual_skillforge_unimplemented = actual.skillforge_unimplemented.clone();
    actual_skillforge_unimplemented.sort_unstable();

    if expected_skillforge_unimplemented != actual_skillforge_unimplemented {
        drifts.push(format!(
            "{SKILLFORGE_HEADER} mismatch: expected={expected_skillforge_unimplemented:?}, actual={actual_skillforge_unimplemented:?}"
        ));
    }

    if drifts.is_empty() {
        Ok(())
    } else {
        Err(drifts.join("; "))
    }
}

pub fn parse_registry_coming_soon_count(source: &str) -> Result<usize> {
    let scope = source
        .split("#[cfg(test)]")
        .next()
        .ok_or_else(|| anyhow!("registry source is empty"))?;

    Ok(scope.matches("IntegrationStatus::ComingSoon").count())
}

pub fn parse_skillforge_unimplemented_sources(source: &str) -> Result<Vec<String>> {
    parse_skillforge_unimplemented_sources_with_validation(source)
}

pub fn parse_registry_coming_soon_entries(
    source: &str,
) -> Result<Vec<UnimplementedInventoryEntry>> {
    let scope = source
        .split("#[cfg(test)]")
        .next()
        .ok_or_else(|| anyhow!("registry source is empty"))?;

    let mut entries = Vec::new();
    let mut in_entry = false;
    let mut name = None;
    let mut category = None;
    let mut is_coming_soon = false;

    for line in scope.lines() {
        let trimmed = line.trim();

        if trimmed == "IntegrationEntry {" {
            in_entry = true;
            name = None;
            category = None;
            is_coming_soon = false;
            continue;
        }

        if !in_entry {
            continue;
        }

        if let Some(parsed_name) = parse_integration_field_string(trimmed, "name:") {
            name = Some(parsed_name.to_string());
        }

        if let Some(parsed_category) = parse_integration_category(trimmed)? {
            category = Some(parsed_category);
        }

        if trimmed.contains("IntegrationStatus::ComingSoon") {
            is_coming_soon = true;
        }

        if trimmed == "}," {
            if is_coming_soon {
                if let (Some(name), Some(category)) = (name.take(), category.take()) {
                    entries.push(UnimplementedInventoryEntry {
                        source: INTEGRATION_SOURCE.to_string(),
                        category,
                        status: "ComingSoon".to_string(),
                        priority: INTEGRATION_PRIORITY,
                        name,
                    });
                } else {
                    return Err(anyhow!(
                        "missing integration name/category in registry parser"
                    ));
                }
            }

            in_entry = false;
        }
    }

    Ok(entries)
}

fn parse_integration_field_string<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    if !line.starts_with(key) {
        return None;
    }

    let start = line.find('"')?;
    let end = line[start + 1..].find('"')?;
    Some(&line[start + 1..start + 1 + end])
}

fn parse_integration_category(line: &str) -> Result<Option<String>> {
    if !line.starts_with("category:") {
        return Ok(None);
    }

    let Some(raw_category) = line.split("IntegrationCategory::").nth(1) else {
        return Ok(None);
    };

    let variant = raw_category
        .split([',', ' ', '\t', '{', '}'])
        .next()
        .unwrap_or_default();

    Ok(Some(match variant {
        "Chat" => "Chat Providers".to_string(),
        "AiModel" => "AI Models".to_string(),
        "Productivity" => "Productivity".to_string(),
        "MusicAudio" => "Music & Audio".to_string(),
        "SmartHome" => "Smart Home".to_string(),
        "ToolsAutomation" => "Tools & Automation".to_string(),
        "MediaCreative" => "Media & Creative".to_string(),
        "Social" => "Social".to_string(),
        "Platform" => "Platforms".to_string(),
        _ => return Err(anyhow!("unknown integration category '{variant}'")),
    }))
}

fn parse_skillforge_unimplemented_entries(
    source: &str,
) -> Result<Vec<UnimplementedInventoryEntry>> {
    let sources = parse_skillforge_unimplemented_sources_with_validation(source)?;

    Ok(sources
        .into_iter()
        .map(|name| UnimplementedInventoryEntry {
            source: SKILLFORGE_SOURCE_NAME.to_string(),
            category: "Sources".to_string(),
            status: "Unimplemented".to_string(),
            priority: SKILLFORGE_PRIORITY,
            name,
        })
        .collect())
}

fn parse_skillforge_unimplemented_sources_with_validation(source: &str) -> Result<Vec<String>> {
    let scope = source
        .split("#[cfg(test)]")
        .next()
        .ok_or_else(|| anyhow!("skillforge source is empty"))?;

    let unimplemented = collect_skillforge_unimplemented_sources(scope);
    for source_name in &unimplemented {
        validate_scout_source_name(source_name)?;
    }

    Ok(unimplemented)
}

fn collect_skillforge_unimplemented_sources(scope: &str) -> Vec<String> {
    let mut unimplemented = Vec::new();
    let marker = "Source not yet implemented";
    let lines: Vec<&str> = scope.lines().collect();

    for (index, line) in lines.iter().enumerate() {
        if !line.contains(marker) {
            continue;
        }

        for candidate in lines[..=index].iter().rev() {
            if !candidate.contains("ScoutSource::") || !candidate.contains("=>") {
                continue;
            }

            let Some(match_arm) = candidate.split("=>").next() else {
                continue;
            };

            for segment in match_arm.split('|') {
                if let Some(start) = segment.find("ScoutSource::") {
                    let suffix = &segment[start + "ScoutSource::".len()..];
                    let source_name: String = suffix
                        .chars()
                        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                        .collect();

                    if !source_name.is_empty() {
                        unimplemented.push(source_name);
                    }
                }
            }

            break;
        }
    }

    unimplemented.sort_unstable();
    unimplemented.dedup();

    unimplemented
}

fn validate_scout_source_name(name: &str) -> Result<()> {
    match name {
        "GitHub" | "ClawHub" | "HuggingFace" => Ok(()),
        _ => Err(anyhow!("UnknownScoutSource: {name}")),
    }
}

fn normalize_and_deduplicate(
    mut entries: Vec<UnimplementedInventoryEntry>,
) -> Vec<UnimplementedInventoryEntry> {
    entries.sort_by(|a, b| {
        (
            a.category.as_str(),
            a.name.as_str(),
            a.source.as_str(),
            a.status.as_str(),
            a.priority,
        )
            .cmp(&(
                b.category.as_str(),
                b.name.as_str(),
                b.source.as_str(),
                b.status.as_str(),
                b.priority,
            ))
    });

    entries.dedup();
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::config::{ChannelsConfig, Config};
    use crate::plugins::integrations::registry;

    #[test]
    fn parse_registry_coming_soon_count_matches_baseline() {
        let count = parse_registry_coming_soon_count(REGISTRY_SOURCE).unwrap();
        assert_eq!(count, 35);
    }

    #[test]
    fn parse_skillforge_unimplemented_sources_matches_baseline() {
        let sources = parse_skillforge_unimplemented_sources(SKILLFORGE_SOURCE).unwrap();
        assert!(sources.is_empty());
    }

    #[test]
    fn validate_inventory_against_sources_detects_mismatches() {
        let expected = ScopeLockInventory {
            coming_soon_count: 2,
            skillforge_unimplemented: vec!["ClawHub".to_string()],
        };

        let error =
            validate_inventory_against_sources(&expected, REGISTRY_SOURCE, SKILLFORGE_SOURCE)
                .expect_err("mismatch should be detected");

        assert!(error.contains("coming_soon_count mismatch"));
        assert!(error.contains("skillforge_unimplemented mismatch"));
    }

    #[test]
    fn validate_inventory_against_sources_treats_skillforge_order_as_stable() {
        let expected = build_scope_lock_inventory().expect("baseline inventory should build");
        let mut reordered = expected.clone();
        reordered
            .skillforge_unimplemented
            .sort_unstable_by(|a, b| b.cmp(a));

        assert!(
            validate_inventory_against_sources(&reordered, REGISTRY_SOURCE, SKILLFORGE_SOURCE)
                .is_ok(),
            "validation should ignore list ordering differences"
        );
    }

    #[test]
    fn inventory_json_output_is_stable() {
        let first = build_scope_lock_inventory().expect("baseline inventory should build");
        let second = build_scope_lock_inventory().expect("baseline inventory should build");

        assert_eq!(first.to_json_pretty(), second.to_json_pretty());
    }

    #[test]
    fn parse_registry_coming_soon_count_matches_symbol_scan() {
        let scope = REGISTRY_SOURCE
            .split("#[cfg(test)]")
            .next()
            .expect("registry source should have cfg(test) marker");
        let symbol_count = count_symbol_occurrences(scope, "IntegrationStatus::ComingSoon");
        let parsed_count = parse_registry_coming_soon_count(REGISTRY_SOURCE).unwrap();

        assert_eq!(parsed_count, symbol_count);
    }

    #[test]
    fn parse_skillforge_unimplemented_sources_matches_symbol_scan() {
        let parsed = parse_skillforge_unimplemented_sources(SKILLFORGE_SOURCE).unwrap();
        let scanned = scan_unimplemented_skillforge_sources(SKILLFORGE_SOURCE);

        assert_eq!(parsed, scanned);
    }

    fn scan_unimplemented_skillforge_sources(source: &str) -> Vec<String> {
        let scope = source
            .split("#[cfg(test)]")
            .next()
            .expect("skillforge source should have cfg(test) marker");
        collect_skillforge_unimplemented_sources(scope)
    }

    fn count_symbol_occurrences(text: &str, symbol: &str) -> usize {
        text.matches(symbol).count()
    }

    #[test]
    fn sort_is_stable_by_category_then_name() {
        let normalized = vec![
            UnimplementedInventoryEntry {
                source: "skillforge".to_string(),
                category: "Sources".to_string(),
                status: "Unimplemented".to_string(),
                priority: 2,
                name: "ClawHub".to_string(),
            },
            UnimplementedInventoryEntry {
                source: "integrations".to_string(),
                category: "Social".to_string(),
                status: "ComingSoon".to_string(),
                priority: 1,
                name: "Slack".to_string(),
            },
            UnimplementedInventoryEntry {
                source: "integrations".to_string(),
                category: "AI Models".to_string(),
                status: "ComingSoon".to_string(),
                priority: 1,
                name: "Claude".to_string(),
            },
            UnimplementedInventoryEntry {
                source: "skillforge".to_string(),
                category: "Sources".to_string(),
                status: "Unimplemented".to_string(),
                priority: 2,
                name: "Aether".to_string(),
            },
        ];

        let normalized = normalize_and_deduplicate(normalized);

        assert_eq!(
            normalized,
            vec![
                UnimplementedInventoryEntry {
                    source: "integrations".to_string(),
                    category: "AI Models".to_string(),
                    status: "ComingSoon".to_string(),
                    priority: 1,
                    name: "Claude".to_string(),
                },
                UnimplementedInventoryEntry {
                    source: "integrations".to_string(),
                    category: "Social".to_string(),
                    status: "ComingSoon".to_string(),
                    priority: 1,
                    name: "Slack".to_string(),
                },
                UnimplementedInventoryEntry {
                    source: "skillforge".to_string(),
                    category: "Sources".to_string(),
                    status: "Unimplemented".to_string(),
                    priority: 2,
                    name: "Aether".to_string(),
                },
                UnimplementedInventoryEntry {
                    source: "skillforge".to_string(),
                    category: "Sources".to_string(),
                    status: "Unimplemented".to_string(),
                    priority: 2,
                    name: "ClawHub".to_string(),
                },
            ]
        );
    }

    #[test]
    fn parse_integration_capability_matrix_matches_registry_projection() {
        let matrix = load_integration_capability_matrix().expect("matrix artifact should parse");
        let entries = registry::all_integrations();
        let config = Config::default();

        let validation = validate_integration_status_against_matrix(&matrix, &entries, &config);
        assert!(
            validation.is_ok(),
            "baseline status matrix should match registry status projection: {validation:?}"
        );
    }

    #[test]
    fn registry_projection_rejects_unbacked_active_or_available() {
        let mut matrix =
            load_integration_capability_matrix().expect("matrix artifact should parse");
        matrix.entries.retain(|entry| entry.name != "Shell");

        let entries = registry::all_integrations();
        let config = Config {
            channels_config: ChannelsConfig::default(),
            ..Config::default()
        };

        let result = validate_integration_status_against_matrix(&matrix, &entries, &config)
            .expect_err("matrix missing implemented entry should fail");

        let has_shell = result.iter().any(|item| item.name == "Shell");
        let has_status = result
            .iter()
            .any(|item| item.kind == "unsupported_status_claim");

        assert!(
            has_shell,
            "missing implemented integration should be reported: {result:#?}"
        );
        assert!(
            has_status,
            "missing implemented entry should be reported as unsupported status claim"
        );
    }
}
