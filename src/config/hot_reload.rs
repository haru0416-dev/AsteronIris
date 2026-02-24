use arc_swap::ArcSwap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::Config;

/// Live-reloadable configuration holder.
///
/// Wraps `Config` in an `ArcSwap` so readers never block and writers
/// atomically swap the pointer. A file-watcher task can call
/// [`ConfigHandle::reload`] to pick up changes from disk.
pub struct ConfigHandle {
    inner: Arc<ArcSwap<Config>>,
    path: PathBuf,
}

impl ConfigHandle {
    /// Create a new handle seeded with `config`.
    pub fn new(config: Config) -> Self {
        let path = config.config_path.clone();
        Self {
            inner: Arc::new(ArcSwap::from_pointee(config)),
            path,
        }
    }

    /// Load current config snapshot. Lock-free.
    pub fn load(&self) -> arc_swap::Guard<Arc<Config>> {
        self.inner.load()
    }

    /// Return a clone of the current `Arc<Config>`.
    pub fn load_full(&self) -> Arc<Config> {
        self.inner.load_full()
    }

    /// Reload config from disk, atomically swapping the active snapshot.
    ///
    /// Returns `Ok(())` on success, propagating parse/validation errors.
    pub fn reload(&self) -> anyhow::Result<()> {
        let fresh = load_from_path(&self.path)?;
        self.inner.store(Arc::new(fresh));
        tracing::info!(path = %self.path.display(), "config hot-reloaded");
        Ok(())
    }

    /// Manually swap in a new config (e.g. after programmatic mutation).
    pub fn store(&self, config: Config) {
        self.inner.store(Arc::new(config));
    }

    /// Get the `ArcSwap` for direct subscription patterns.
    pub fn raw(&self) -> &Arc<ArcSwap<Config>> {
        &self.inner
    }

    /// Config file path being watched.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Clone for ConfigHandle {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            path: self.path.clone(),
        }
    }
}

fn load_from_path(path: &Path) -> anyhow::Result<Config> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read config at {}: {e}", path.display()))?;
    let config: Config = toml::from_str(&contents)
        .map_err(|e| anyhow::anyhow!("failed to parse config at {}: {e}", path.display()))?;
    config.validate_autonomy_controls()?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_handle_load_returns_current_snapshot() {
        let config = Config::default();
        let handle = ConfigHandle::new(config);
        let snapshot = handle.load();
        assert!(snapshot.default_provider.is_some());
    }

    #[test]
    fn config_handle_store_swaps_atomically() {
        let config = Config::default();
        let handle = ConfigHandle::new(config);

        let mut updated = Config::default();
        updated.default_temperature = 1.5;
        handle.store(updated);

        let snapshot = handle.load();
        assert!((snapshot.default_temperature - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn config_handle_clone_shares_state() {
        let config = Config::default();
        let handle = ConfigHandle::new(config);
        let clone = handle.clone();

        let mut updated = Config::default();
        updated.default_temperature = 0.3;
        handle.store(updated);

        let snapshot = clone.load();
        assert!((snapshot.default_temperature - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn reload_fails_on_missing_file() {
        let config = Config {
            config_path: PathBuf::from("/nonexistent/path/config.toml"),
            ..Config::default()
        };
        let handle = ConfigHandle::new(config);
        assert!(handle.reload().is_err());
    }
}
