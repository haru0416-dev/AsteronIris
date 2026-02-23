use super::IntegrationEntry;

mod catalog;
mod status;

/// Returns the full catalog of integrations.
pub fn all_integrations() -> Vec<IntegrationEntry> {
    catalog::all_integrations()
}

#[cfg(test)]
mod tests;
