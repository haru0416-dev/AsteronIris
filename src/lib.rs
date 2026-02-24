#![warn(clippy::all, clippy::pedantic)]
#![allow(
    async_fn_in_trait,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::unnecessary_literal_bound,
    clippy::module_name_repetitions,
    clippy::struct_field_names,
    clippy::must_use_candidate,
    clippy::new_without_default,
    clippy::return_self_not_must_use
)]

#[macro_use]
extern crate rust_i18n;

i18n!("locales", fallback = "en");

// ── Phase 0: Foundation ──────────────────────────────────────────────────────
pub mod config;
pub mod error;
pub mod media;
pub mod security;
pub mod ui;
pub mod utils;

// ── Phase 1: Abstract layer ──────────────────────────────────────────────────
pub mod llm;
pub mod memory;
pub mod prompt;
pub mod session;
pub mod tools;

// ── Phase 2: Agent ──────────────────────────────────────────────────────────
pub mod agent;
pub mod persona;
pub mod subagents;

// ── Phase 3: Process model ──────────────────────────────────────────────────
pub mod process;

// ── Phase 4: Transport + orchestration ──────────────────────────────────────
pub mod planner;
pub mod transport;

// ── Phase 5: Platform services + plugins ────────────────────────────────────
pub mod platform;
pub mod plugins;
pub mod runtime;

// ── Phase 6: Entry points ───────────────────────────────────────────────────
pub mod app;
pub mod cli;
pub mod onboard;

// ── Re-exports ───────────────────────────────────────────────────────────────
pub use config::Config;
