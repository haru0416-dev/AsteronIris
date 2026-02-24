//! `SkillForge` — Skill auto-discovery, evaluation, and integration engine.
//!
//! Pipeline: Scout -> Gate -> Evaluate -> Integrate
//! Discovers skills from external sources, runs them through a 4-layer
//! security gate, scores qualified candidates, and generates manifests.

pub mod capabilities;
mod config;
pub mod evaluate;
mod forge;
pub mod gate;
pub mod integrate;
mod overrides;
pub mod patterns;
pub mod provenance;
pub mod scout;
pub mod tiers;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use config::SkillForgeConfig;
#[allow(unused_imports)]
pub use forge::{ForgeReport, SkillForge};
#[allow(unused_imports)]
pub use gate::{Gate, GateInput, GateVerdict};
#[allow(unused_imports)]
pub use overrides::{SkillOverride, SkillOverrides, load_overrides};
#[allow(unused_imports)]
pub use patterns::ReasonCode;
#[allow(unused_imports)]
pub use tiers::SkillTier;
