#![allow(dead_code)]

use super::critic::CritiqueResult;
use super::types::{Domain, Suggestion, TasteContext};

pub(crate) trait DomainAdapter: Send + Sync {
    fn domain(&self) -> Domain;
    fn suggest(&self, critique: &CritiqueResult, ctx: &TasteContext) -> Vec<Suggestion>;
}
