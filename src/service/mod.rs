mod commands;
mod platform;
mod utils;

#[cfg(test)]
mod tests;

pub use commands::handle_command;

pub(super) const SERVICE_LABEL: &str = "com.asteroniris.daemon";
