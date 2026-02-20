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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scope_is_rejected_in_tenant_mode() {
        let context = TenantPolicyContext::enabled("tenant-alpha");

        assert_eq!(
            context.enforce_recall_scope("default"),
            Err(TENANT_DEFAULT_SCOPE_FALLBACK_DENIED_ERROR)
        );
    }

    #[test]
    fn empty_entity_id_is_rejected_in_tenant_mode() {
        let context = TenantPolicyContext::enabled("tenant-alpha");

        assert_eq!(
            context.enforce_recall_scope("   "),
            Err(TENANT_DEFAULT_SCOPE_FALLBACK_DENIED_ERROR)
        );
    }

    #[test]
    fn matching_tenant_id_is_allowed() {
        let context = TenantPolicyContext::enabled("tenant-alpha");

        assert!(context.enforce_recall_scope("tenant-alpha").is_ok());
    }

    #[test]
    fn mismatched_tenant_id_is_rejected() {
        let context = TenantPolicyContext::enabled("tenant-alpha");

        assert_eq!(
            context.enforce_recall_scope("tenant-beta"),
            Err(TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR)
        );
    }

    #[test]
    fn hierarchical_colon_scope_is_allowed_for_same_tenant() {
        let context = TenantPolicyContext::enabled("tenant-alpha");

        assert!(
            context
                .enforce_recall_scope("tenant-alpha:subtenant:user-1")
                .is_ok()
        );
    }

    #[test]
    fn hierarchical_slash_scope_is_allowed_for_same_tenant() {
        let context = TenantPolicyContext::enabled("tenant-alpha");

        assert!(
            context
                .enforce_recall_scope("tenant-alpha/subtenant/session")
                .is_ok()
        );
    }

    #[test]
    fn empty_tenant_id_in_context_rejects_requests() {
        let context = TenantPolicyContext {
            tenant_mode_enabled: true,
            tenant_id: Some(String::new()),
        };

        assert_eq!(
            context.enforce_recall_scope("tenant-alpha"),
            Err(TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR)
        );
    }

    #[test]
    fn missing_tenant_id_in_context_rejects_requests() {
        let context = TenantPolicyContext {
            tenant_mode_enabled: true,
            tenant_id: None,
        };

        assert_eq!(
            context.enforce_recall_scope("tenant-alpha"),
            Err(TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR)
        );
    }

    #[test]
    fn non_tenant_mode_always_allows() {
        let context = TenantPolicyContext::disabled();

        assert!(context.enforce_recall_scope("").is_ok());
        assert!(context.enforce_recall_scope("default").is_ok());
        assert!(context.enforce_recall_scope("tenant-beta").is_ok());
    }
}
