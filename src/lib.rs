#![warn(clippy::all, clippy::pedantic)]
#![allow(
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

pub mod auth;
pub mod channels;
pub mod commands;
pub mod config;
pub mod diagnostics;
pub mod eval;
pub mod gateway;
pub mod integrations;
pub mod intelligence;
pub mod links;
#[cfg(feature = "mcp")]
pub mod mcp;
pub mod media;
pub mod observability;
pub mod onboard;
pub mod persona;
pub mod platform;
pub mod runtime;
pub mod security;
pub mod skills;
pub mod tunnel;
pub mod ui;
pub mod usage;
pub mod util;

pub use commands::{
    AuthCommands, ChannelCommands, CronCommands, IntegrationCommands, ServiceCommands,
    SkillCommands,
};
pub use config::Config;
