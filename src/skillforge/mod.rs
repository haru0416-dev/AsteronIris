//! `SkillForge` — Skill auto-discovery, evaluation, and integration engine.
//!
//! Pipeline: Scout → Evaluate → Integrate
//! Discovers skills from external sources, scores them, and generates
//! AsteronIris-compatible manifests for qualified candidates.

mod config;
pub mod evaluate;
mod forge;
pub mod integrate;
pub mod scout;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use config::SkillForgeConfig;
#[allow(unused_imports)]
pub use forge::{ForgeReport, SkillForge};
