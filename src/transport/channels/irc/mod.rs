mod auth;
pub mod channel;
mod message;
mod parse;
mod tls;

pub use channel::{IrcChannel, IrcChannelConfig};

#[cfg(test)]
mod tests;
