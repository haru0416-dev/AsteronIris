use chrono::Utc;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};
use std::time::Instant;

#[derive(Debug, Clone, Serialize)]
pub struct ComponentHealth {
    pub status: String,
    pub updated_at: String,
    pub last_ok: Option<String>,
    pub last_error: Option<String>,
    pub restart_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthSnapshot {
    pub pid: u32,
    pub updated_at: String,
    pub uptime_seconds: u64,
    pub components: BTreeMap<String, ComponentHealth>,
}

struct HealthRegistry {
    started_at: Instant,
    components: RwLock<BTreeMap<String, ComponentHealth>>,
}

static REGISTRY: OnceLock<HealthRegistry> = OnceLock::new();

fn registry() -> &'static HealthRegistry {
    REGISTRY.get_or_init(|| HealthRegistry {
        started_at: Instant::now(),
        components: RwLock::new(BTreeMap::new()),
    })
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn upsert_component<F>(component: &str, update: F)
where
    F: FnOnce(&mut ComponentHealth),
{
    if let Ok(mut map) = registry().components.write() {
        let now = now_rfc3339();
        let entry = map
            .entry(component.to_string())
            .or_insert_with(|| ComponentHealth {
                status: "starting".into(),
                updated_at: now.clone(),
                last_ok: None,
                last_error: None,
                restart_count: 0,
            });
        update(entry);
        entry.updated_at = now;
    }
}

pub fn mark_component_ok(component: &str) {
    upsert_component(component, |entry| {
        entry.status = "ok".into();
        entry.last_ok = Some(now_rfc3339());
        entry.last_error = None;
    });
}

#[allow(clippy::needless_pass_by_value)]
pub fn mark_component_error(component: &str, error: impl ToString) {
    let err = error.to_string();
    upsert_component(component, move |entry| {
        entry.status = "error".into();
        entry.last_error = Some(err);
    });
}

pub fn bump_component_restart(component: &str) {
    upsert_component(component, |entry| {
        entry.restart_count = entry.restart_count.saturating_add(1);
    });
}

pub fn snapshot() -> HealthSnapshot {
    let components = registry()
        .components
        .read()
        .map_or_else(|_| BTreeMap::new(), |map| map.clone());

    HealthSnapshot {
        pid: std::process::id(),
        updated_at: now_rfc3339(),
        uptime_seconds: registry().started_at.elapsed().as_secs(),
        components,
    }
}

pub fn snapshot_json() -> serde_json::Value {
    serde_json::to_value(snapshot()).unwrap_or_else(|_| {
        serde_json::json!({
            "status": "error",
            "message": "failed to serialize health snapshot"
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_component(prefix: &str) -> String {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("{prefix}-{id}")
    }

    #[test]
    fn mark_component_ok_sets_ok_state() {
        let component = unique_component("health-ok");
        mark_component_ok(&component);

        let snap = snapshot();
        let state = snap
            .components
            .get(&component)
            .expect("component should exist in snapshot");

        assert_eq!(state.status, "ok");
        assert!(state.last_ok.is_some());
        assert_eq!(state.last_error, None);
    }

    #[test]
    fn mark_component_error_sets_error_and_preserves_last_ok() {
        let component = unique_component("health-error");
        mark_component_ok(&component);
        mark_component_error(&component, "boom");

        let snap = snapshot();
        let state = snap
            .components
            .get(&component)
            .expect("component should exist in snapshot");

        assert_eq!(state.status, "error");
        assert_eq!(state.last_error.as_deref(), Some("boom"));
        assert!(state.last_ok.is_some());
    }

    #[test]
    fn bump_component_restart_increments_counter() {
        let component = unique_component("health-restart");
        bump_component_restart(&component);
        bump_component_restart(&component);

        let snap = snapshot();
        let state = snap
            .components
            .get(&component)
            .expect("component should exist in snapshot");

        assert_eq!(state.restart_count, 2);
    }

    #[test]
    fn snapshot_json_includes_component_data() {
        let component = unique_component("health-json");
        mark_component_ok(&component);

        let json = snapshot_json();
        let status = json
            .get("components")
            .and_then(|components| components.get(&component))
            .and_then(|entry| entry.get("status"))
            .and_then(serde_json::Value::as_str);

        assert_eq!(status, Some("ok"));
    }
}
