#![warn(clippy::all, clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::unnecessary_literal_bound,
    clippy::module_name_repetitions,
    clippy::struct_field_names,
    dead_code
)]

use anyhow::{Context, Result};
use clap::Parser;
use std::sync::Arc;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use asteroniris::Config;
use asteroniris::cli::commands::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    // Install default crypto provider for Rustls TLS.
    if let Err(e) = rustls::crypto::ring::default_provider().install_default() {
        eprintln!("Warning: Failed to install default crypto provider: {e:?}");
    }

    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .context("setting default subscriber failed")?;

    let cli = Cli::parse();
    let config = Arc::new(Config::load_or_init()?);
    asteroniris::app::dispatch::dispatch(cli, config).await
}
