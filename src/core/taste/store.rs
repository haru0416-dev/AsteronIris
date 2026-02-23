#![allow(dead_code)]

use super::types::{Domain, PairComparison};
use async_trait::async_trait;

pub struct ItemRating {
    pub item_id: String,
    pub domain: Domain,
    pub rating: f64,
    pub n_comparisons: u32,
    pub updated_at: String,
}

#[async_trait]
pub(crate) trait TasteStore: Send + Sync {
    async fn save_comparison(&self, comparison: &PairComparison) -> anyhow::Result<()>;
    async fn get_comparisons_for_item(
        &self,
        item_id: &str,
        domain: &Domain,
    ) -> anyhow::Result<Vec<PairComparison>>;
    async fn get_rating(
        &self,
        item_id: &str,
        domain: &Domain,
    ) -> anyhow::Result<Option<ItemRating>>;
    async fn update_rating(&self, rating: ItemRating) -> anyhow::Result<()>;
    async fn get_all_ratings(&self, domain: &Domain) -> anyhow::Result<Vec<ItemRating>>;
}
