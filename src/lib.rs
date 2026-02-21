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

pub mod channels;
pub mod commands;
pub mod config;
pub mod core;
#[doc(hidden)]
pub mod diagnostics;
#[doc(hidden)]
pub mod eval;
#[doc(hidden)]
pub mod intelligence;
pub mod media;
#[doc(hidden)]
pub mod observability;
pub mod onboard;
#[doc(hidden)]
pub mod persona;
#[doc(hidden)]
pub mod platform;
pub mod plugins;
pub mod runtime;
pub mod security;
pub mod transport;
pub mod ui;
#[doc(hidden)]
pub mod usage;
#[doc(hidden)]
pub mod util;
pub mod utils;

pub use commands::{
    AuthCommands, ChannelCommands, CronCommands, IntegrationCommands, ServiceCommands,
    SkillCommands,
};
pub use config::Config;
