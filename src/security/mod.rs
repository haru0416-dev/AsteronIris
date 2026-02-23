pub mod approval;
pub mod auth;
pub mod defaults;
pub mod external_content;
pub mod pairing;
pub mod permissions;
pub mod policy;
pub mod secrets;
pub mod url_validation;
pub mod writeback_guard;

#[allow(unused_imports)]
pub use approval::{
    ApprovalBroker, ApprovalDecision, ApprovalRequest, AutoDenyBroker, GrantScope, PermissionGrant,
    RiskLevel, classify_risk, summarize_args,
};
#[allow(unused_imports)]
pub use approval::{ChannelApprovalContext, TextReplyApprovalBroker, broker_for_channel};
#[allow(unused_imports)]
pub use approval::CliApprovalBroker;
#[cfg(feature = "discord")]
#[allow(unused_imports)]
pub use approval::DiscordApprovalBroker;
#[allow(unused_imports)]
pub use approval::TelegramApprovalBroker;
pub use defaults::{default_allowed_commands, default_forbidden_paths};
#[allow(unused_imports)]
pub use pairing::PairingGuard;
pub use permissions::PermissionStore;
pub use policy::{
    ActionPolicyVerdict, AutonomyLevel, EntityRateLimiter, ExternalActionExecution, SecurityPolicy,
};
#[allow(unused_imports)]
pub use secrets::SecretStore;
#[allow(unused_imports)]
pub use url_validation::{
    is_private_host as is_ssrf_private_host, is_private_ip, validate_url_not_ssrf,
};
