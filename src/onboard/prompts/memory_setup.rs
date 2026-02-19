use crate::config::MemoryConfig;
use anyhow::Result;
use console::style;
use dialoguer::{Confirm, Select};

use super::super::view::print_bullet;

pub fn setup_memory() -> Result<MemoryConfig> {
    print_bullet("Choose how AsteronIris stores and searches memories.");
    print_bullet("You can always change this later in config.toml.");
    println!();

    let options = vec![
        "SQLite with Vector Search (recommended) — fast, hybrid search, embeddings",
        "Markdown Files — simple, human-readable, no dependencies",
        "None — disable persistent memory",
    ];

    let choice = Select::new()
        .with_prompt("  Select memory backend")
        .items(&options)
        .default(0)
        .interact()?;

    let backend = match choice {
        1 => "markdown",
        2 => "none",
        _ => "sqlite", // 0 and any unexpected value defaults to sqlite
    };

    let auto_save = if backend == "none" {
        false
    } else {
        let save = Confirm::new()
            .with_prompt("  Auto-save conversations to memory?")
            .default(true)
            .interact()?;
        save
    };

    println!(
        "  {} Memory: {} (auto-save: {})",
        style("✓").green().bold(),
        style(backend).green(),
        if auto_save { "on" } else { "off" }
    );

    Ok(MemoryConfig {
        backend: backend.to_string(),
        auto_save,
        hygiene_enabled: backend == "sqlite", // Only enable hygiene for SQLite
        archive_after_days: if backend == "sqlite" { 7 } else { 0 },
        purge_after_days: if backend == "sqlite" { 30 } else { 0 },
        conversation_retention_days: 30,
        layer_retention_working_days: None,
        layer_retention_episodic_days: None,
        layer_retention_semantic_days: None,
        layer_retention_procedural_days: None,
        layer_retention_identity_days: None,
        ledger_retention_days: None,
        embedding_provider: "none".to_string(),
        embedding_model: "text-embedding-3-small".to_string(),
        embedding_dimensions: 1536,
        vector_weight: 0.7,
        keyword_weight: 0.3,
        embedding_cache_size: if backend == "sqlite" { 10000 } else { 0 },
        chunk_max_tokens: 512,
    })
}
