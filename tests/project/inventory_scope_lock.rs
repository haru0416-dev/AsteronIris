use asteroniris::config::Config;
use asteroniris::plugins::integrations::inventory::{
    IntegrationCapabilityDrift, IntegrationCapabilityMatrix, IntegrationCapabilityMatrixEntry,
    UnimplementedInventoryEntry, build_scope_lock_inventory, load_scope_lock_baseline_inventory,
    normalize_unimplemented_inventory_from_sources, parse_registry_coming_soon_count,
    parse_skillforge_unimplemented_sources, validate_integration_status_against_matrix,
    validate_inventory_against_sources,
};
use asteroniris::plugins::integrations::registry;

fn baseline_matrix() -> Vec<IntegrationCapabilityMatrixEntry> {
    let matrix: IntegrationCapabilityMatrix = serde_json::from_str(include_str!(
        "../../src/plugins/integrations/integration_capability_matrix.json"
    ))
    .expect("integration capability matrix should parse");
    matrix.entries
}

#[test]
fn inventory_scope_lock() {
    let inventory = build_scope_lock_inventory().expect("scope-lock inventory should build");
    let artifact = inventory.to_json_pretty();
    let baseline = load_scope_lock_baseline_inventory().expect("baseline artifact should parse");
    let registry_source = include_str!("../../src/plugins/integrations/registry.rs");
    let skillforge_source = include_str!("../../src/plugins/skillforge/mod.rs");

    let expected_coming_soon_count = parse_registry_coming_soon_count(registry_source)
        .expect("registry coming-soon parser should work");
    let expected_skillforge_unimplemented =
        parse_skillforge_unimplemented_sources(skillforge_source)
            .expect("skillforge unimplemented parser should work");

    println!("{artifact}");

    assert!(artifact.contains("\"coming_soon_count\""));
    assert!(artifact.contains("\"skillforge_unimplemented\""));
    assert_eq!(inventory, baseline);
    assert_eq!(inventory.coming_soon_count, expected_coming_soon_count);
    assert_eq!(
        inventory.skillforge_unimplemented,
        expected_skillforge_unimplemented
    );
    assert!(inventory.skillforge_unimplemented.is_empty());
}

#[test]
fn inventory_detects_registry_drift() {
    let inventory = build_scope_lock_inventory().expect("baseline inventory should build");
    let expected_coming_soon_count = inventory.coming_soon_count;

    let drifted_registry = include_str!("../../src/plugins/integrations/registry.rs").replacen(
        "IntegrationStatus::ComingSoon",
        "IntegrationStatus::Available",
        1,
    );
    let drifted_coming_soon_count =
        parse_registry_coming_soon_count(&drifted_registry).expect("drifted registry should parse");

    let drift_error = validate_inventory_against_sources(
        &inventory,
        &drifted_registry,
        include_str!("../../src/plugins/skillforge/mod.rs"),
    )
    .expect_err("drifted registry fixture should fail scope lock");

    println!("{drift_error}");

    let expected_mismatch = format!(
        "coming_soon_count mismatch: expected={}, actual={}",
        expected_coming_soon_count, drifted_coming_soon_count
    );

    assert!(drift_error.contains(&expected_mismatch));
    assert!(!drift_error.contains("skillforge_unimplemented mismatch"));
}

#[test]
fn inventory_ignores_test_only_registry_coming_soon_tokens() {
    let registry_source = include_str!("../../src/plugins/integrations/registry.rs");
    let baseline_count =
        parse_registry_coming_soon_count(registry_source).expect("baseline registry should parse");

    let fixture_with_test_only_drift = format!(
        "{registry_source}\n#[cfg(test)]\nmod drift_only {{\n    fn test_only_fixture() {{\n        let _ = \"IntegrationStatus::ComingSoon\";\n        let _ = \"IntegrationStatus::ComingSoon\";\n    }}\n}}\n"
    );

    let parsed_count = parse_registry_coming_soon_count(&fixture_with_test_only_drift)
        .expect("fixture with test-only drift should parse");

    assert_eq!(
        parsed_count, baseline_count,
        "test-only drift tokens must not affect production-scope count"
    );
}

