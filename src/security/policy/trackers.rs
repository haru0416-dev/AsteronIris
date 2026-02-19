use std::sync::Mutex;
use std::time::Instant;

/// Sliding-window action tracker for rate limiting.
#[derive(Debug)]
pub struct ActionTracker {
    /// Timestamps of recent actions (kept within the last hour).
    actions: Mutex<Vec<Instant>>,
}

impl ActionTracker {
    pub fn new() -> Self {
        Self {
            actions: Mutex::new(Vec::new()),
        }
    }

    /// Record an action and return the current count within the window.
    pub fn record(&self) -> usize {
        let mut actions = self
            .actions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let cutoff = Instant::now()
            .checked_sub(std::time::Duration::from_secs(3600))
            .unwrap_or_else(Instant::now);
        actions.retain(|t| *t > cutoff);
        actions.push(Instant::now());
        actions.len()
    }

    /// Count of actions in the current window without recording.
    pub fn count(&self) -> usize {
        let mut actions = self
            .actions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let cutoff = Instant::now()
            .checked_sub(std::time::Duration::from_secs(3600))
            .unwrap_or_else(Instant::now);
        actions.retain(|t| *t > cutoff);
        actions.len()
    }
}

impl Clone for ActionTracker {
    fn clone(&self) -> Self {
        let actions = self
            .actions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self {
            actions: Mutex::new(actions.clone()),
        }
    }
}

#[derive(Debug)]
pub struct CostTracker {
    state: Mutex<DailyCostState>,
}

#[derive(Debug, Clone, Copy)]
struct DailyCostState {
    day_epoch: u64,
    spent_cents: u32,
}

impl CostTracker {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(DailyCostState {
                day_epoch: current_day_epoch(),
                spent_cents: 0,
            }),
        }
    }

    pub fn record(&self, additional_cents: u32, max_cents_per_day: u32) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rollover_day_if_needed(&mut state);
        if additional_cents == 0 {
            return state.spent_cents <= max_cents_per_day;
        }
        if state.spent_cents.saturating_add(additional_cents) > max_cents_per_day {
            return false;
        }
        state.spent_cents = state.spent_cents.saturating_add(additional_cents);
        true
    }

    pub fn spent_today(&self) -> u32 {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rollover_day_if_needed(&mut state);
        state.spent_cents
    }
}

impl Clone for CostTracker {
    fn clone(&self) -> Self {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self {
            state: Mutex::new(*state),
        }
    }
}

fn current_day_epoch() -> u64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
        .as_secs();
    secs / 86_400
}

fn rollover_day_if_needed(state: &mut DailyCostState) {
    let today = current_day_epoch();
    if state.day_epoch != today {
        state.day_epoch = today;
        state.spent_cents = 0;
    }
}
