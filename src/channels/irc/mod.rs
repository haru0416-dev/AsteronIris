mod auth;
pub mod channel;
mod message;
mod parse;
mod tls;

pub use channel::IrcChannel;

#[cfg(test)]
mod tests;
