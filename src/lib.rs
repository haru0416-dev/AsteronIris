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

pub mod cli;
pub mod config;
pub mod core;
pub mod media;
pub mod onboard;
#[doc(hidden)]
pub mod platform;
pub mod plugins;
pub mod runtime;
pub mod security;
pub mod transport;
pub mod ui;
pub mod utils;

pub use cli::commands::{
    AuthCommands, ChannelCommands, CronCommands, IntegrationCommands, ServiceCommands,
    SkillCommands,
};
pub use config::Config;
