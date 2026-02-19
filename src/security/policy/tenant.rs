use serde::{Deserialize, Serialize};

pub const TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR: &str =
    "blocked by security policy: tenant recall scope mismatch";
pub const TENANT_DEFAULT_SCOPE_FALLBACK_DENIED_ERROR: &str =
    "blocked by security policy: tenant mode forbids default recall scope";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantPolicyContext {
    pub tenant_mode_enabled: bool,
    pub tenant_id: Option<String>,
}

impl TenantPolicyContext {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn enabled(tenant_id: impl Into<String>) -> Self {
        Self {
            tenant_mode_enabled: true,
            tenant_id: Some(tenant_id.into()),
        }
    }

    pub fn enforce_recall_scope(&self, entity_id: &str) -> Result<(), &'static str> {
        if !self.tenant_mode_enabled {
            return Ok(());
        }

        let requested = entity_id.trim();
        if requested.is_empty() || requested == "default" {
            return Err(TENANT_DEFAULT_SCOPE_FALLBACK_DENIED_ERROR);
        }

        let Some(tenant_id) = self.tenant_id.as_deref() else {
            return Err(TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR);
        };

        let in_scope = requested == tenant_id
            || requested
                .strip_prefix(tenant_id)
                .is_some_and(|suffix| suffix.starts_with(':') || suffix.starts_with('/'));

        if in_scope {
            Ok(())
        } else {
            Err(TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR)
        }
    }
}
