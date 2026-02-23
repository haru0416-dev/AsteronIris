# Wave 4 Issues & Gotchas

<!-- Append only. Format: ## [TIMESTAMP] Task: {task-id}\n{content} -->

## [2026-02-23] Task: T11 - dispatch_parallel implementation

- Initial subagent-filtered tests were flaky because `configure_runtime` writes a global runtime used by multiple async tests.
- Fix: added a test-only global mutex (`TEST_RUNTIME_LOCK`) in `src/core/subagents/mod.rs` and acquired it in all subagent runtime-mutating tests to serialize runtime reconfiguration.
