use super::ReasonCode;

const SECURITY_DISABLE_PATTERNS: &[&str] = &[
    "set workspace_only = false",
    "set workspace_only=false",
    "workspace_only: false",
    "disable security",
    "disable guard",
    "turn off security",
    "security_policy = none",
    "security_policy = disabled",
    "remove security checks",
    "allowlist = *",
    "allow_all = true",
];

const CONFIG_TAMPERING_PATTERNS: &[&str] = &[
    "change autonomy_level to full",
    "autonomy_level = full",
    "set autonomy to unrestricted",
    "auto_approve = true",
    "bypass_approval = true",
    "disable_review = true",
    "skip_evaluation = true",
    "set min_score = 0",
    "min_score = 0.0",
    "set auto_integrate = true",
];

const CAPABILITY_MISMATCH_PATTERNS: &[&str] = &[
    "requires root",
    "needs sudo",
    "run as administrator",
    "requires elevated privileges",
];

const PERMISSION_REQUEST_PATTERNS: &[&str] = &[
    "grant me access",
    "give this skill permission",
    "add to allowlist",
    "whitelist this",
    "trust this skill",
];

const INSTRUCTION_OVERRIDE_PATTERNS: &[&str] = &[
    "ignore previous instructions",
    "ignore all previous instructions",
    "disregard previous instructions",
    "forget previous instructions",
    "developer message",
    "system prompt",
];

const PRIVILEGE_ESCALATION_PATTERNS: &[&str] = &[
    "bypass safety",
    "override safety",
    "act as system",
    "you are now root",
];

const SECRET_EXFILTRATION_PATTERNS: &[&str] = &[
    "reveal secrets",
    "exfiltrate",
    "print api key",
    "show environment variables",
    "dump tokens",
];

const TOOL_JAILBREAK_PATTERNS: &[&str] = &[
    "tool jailbreak",
    "execute shell",
    "call the shell tool",
    "bypass tool policy",
];

pub fn detect_markdown_reasons(text: &str) -> Vec<ReasonCode> {
    let mut reasons = Vec::new();
    let normalized = text.to_ascii_lowercase();
    let has = |patterns: &[&str]| patterns.iter().any(|pattern| normalized.contains(pattern));

    if has(SECURITY_DISABLE_PATTERNS) {
        reasons.push(ReasonCode::SecurityDisable);
    }
    if has(CONFIG_TAMPERING_PATTERNS) {
        reasons.push(ReasonCode::ConfigTampering);
    }
    if has(CAPABILITY_MISMATCH_PATTERNS) {
        reasons.push(ReasonCode::CapabilityMismatch);
    }
    if has(PERMISSION_REQUEST_PATTERNS) {
        reasons.push(ReasonCode::PermissionRequest);
    }
    if has(INSTRUCTION_OVERRIDE_PATTERNS) {
        reasons.push(ReasonCode::InstructionOverride);
    }
    if has(PRIVILEGE_ESCALATION_PATTERNS) {
        reasons.push(ReasonCode::PrivilegeEscalation);
    }
    if has(SECRET_EXFILTRATION_PATTERNS) {
        reasons.push(ReasonCode::SecretExfiltration);
    }
    if has(TOOL_JAILBREAK_PATTERNS) {
        reasons.push(ReasonCode::ToolJailbreak);
    }

    reasons
}
