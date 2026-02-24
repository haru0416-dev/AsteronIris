#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImmutableStateHeader {
    pub schema_version: u32,
    pub identity_principles_hash: String,
    pub safety_posture: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateHeaderWriteback {
    pub current_objective: String,
    pub open_loops: Vec<String>,
    pub next_actions: Vec<String>,
    pub commitments: Vec<String>,
    pub recent_context_summary: String,
    pub last_updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WritebackPayload {
    pub state_header: StateHeaderWriteback,
    pub memory_append: Vec<String>,
    pub self_tasks: Vec<SelfTaskWriteback>,
    pub style_profile: Option<StyleProfileWriteback>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfTaskWriteback {
    pub title: String,
    pub instructions: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StyleProfileWriteback {
    pub formality: u8,
    pub verbosity: u8,
    pub temperature: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WritebackGuardVerdict {
    Accepted(WritebackPayload),
    Rejected { reason: String },
}