#[test]
fn inventory_skillforge_marker_controls_unimplemented_extraction() {
    let markerless_skillforge_source = r#"
pub fn build() -> () {
    let source: ScoutSource = unimplemented!("unused");
    match source {
        ScoutSource::ClawHub | ScoutSource::HuggingFace => {
            info!("not implemented yet, but marker text differs");
        }
        ScoutSource::GitHub => {
            info!("implemented source");
        }
    }
}
"#;

    let unimplemented = parse_skillforge_unimplemented_sources(markerless_skillforge_source)
        .expect("markerless fixture should parse");

    assert!(
        unimplemented.is_empty(),
        "only explicit marker path should produce unimplemented sources"
    );
}

#[test]
fn normalize_unimplemented_inventory() {
    let registry_source = r#"
pub fn all_integrations() -> Vec<IntegrationEntry> {
    vec![
        IntegrationEntry {
            name: "Zeta",
            description: "Zeta integration",
            category: IntegrationCategory::AiModel,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Alpha",
            description: "Alpha integration",
            category: IntegrationCategory::Chat,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Alpha",
            description: "Duplicate",
            category: IntegrationCategory::Chat,
            status_fn: |_| IntegrationStatus::ComingSoon,
        },
        IntegrationEntry {
            name: "Active",
            description: "Already active",
            category: IntegrationCategory::Chat,
            status_fn: |_| IntegrationStatus::Available,
        },
    ];
}
"#;

    let skillforge_source = r#"
pub fn build() -> () {
    let source: ScoutSource = unimplemented!("unused");
    match source {
        ScoutSource::HuggingFace => {
            info!("Source not yet implemented");
        }
        ScoutSource::GitHub => {
            info!("implemented source");
        }
        ScoutSource::ClawHub => {
            info!("Source not yet implemented");
        }
    }
}
"#;

    let normalized: Vec<UnimplementedInventoryEntry> =
        normalize_unimplemented_inventory_from_sources(registry_source, skillforge_source)
            .expect("normalization should parse fixtures");

    assert_eq!(
        normalized,
        vec![
            UnimplementedInventoryEntry {
                source: "integrations".to_string(),
                category: "AI Models".to_string(),
                status: "ComingSoon".to_string(),
                priority: 1,
                name: "Zeta".to_string(),
            },
            UnimplementedInventoryEntry {
                source: "integrations".to_string(),
                category: "Chat Providers".to_string(),
                status: "ComingSoon".to_string(),
                priority: 1,
                name: "Alpha".to_string(),
            },
            UnimplementedInventoryEntry {
                source: "skillforge".to_string(),
                category: "Sources".to_string(),
                status: "Unimplemented".to_string(),
                priority: 2,
                name: "ClawHub".to_string(),
            },
            UnimplementedInventoryEntry {
                source: "skillforge".to_string(),
                category: "Sources".to_string(),
                status: "Unimplemented".to_string(),
                priority: 2,
                name: "HuggingFace".to_string(),
            },
        ]
    );
}

#[test]
fn normalize_inventory_unknown_source() {
    let registry_source = r#"
pub fn all_integrations() -> Vec<IntegrationEntry> {
    vec![]
}
"#;

    let skillforge_source = r#"
pub fn build() -> () {
    let source: ScoutSource = unimplemented!("unused");
    match source {
        ScoutSource::GitHub => {}
        ScoutSource::ClawHub | ScoutSource::UnknownSource => {
            warn!("Source not yet implemented");
        }
    }
}
"#;

    let error = normalize_unimplemented_inventory_from_sources(registry_source, skillforge_source)
        .expect_err("unknown source should fail normalization");

    let message = format!("{error}");
    assert!(message.contains("UnknownScoutSource"), "{message}");
    assert!(message.contains("UnknownSource"), "{message}");
}

