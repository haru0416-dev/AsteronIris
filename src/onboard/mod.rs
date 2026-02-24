pub mod domain;
pub mod flow;
pub mod prompts;
pub mod scaffold;
pub mod view;
pub mod wizard;

// TODO: Port onboard TUI (18 files, feature-gated behind `tui`).

pub use flow::{run_channels_repair_wizard, run_quick_setup, run_wizard};
