mod channel;

pub use channel::WhatsAppChannel;

#[cfg(test)]
use super::traits::Channel;

#[cfg(test)]
mod tests;
