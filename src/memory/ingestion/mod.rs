mod pipeline;
mod signal_envelope;

pub use pipeline::{IngestionPipeline, IngestionResult, SqliteIngestionPipeline};
pub use signal_envelope::SignalEnvelope;

#[cfg(test)]
use crate::memory::traits::Memory;
#[cfg(test)]
use crate::memory::types::{PrivacyLevel, SignalTier, SourceKind};
#[cfg(test)]
use pipeline::semantic_dedup_key;
#[cfg(test)]
use std::sync::Arc;

#[cfg(test)]
mod tests;
