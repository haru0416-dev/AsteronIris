use crate::config::MemoryConfig;
use crate::ui::style as ui;
use anyhow::Result;
use dialoguer::{Confirm, Select};

use super::super::view::print_bullet;

pub fn setup_memory() -> Result<MemoryConfig> {
    print_bullet(&t!("onboard.memory.intro"));
    print_bullet(&t!("onboard.memory.later_hint"));
    println!();

    let options = vec![
        t!("onboard.memory.sqlite").to_string(),
        t!("onboard.memory.markdown").to_string(),
        t!("onboard.memory.none").to_string(),
    ];

    let choice = Select::new()
        .with_prompt(format!("  {}", t!("onboard.memory.select_prompt")))
        .items(&options)
        .default(0)
        .interact()?;

    let backend = match choice {
        1 => "markdown",
        2 => "none",
        _ => "sqlite",
    };

    let auto_save = if backend == "none" {
        false
    } else {
        Confirm::new()
            .with_prompt(format!("  {}", t!("onboard.memory.auto_save_prompt")))
            .default(true)
            .interact()?
    };

    println!(
        "  {} {}",
        ui::success("✓"),
        t!(
            "onboard.memory.confirm",
            backend = ui::value(backend),
            auto_save = if auto_save { "on" } else { "off" }
        )
    );

    Ok(MemoryConfig {
        backend: backend.to_string(),
        auto_save,
        hygiene_enabled: backend == "sqlite",
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
