// Taste engine module — gated behind `taste` feature.
// Provides LLM-based aesthetic evaluation for text and UI artifacts.

pub(crate) mod adapter;
pub(crate) mod critic;
pub mod engine;
pub(crate) mod learner;
pub(crate) mod store;
pub mod types;

pub use engine::{TasteEngine, create_taste_engine};
pub use types::*;
