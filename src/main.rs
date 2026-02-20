#![warn(clippy::all, clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::unnecessary_literal_bound,
    clippy::module_name_repetitions,
    clippy::struct_field_names,
    dead_code
)]

#[macro_use]
extern crate rust_i18n;

i18n!("locales", fallback = "en");

use anyhow::Result;
use clap::Parser;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

mod agent;
mod app;
mod auth;
mod channels;
mod cli;
mod config;
mod cron;
mod daemon;
mod doctor;
mod gateway;
mod health;
mod heartbeat;
mod integrations;
mod memory;
mod observability;
mod onboard;
mod persona;
mod providers;
mod runtime;
mod security;
mod service;
mod skillforge;
mod skills;
mod tools;
mod tunnel;
mod util;

pub(crate) use cli::commands::{
    AuthCommands, ChannelCommands, Cli, CronCommands, IntegrationCommands, ServiceCommands,
    SkillCommands,
};
use config::Config;

#[tokio::main]
async fn main() -> Result<()> {
    // Install default crypto provider for Rustls TLS.
    // This prevents the error: "could not automatically determine the process-level CryptoProvider"
    // when both aws-lc-rs and ring features are available (or neither is explicitly selected).
    if let Err(e) = rustls::crypto::ring::default_provider().install_default() {
        eprintln!("Warning: Failed to install default crypto provider: {e:?}");
    }

    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let cli = Cli::parse();
    let config = Config::load_or_init()?;
    app::dispatch::dispatch(cli, config).await
}
