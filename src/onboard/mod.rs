pub mod domain;
pub mod flow;
pub mod prompts;
pub mod scaffold;
#[cfg(feature = "tui")]
pub mod tui;
pub mod view;
pub mod wizard;

pub use flow::{run_channels_repair_wizard, run_quick_setup, run_wizard};