#[test]
fn normalize_unimplemented_inventory_is_sorted_and_deduplicated() {
    let registry_source = r#"
pub fn all_integrations() -> Vec<IntegrationEntry> {
            vec![
                IntegrationEntry {
                    name: "Beta",
                    description: "Social integration",
                    category: IntegrationCategory::Social,
                    status_fn: |_| IntegrationStatus::ComingSoon,
                },
                IntegrationEntry {
                    name: "Alpha",
                    description: "AI integration",
                    category: IntegrationCategory::AiModel,
                    status_fn: |_| IntegrationStatus::ComingSoon,
                },
                IntegrationEntry {
                    name: "Beta",
                    description: "Social duplicate",
                    status_fn: |_| IntegrationStatus::ComingSoon,
                    category: IntegrationCategory::Social,
                },
                IntegrationEntry {
                    name: "Ignored",
                    description: "Available integration",
                    category: IntegrationCategory::ToolsAutomation,
                    status_fn: |_| IntegrationStatus::Active,
                },
            ];
        }
"#;

    let skillforge_source = r#"
pub fn build() -> () {
            let source: ScoutSource = unimplemented!("unused");
            match source {
                ScoutSource::HuggingFace => {
                    info!("Source not yet implemented");
                }
                ScoutSource::ClawHub => {
                    warn!("Source not yet implemented");
                }
            }
        }
"#;

    let normalized =
        normalize_unimplemented_inventory_from_sources(registry_source, skillforge_source)
            .expect("normalization should parse and deduplicate fixture");

    assert_eq!(
        normalized,
        vec![
            UnimplementedInventoryEntry {
                source: "integrations".to_string(),
                category: "AI Models".to_string(),
                status: "ComingSoon".to_string(),
                priority: 1,
                name: "Alpha".to_string(),
            },
            UnimplementedInventoryEntry {
                source: "integrations".to_string(),
                category: "Social".to_string(),
                status: "ComingSoon".to_string(),
                priority: 1,
                name: "Beta".to_string(),
            },
            UnimplementedInventoryEntry {
                source: "skillforge".to_string(),
                category: "Sources".to_string(),
                status: "Unimplemented".to_string(),
                priority: 2,
                name: "ClawHub".to_string(),
            },
            UnimplementedInventoryEntry {
                source: "skillforge".to_string(),
                category: "Sources".to_string(),
                status: "Unimplemented".to_string(),
                priority: 2,
                name: "HuggingFace".to_string(),
            },
        ]
    );
}

#[test]
fn integrations_status_matches_capability_matrix() {
    let matrix_entries = baseline_matrix();
    let registry_entries = registry::all_integrations();
    let matrix = asteroniris::plugins::integrations::inventory::IntegrationCapabilityMatrix {
        schema_version: "1".to_string(),
        source_file: "src/plugins/integrations/registry.rs".to_string(),
        entries: matrix_entries,
    };

    validate_integration_status_against_matrix(&matrix, &registry_entries, &Config::default())
        .expect("status and matrix projection should match");

    assert!(
        matrix
            .entries
            .iter()
            .any(|entry| entry.name == "Shell" && entry.implemented),
        "baseline matrix must include implemented Shell"
    );
    assert!(
        matrix.entries.iter().all(|entry| entry.name != "WhatsApp"),
        "non-implemented WhatsApp must remain unlisted"
    );
}

#[test]
fn integrations_rejects_unbacked_active_status() {
    let mut matrix = asteroniris::plugins::integrations::inventory::IntegrationCapabilityMatrix {
        schema_version: "1".to_string(),
        source_file: "src/plugins/integrations/registry.rs".to_string(),
        entries: baseline_matrix()
            .into_iter()
            .filter(|entry| entry.name != "Shell")
            .collect(),
    };
    matrix.entries.sort_by(|a, b| a.name.cmp(&b.name));

    let registry_entries = registry::all_integrations();
    let drifts =
        validate_integration_status_against_matrix(&matrix, &registry_entries, &Config::default())
            .expect_err("missing implemented capability for Shell should fail");

    assert!(
        drifts
            .iter()
            .any(|drift: &IntegrationCapabilityDrift| drift.name == "Shell"),
        "expected Shell mismatch to be reported: {drifts:?}"
    );
    assert!(
        drifts
            .iter()
            .any(|drift| drift.kind == "unsupported_status_claim"),
        "expected unsupported status claim classification for missing entry"
    );
}

#[test]
fn load_capability_matrix_and_validate() {
    let matrix =
        asteroniris::plugins::integrations::inventory::load_integration_capability_matrix()
            .expect("matrix should parse through loader helper");
    let registry_entries = registry::all_integrations();
    let result =
        validate_integration_status_against_matrix(&matrix, &registry_entries, &Config::default());
    assert!(
        result.is_ok(),
        "matrix projection should pass baseline validation: {result:?}"
    );
}
