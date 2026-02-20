pub(super) const MAX_CURRENT_OBJECTIVE_CHARS: usize = 280;
pub(super) const MAX_RECENT_CONTEXT_SUMMARY_CHARS: usize = 1200;
pub(super) const MAX_LIST_ITEM_CHARS: usize = 240;
pub(super) const MAX_MEMORY_APPEND_ITEMS: usize = 8;
pub(super) const MAX_MEMORY_APPEND_ITEM_CHARS: usize = 240;
pub(super) const MAX_SELF_TASKS: usize = 5;
pub(super) const MAX_SELF_TASK_TITLE_CHARS: usize = 120;
pub(super) const MAX_SELF_TASK_INSTRUCTIONS_CHARS: usize = 240;
pub(super) const MAX_SELF_TASK_EXPIRY_HOURS: i64 = 72;
pub(super) const STYLE_SCORE_MIN: u8 = 0;
pub(super) const STYLE_SCORE_MAX: u8 = 100;
pub(super) const STYLE_TEMPERATURE_MIN: f64 = 0.0;
pub(super) const STYLE_TEMPERATURE_MAX: f64 = 1.0;

pub(super) const MAX_OPEN_LOOPS: usize = 7;
pub(super) const MAX_NEXT_ACTIONS: usize = 3;
pub(super) const MAX_COMMITMENTS: usize = 5;

pub(super) const ALLOWED_TOP_LEVEL_FIELDS: [&str; 4] = [
    "state_header",
    "memory_append",
    "self_tasks",
    "style_profile",
];
pub(super) const ALLOWED_STATE_HEADER_FIELDS: [&str; 9] = [
    "schema_version",
    "identity_principles_hash",
    "safety_posture",
    "current_objective",
    "open_loops",
    "next_actions",
    "commitments",
    "recent_context_summary",
    "last_updated_at",
];

pub(super) const POISON_PATTERNS: [&str; 10] = [
    "ignore previous instructions",
    "ignore all previous instructions",
    "system prompt",
    "developer message",
    "override safety",
    "bypass safety",
    "disable guard",
    "exfiltrate",
    "reveal secrets",
    "tool jailbreak",
];
