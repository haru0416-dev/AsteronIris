use chrono::{DateTime, Utc};

pub const AGENT_PENDING_CAP: usize = 5;

#[derive(Debug, Clone)]
pub struct CronJob {
    pub id: String,
    pub expression: String,
    pub command: String,
    pub next_run: DateTime<Utc>,
    pub last_run: Option<DateTime<Utc>>,
    pub last_status: Option<String>,
    pub job_kind: CronJobKind,
    pub origin: CronJobOrigin,
    pub expires_at: Option<DateTime<Utc>>,
    pub max_attempts: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CronJobKind {
    User,
    Agent,
}

impl CronJobKind {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
        }
    }

    pub(crate) fn from_db(value: &str) -> Self {
        if value.eq_ignore_ascii_case("agent") {
            Self::Agent
        } else {
            Self::User
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CronJobOrigin {
    User,
    Agent,
}

impl CronJobOrigin {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
        }
    }

    pub(crate) fn from_db(value: &str) -> Self {
        if value.eq_ignore_ascii_case("agent") {
            Self::Agent
        } else {
            Self::User
        }
    }

    pub(crate) fn is_agent(self) -> bool {
        self == Self::Agent
    }
}

#[derive(Debug, Clone)]
pub struct CronJobMetadata {
    pub job_kind: CronJobKind,
    pub origin: CronJobOrigin,
    pub expires_at: Option<DateTime<Utc>>,
    pub max_attempts: u32,
}

impl Default for CronJobMetadata {
    fn default() -> Self {
        Self {
            job_kind: CronJobKind::User,
            origin: CronJobOrigin::User,
            expires_at: None,
            max_attempts: 1,
        }
    }
}
