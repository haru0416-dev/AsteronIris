#![allow(dead_code)]

use super::types::PairComparison;

pub(crate) trait TasteLearner: Send + Sync {
    fn update(&mut self, winner_id: &str, loser_id: &str, outcome: f64);
    fn get_rating(&self, item_id: &str) -> Option<(f64, u32)>;
    fn get_rating_if_sufficient(&self, item_id: &str, min_comparisons: u32) -> Option<f64>;
    fn from_comparisons(comparisons: &[PairComparison]) -> Self
    where
        Self: Sized;
}
