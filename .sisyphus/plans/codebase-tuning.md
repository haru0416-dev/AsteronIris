# AsteronIris Codebase Foundation Overhaul

## TL;DR

> **Quick Summary**: Comprehensive structural overhaul of AsteronIris (53,583 LOC / 36 modules) to eliminate patchwork duplication, optimize data flow, decompose bloated modules, and align patterns — all while preserving every existing behavior and passing all 953+ tests.
> 
> **Deliverables**:
> - Eliminated ~320 lines of duplicated channel/tool code
> - Arc<Config> shared ownership (eliminates 10+ deep Config clones)
> - 5 fat mod.rs files decomposed into proper sub-modules
> - Cow<str> fast path for secret scrubbing (zero-alloc when no secrets)
> - Dead code cleanup and production unwrap() fixes
> - Test modules extracted for maintainability
> 
> **Estimated Effort**: Large
> **Parallel Execution**: YES — 6 waves + final verification
> **Critical Path**: Test Extraction → Deduplication → Arc<Config> → Module Decomposition → Data Flow Opt → Cleanup

---

## Context

### Original Request
コード自体の基盤からの改善。継ぎ足しが多いのでデータフローのチューニング、軽量化、既存機能のチューニングを包括的に実施。

### Interview Summary
**Key Discussions**:
- Aggressiveness: **Structural overhaul** — pub API cleanup, God Module decomposition, module consolidation, trait boundary review
- Priority: **Balanced** — both performance (allocations) and maintainability (dedup/structure)
- Off-limits: **None** — all modules open for improvement, but security behavior must be preserved
- Test strategy: **Existing tests pass + tests themselves get optimized**

**Research Findings**:
- Config imported by 19 modules (God Module) — cloned 10+ times at daemon startup
- 5 mod.rs files violate "thin facade" rule (security/policy 1016 LOC, providers 963 LOC, gateway 929 LOC)
- Channel construction duplicated 2x (~170 lines), tool descriptions duplicated 2x (~40 lines)
- scrub_secret_patterns() always allocates even when no secrets found
- security/policy/mod.rs is 86% test code — "decomposing" it is primarily test extraction
- **Dual crate root**: main.rs and lib.rs declare different module sets — refactoring must compile under BOTH
- 16 dead_code suppressions (not 27 as initially estimated) in memory/sqlite/*
- Only 2-3 production unwrap() calls (not 849 — rest are in test code)

### Metis Review
**Identified Gaps** (addressed):
- **Dual crate root danger**: Every module move must compile under both `cargo check --lib` AND `cargo check --bin asteroniris`
- **Feature gate danger zones**: email stub, vector-search conditional arrays, tui path refs, cfg(test) imports
- **Corrected dead code count**: 16 actual suppressions across memory/sqlite (12), lancedb (1), markdown (2), gemini (1)
- **scrub Cow<str> complexity**: Non-trivial — helper function must return whether it modified anything
- **Module consolidation dropped**: health+heartbeat+doctor share NO types/traits — merging creates artificial coupling
- **Config TOML format preserved**: No field renames or nesting changes — must not break existing user configs
- **Test ordering**: Extract test modules FIRST to reduce blast radius of subsequent changes
- **Security regression gate**: Explicit test commands for all 5 security layers after any security/ change

---

## Work Objectives

### Core Objective
Eliminate patchwork technical debt and optimize data flow in AsteronIris while preserving all external behavior, security properties, and config compatibility.

### Concrete Deliverables
- All duplicated code extracted to shared functions/constants
- Config passed by Arc or reference instead of owned-clone
- Fat mod.rs files decomposed with facade re-exports preserved
- Secret scrubbing optimized with Cow<str> fast path
- All dead_code suppressions reviewed and resolved
- Production unwrap() calls fixed (2-3 items)
- Test modules extracted from fat files

### Definition of Done
- [ ] `cargo fmt -- --check` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo test` passes (excluding known pre-existing failure: `inventory_scope_lock::inventory_scope_lock`)
- [ ] `cargo check --no-default-features` passes
- [ ] `cargo check --features "email,vector-search,tui,media"` passes
- [ ] Zero new clippy suppressions added (baseline: count existing before starting)
- [ ] clippy suppression count does not increase

### Must Have
- All 953+ existing tests pass
- Security behavior preserved across all 5 layers
- Config TOML deserialization backward-compatible
- All `pub use` re-exports in facade mod.rs files preserved
- Feature-gated compilation works for all feature combinations

### Must NOT Have (Guardrails)
- **No new features** — pure refactoring only
- **No new dependencies** — work with existing crate set
- **No Config TOML format changes** — no field renames, no nesting changes
- **No security model changes** — deny-by-default, pairing, vault, writeback guard, scrubbing all preserved
- **No module consolidation of health+heartbeat+doctor** — they share no types
- **No hunting for unwrap() in test code** — only fix 2-3 production instances
- **No removing `#[allow(dead_code)]` without verifying** the item isn't used in SQL string interpolation or serde
- **No over-engineering** — no trait abstractions "for future flexibility" that add complexity now

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES (cargo test, 953 unit tests + 6 integration binaries)
- **Automated tests**: Existing tests (no TDD for refactoring — behavior doesn't change)
- **Framework**: cargo test (built-in)

### QA Policy
Every task MUST end with the **Standard Verification Gate**:
```bash
cargo fmt -- --check
cargo clippy -- -D warnings
cargo test
```

For module structural changes, add **Feature Matrix Gate**:
```bash
cargo check --no-default-features
cargo check --features "email,vector-search,tui,media"
cargo check --lib
cargo check --bin asteroniris
```

For security/ changes, add **Security Regression Gate**:
```bash
cargo test --test agent -- action_intent
cargo test --test agent -- external_content
cargo test --test gateway
cargo test --test persona -- injection_guard
cargo test --test memory -- governance
cargo test --test memory -- revocation_gate
cargo test --test memory -- tenant_recall
```

Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Test Extraction — all parallel, zero behavior risk):
├── Task 1: Extract tests from security/policy/mod.rs → tests.rs [quick]
├── Task 2: Extract tests from gateway/mod.rs → tests.rs [quick]
├── Task 3: Extract tests from security/secrets.rs → tests.rs [quick]
├── Task 4: Extract tests from providers/mod.rs → tests.rs [quick]
└── Task 5: Extract tests from skills/mod.rs → tests.rs [quick]

Wave 2 (Code Deduplication — mostly parallel, after Wave 1):
├── Task 6: Extract shared channel factory function (depends: none) [unspecified-high]
├── Task 7: Extract shared tool descriptions constant (depends: none) [quick]
├── Task 8: Fix production unwrap() calls (depends: none) [quick]
└── Task 9: Clean up email feature-gate stub (depends: none) [quick]

Wave 3 (Arc<Config> Migration — sequential core, after Wave 2):
├── Task 10: Introduce Arc<Config> at daemon/gateway/channels entry points [deep]
├── Task 11: Propagate &Config / Arc<Config> through agent loop [deep]
└── Task 12: Update integration tests for Config reference changes [unspecified-high]

Wave 4 (Module Decomposition — parallel, after Wave 3):
├── Task 13: Decompose providers/mod.rs → factory.rs + scrub.rs [unspecified-high]
├── Task 14: Decompose security/policy/mod.rs → enforcement.rs [unspecified-high]
├── Task 15: Decompose skills/mod.rs → loader.rs [unspecified-high]
├── Task 16: Split channels/imessage.rs into sub-modules [unspecified-high]
└── Task 17: Split channels/telegram.rs into sub-modules [unspecified-high]

Wave 5 (Data Flow Optimization — parallel, after Wave 4):
├── Task 18: Implement Cow<str> for scrub_secret_patterns() [deep]
├── Task 19: Reduce Arc cloning in agent loop hot path [unspecified-high]
└── Task 20: Optimize tool registry creation [quick]

Wave 6 (Cleanup & Polish — parallel, after Wave 5):
├── Task 21: Audit and resolve dead_code suppressions in memory/* [unspecified-high]
├── Task 22: Test optimization: extract/deduplicate test helpers [unspecified-high]
└── Task 23: Remove resolved clippy suppressions (too_many_lines, too_many_arguments) [quick]

Wave FINAL (After ALL tasks — independent review, 4 parallel):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Full regression QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)

Critical Path: T1-5 → T6 → T10 → T13 → T18 → T21 → F1-F4
Parallel Speedup: ~65% faster than sequential
Max Concurrent: 5 (Waves 1 & 4)
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave |
|------|-----------|--------|------|
| 1-5 | — | 6-9, 13-17 | 1 |
| 6 | — | 10 | 2 |
| 7 | — | 10, 11 | 2 |
| 8 | — | — | 2 |
| 9 | — | — | 2 |
| 10 | 6, 7 | 11, 12, 13-17 | 3 |
| 11 | 10 | 12, 19 | 3 |
| 12 | 11 | — | 3 |
| 13-17 | 10 | 18-20 | 4 |
| 18-20 | 13-17 | 21-23 | 5 |
| 21-23 | 18-20 | F1-F4 | 6 |
| F1-F4 | 21-23 | — | FINAL |

### Agent Dispatch Summary

- **Wave 1**: 5 tasks → all `quick`
- **Wave 2**: 4 tasks → T6 `unspecified-high`, T7-T9 `quick`
- **Wave 3**: 3 tasks → T10-T11 `deep`, T12 `unspecified-high`
- **Wave 4**: 5 tasks → all `unspecified-high`
- **Wave 5**: 3 tasks → T18 `deep`, T19 `unspecified-high`, T20 `quick`
- **Wave 6**: 3 tasks → T21-T22 `unspecified-high`, T23 `quick`
- **FINAL**: 4 tasks → F1 `oracle`, F2-F3 `unspecified-high`, F4 `deep`

---

## TODOs

### Wave 1: Test Extraction (all parallel — zero behavior risk)

- [ ] 1. Extract tests from security/policy/mod.rs → tests.rs

  **What to do**:
  - Move the `#[cfg(test)] mod tests { ... }` block (~877 lines, 87 tests) from `src/security/policy/mod.rs` to a new `src/security/policy/tests.rs` file
  - In mod.rs, replace the inline test module with `#[cfg(test)] mod tests;`
  - Ensure all `use super::*;` and test-specific imports are correct in the new file
  - Run `cargo test --lib -- security::policy::tests` to verify all 87 tests pass
  - Run Feature Matrix Gate (this module has no feature gates but verify anyway)

  **Must NOT do**:
  - Do not change any production logic — only move test code
  - Do not rename any test functions
  - Do not change test module path (tests should still appear as `security::policy::tests::*` in cargo test output)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []
    - Pure mechanical file split, no domain knowledge needed

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3, 4, 5)
  - **Blocks**: Tasks 13, 14 (module decomposition depends on clean files)
  - **Blocked By**: None

  **References**:
  - `src/security/policy/mod.rs` — Production code ends ~line 138, tests start ~line 139. The file is 86% test code.
  - `AGENTS.md` — "mod.rs is a thin facade" rule. Tests in separate files is the target pattern.

  **Acceptance Criteria**:
  - [ ] `src/security/policy/tests.rs` exists with all 87 tests
  - [ ] `src/security/policy/mod.rs` reduced to ~138 production lines + `#[cfg(test)] mod tests;`
  - [ ] `cargo test --lib -- security::policy` → all 87 tests pass

  **QA Scenarios**:
  ```
  Scenario: All security policy tests pass after extraction
    Tool: Bash
    Preconditions: Tests moved to tests.rs
    Steps:
      1. Run: cargo test --lib -- security::policy::tests 2>&1
      2. Assert output contains "test result: ok" and "87 passed" (or original count)
      3. Assert zero failures
    Expected Result: All tests pass with identical count to baseline
    Failure Indicators: Any "FAILED" in output, test count mismatch
    Evidence: .sisyphus/evidence/task-1-security-policy-tests.txt

  Scenario: Production code unchanged
    Tool: Bash
    Preconditions: mod.rs modified
    Steps:
      1. Run: cargo clippy -- -D warnings 2>&1 | grep 'security/policy'
      2. Run: cargo check --lib 2>&1
    Expected Result: Zero clippy warnings, successful compilation
    Evidence: .sisyphus/evidence/task-1-clippy-check.txt
  ```

  **Commit**: YES (groups with T2-T5)
  - Message: `refactor(tests): extract test modules from fat source files`
  - Files: `src/security/policy/mod.rs`, `src/security/policy/tests.rs`
  - Pre-commit: `cargo test --lib -- security::policy`

- [ ] 2. Extract tests from gateway/mod.rs → tests.rs

  **What to do**:
  - Move the `#[cfg(test)] mod tests { ... }` block (~655 lines, 28 tests) from `src/gateway/mod.rs` to `src/gateway/tests.rs`
  - In mod.rs, replace with `#[cfg(test)] mod tests;`
  - Handle the `#[cfg(test)] use defense::apply_external_ingress_policy;` import on line 23 — move it into tests.rs
  - Verify all test helper functions (`make_test_state`, `make_whatsapp_state`, `compute_whatsapp_signature_*`) move correctly
  - Run `cargo test --lib -- gateway::tests` to verify all 28 tests pass

  **Must NOT do**:
  - Do not change gateway production logic (run_gateway, run_gateway_with_listener)
  - Do not change AppState struct or WebhookBody/WhatsAppVerifyQuery

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3, 4, 5)
  - **Blocks**: Tasks 13 (module decomposition)
  - **Blocked By**: None

  **References**:
  - `src/gateway/mod.rs` — Production ends ~line 272, tests start ~line 274. The file is 70% test code.
  - `src/gateway/mod.rs:22-23` — `#[cfg(test)] use defense::apply_external_ingress_policy;` — this test-only import must move to tests.rs

  **Acceptance Criteria**:
  - [ ] `src/gateway/tests.rs` exists with all 28 tests
  - [ ] `src/gateway/mod.rs` reduced to ~272 production lines
  - [ ] `cargo test --lib -- gateway::tests` → all 28 tests pass

  **QA Scenarios**:
  ```
  Scenario: All gateway tests pass after extraction
    Tool: Bash
    Steps:
      1. Run: cargo test --lib -- gateway::tests 2>&1
      2. Assert "test result: ok" and 28 passed
    Expected Result: All gateway tests pass
    Evidence: .sisyphus/evidence/task-2-gateway-tests.txt

  Scenario: Feature matrix compilation
    Tool: Bash
    Steps:
      1. Run: cargo check --lib && cargo check --bin asteroniris
      2. Run: cargo check --no-default-features
    Expected Result: All compile successfully
    Evidence: .sisyphus/evidence/task-2-feature-check.txt
  ```

  **Commit**: YES (groups with T1, T3-T5)
  - Message: `refactor(tests): extract test modules from fat source files`
  - Files: `src/gateway/mod.rs`, `src/gateway/tests.rs`

- [ ] 3. Extract tests from security/secrets.rs → security/secrets_tests.rs

  **What to do**:
  - Move the `#[cfg(test)] mod tests { ... }` block (~574 lines, 40 tests) from `src/security/secrets.rs` to `src/security/secrets_tests.rs`
  - In secrets.rs, replace with `#[cfg(test)] mod secrets_tests;` (or use `#[cfg(test)] #[path = "secrets_tests.rs"] mod tests;` to preserve module path)
  - Run security regression gate after changes

  **Must NOT do**:
  - Do not change any encryption/decryption logic
  - Do not rename test functions

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 4, 5)
  - **Blocks**: None directly
  - **Blocked By**: None

  **References**:
  - `src/security/secrets.rs` — Production ~281 lines, tests ~574 lines (67% tests)
  - Security regression gate commands in Verification Strategy section

  **Acceptance Criteria**:
  - [ ] Tests extracted, 40 tests pass
  - [ ] `cargo test --lib -- security::secrets` → all pass
  - [ ] Security regression gate passes

  **QA Scenarios**:
  ```
  Scenario: Security secrets tests and regression gate
    Tool: Bash
    Steps:
      1. Run: cargo test --lib -- security::secrets 2>&1
      2. Run: cargo test --test agent -- action_intent 2>&1
      3. Run: cargo test --test memory -- governance 2>&1
    Expected Result: All pass
    Evidence: .sisyphus/evidence/task-3-secrets-tests.txt
  ```

  **Commit**: YES (groups with T1-T2, T4-T5)

- [ ] 4. Extract tests from providers/mod.rs → providers/tests.rs

  **What to do**:
  - Move the `#[cfg(test)] mod tests { ... }` block (~444 lines, 54 tests) to `src/providers/tests.rs`
  - In mod.rs, replace with `#[cfg(test)] mod tests;`
  - Verify all provider factory tests pass

  **Must NOT do**:
  - Do not change provider factory logic or secret scrubbing

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1-3, 5)
  - **Blocks**: Task 13 (providers decomposition)
  - **Blocked By**: None

  **References**:
  - `src/providers/mod.rs` — Production ~519 lines, tests ~444 lines (46% tests)

  **Acceptance Criteria**:
  - [ ] `src/providers/tests.rs` exists with 54 tests
  - [ ] `cargo test --lib -- providers::tests` → all pass

  **QA Scenarios**:
  ```
  Scenario: Provider factory tests pass
    Tool: Bash
    Steps:
      1. Run: cargo test --lib -- providers::tests 2>&1
      2. Assert "test result: ok" and 54 passed
    Expected Result: All pass
    Evidence: .sisyphus/evidence/task-4-providers-tests.txt
  ```

  **Commit**: YES (groups with T1-T3, T5)

- [ ] 5. Extract tests from skills/mod.rs → skills/tests.rs

  **What to do**:
  - Move the `#[cfg(test)] mod tests { ... }` block (~307 lines, 18 tests) to `src/skills/tests.rs`
  - In mod.rs, replace with `#[cfg(test)] mod tests;`

  **Must NOT do**:
  - Do not change skill loading or discovery logic

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1-4)
  - **Blocks**: Task 15 (skills decomposition)
  - **Blocked By**: None

  **References**:
  - `src/skills/mod.rs` — Production ~601 lines, tests ~307 lines (34% tests)

  **Acceptance Criteria**:
  - [ ] `src/skills/tests.rs` exists with 18 tests
  - [ ] `cargo test --lib -- skills::tests` → all pass

  **QA Scenarios**:
  ```
  Scenario: Skills tests pass
    Tool: Bash
    Steps:
      1. Run: cargo test --lib -- skills::tests 2>&1
    Expected Result: All 18 pass
    Evidence: .sisyphus/evidence/task-5-skills-tests.txt
  ```

  **Commit**: YES (groups with T1-T4)

---

### Wave 2: Code Deduplication (mostly parallel, after Wave 1)

- [ ] 6. Extract shared channel factory function

  **What to do**:
  - Create `src/channels/factory.rs` with a shared function that constructs channel instances from config
  - The function should accept `&ChannelsConfig` and return `Vec<Arc<dyn Channel>>` (or `Vec<(&'static str, Arc<dyn Channel>)>` for doctor which needs names)
  - Replace the duplicated construction code in `doctor_channels()` (~lines 129-213) and `start_channels()` (~lines 343-408) with calls to the shared factory
  - Handle the nuance: `doctor_channels` needs `Vec<(&'static str, Arc<dyn Channel>)>` while `start_channels` needs `Vec<Arc<dyn Channel>>`
  - Add `pub mod factory;` to `src/channels/mod.rs`
  - Ensure all channel types (Telegram, Discord, Slack, iMessage, Matrix, WhatsApp, Email, IRC) are constructed by the factory

  **Must NOT do**:
  - Do not change channel behavior
  - Do not change channel constructor signatures
  - Do not remove the `.clone()` calls on config fields — channels need owned strings. The win is eliminating the duplicated LOGIC, not the field clones.
  - Do not change config TOML format

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []
    - Moderate complexity: Must understand the slight differences between doctor and start channel construction

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 7, 8, 9)
  - **Blocks**: Task 10 (Arc<Config> migration touches channel startup)
  - **Blocked By**: None (but Wave 1 should complete first for clean files)

  **References**:
  - `src/channels/mod.rs:126-213` — `doctor_channels()` channel construction block
  - `src/channels/mod.rs:343-408` — `start_channels()` channel construction block (near-identical)
  - `src/channels/traits.rs` — Channel trait definition
  - Each channel constructor: `TelegramChannel::new()`, `DiscordChannel::new()`, etc.

  **Acceptance Criteria**:
  - [ ] `src/channels/factory.rs` exists
  - [ ] `doctor_channels()` and `start_channels()` both use the shared factory
  - [ ] ~170 lines of duplication eliminated
  - [ ] `cargo test --lib -- channels` → all pass
  - [ ] Feature Matrix Gate passes (email feature gate must be handled)

  **QA Scenarios**:
  ```
  Scenario: Channel factory produces all channels
    Tool: Bash
    Steps:
      1. Run: cargo test --lib -- channels 2>&1
      2. Run: cargo check --no-default-features 2>&1
      3. Run: cargo check --features "email" 2>&1
    Expected Result: All pass, factory works with and without email feature
    Evidence: .sisyphus/evidence/task-6-channel-factory.txt

  Scenario: No duplication remains
    Tool: Bash
    Steps:
      1. Grep for the old pattern: grep -n 'TelegramChannel::new' src/channels/mod.rs
      2. Expect exactly 0 occurrences in mod.rs (moved to factory.rs)
      3. Grep factory.rs for TelegramChannel::new — expect exactly 1
    Expected Result: Single source of truth for channel construction
    Evidence: .sisyphus/evidence/task-6-dedup-verify.txt
  ```

  **Commit**: YES (groups with T7)
  - Message: `refactor(channels): deduplicate channel factory and tool descriptions`
  - Files: `src/channels/mod.rs`, `src/channels/factory.rs`

- [ ] 7. Extract shared tool descriptions constant

  **What to do**:
  - Create a shared constant or function for the tool descriptions Vec that's duplicated between `agent/loop_/mod.rs:532-557` and `channels/mod.rs:295-320`
  - Best location: `src/tools/mod.rs` — add a `pub fn tool_descriptions(browser_enabled: bool, composio_enabled: bool) -> Vec<(&'static str, &'static str)>`
  - Replace both inline `tool_descs` constructions with calls to this shared function
  - Handle the difference: agent loop additionally includes `composio` conditionally, channels does not

  **Must NOT do**:
  - Do not change tool descriptions text
  - Do not change when browser/composio tools are included

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 6, 8, 9)
  - **Blocks**: Task 10 (shared function needs stable before Config changes)
  - **Blocked By**: None

  **References**:
  - `src/agent/loop_/mod.rs:532-557` — Tool descriptions in agent loop
  - `src/channels/mod.rs:295-320` — Tool descriptions in channel startup
  - `src/tools/mod.rs` — Existing tool module (natural home for descriptions)

  **Acceptance Criteria**:
  - [ ] Single source of truth for tool descriptions
  - [ ] Both call sites use the shared function
  - [ ] `cargo test` → all pass

  **QA Scenarios**:
  ```
  Scenario: Tool descriptions shared correctly
    Tool: Bash
    Steps:
      1. Grep for old inline pattern: grep -c '"shell",' src/agent/loop_/mod.rs src/channels/mod.rs
      2. Expect 0 in both (moved to tools/mod.rs)
      3. Run: cargo test 2>&1
    Expected Result: Zero inline tool_descs, all tests pass
    Evidence: .sisyphus/evidence/task-7-tool-descs.txt
  ```

  **Commit**: YES (groups with T6)

- [ ] 8. Fix production unwrap() calls

  **What to do**:
  - Fix `src/skills/mod.rs:549` — `.is_ok() && .unwrap()` anti-pattern → replace with `if let Ok(output) = junction_result { ... }`
  - Fix `src/channels/email_channel.rs:432` — `Mutex::lock().unwrap()` → replace with `lock().unwrap_or_else(|e| e.into_inner())` for poisoning resilience (add comment explaining why)
  - Verify exact line numbers by reading the files first (line numbers may have shifted from test extraction)

  **Must NOT do**:
  - Do not touch unwrap() in test code — acceptable per AGENTS.md
  - Do not audit all 849 unwrap() calls — only these 2-3 specific instances
  - Do not change behavior — just make error handling more robust

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2
  - **Blocks**: None
  - **Blocked By**: None

  **References**:
  - `src/skills/mod.rs:549` — `.is_ok() && .unwrap()` anti-pattern
  - `src/channels/email_channel.rs:432` — `Mutex::lock().unwrap()` (only Mutex in codebase)
  - `AGENTS.md` — "No `unwrap()` or `expect()` in production code"

  **Acceptance Criteria**:
  - [ ] Zero `.unwrap()` in production code paths of skills/mod.rs and email_channel.rs
  - [ ] `cargo test` → all pass
  - [ ] `cargo clippy -- -D warnings` → clean

  **QA Scenarios**:
  ```
  Scenario: No production unwrap remains
    Tool: Bash
    Steps:
      1. Run: grep -n '\.unwrap()' src/skills/mod.rs | grep -v '#\[cfg(test)\]' | grep -v 'mod tests'
      2. Run: grep -n '\.unwrap()' src/channels/email_channel.rs | grep -v '#\[cfg(test)\]'
      3. Run: cargo test 2>&1
    Expected Result: No production unwrap() found, tests pass
    Evidence: .sisyphus/evidence/task-8-unwrap-fix.txt
  ```

  **Commit**: YES
  - Message: `fix(skills,email): remove production unwrap calls`

- [ ] 9. Clean up email feature-gate stub

  **What to do**:
  - Review the `#[cfg(not(feature = "email"))]` stub in `src/channels/mod.rs:6-24` that inline-defines `EmailConfig` with 11 fields
  - Verify field parity with the real `EmailConfig` in `src/channels/email_channel.rs`
  - If fields match: add a comment documenting this is a feature-gate stub and must stay in sync
  - If fields diverge: fix the stub to match the real struct
  - Consider whether this can be improved by moving `EmailConfig` to a shared location that doesn't depend on the email feature

  **Must NOT do**:
  - Do not add the email dependency to non-email builds
  - Do not change EmailConfig behavior

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2
  - **Blocks**: None
  - **Blocked By**: None

  **References**:
  - `src/channels/mod.rs:6-24` — Feature-gated email stub
  - `src/channels/email_channel.rs` — Real EmailConfig definition
  - Metis finding: "The email stub defines EmailConfig with 11 fields. If the real EmailConfig adds a field, the stub silently diverges."

  **Acceptance Criteria**:
  - [ ] EmailConfig stub fields match real EmailConfig
  - [ ] `cargo check --no-default-features` → passes
  - [ ] `cargo check --features "email"` → passes

  **QA Scenarios**:
  ```
  Scenario: Feature gate compilation
    Tool: Bash
    Steps:
      1. Run: cargo check --no-default-features 2>&1
      2. Run: cargo check --features "email" 2>&1
    Expected Result: Both pass
    Evidence: .sisyphus/evidence/task-9-email-stub.txt
  ```

  **Commit**: YES (groups with T8)

---

### Wave 3: Arc<Config> Migration (sequential core, after Wave 2)

- [ ] 10. Introduce Arc<Config> at daemon/gateway/channels entry points

  **What to do**:
  - In `src/daemon/mod.rs`: wrap Config in `Arc<Config>` at the top of the daemon entry point, replace 5+ `config.clone()` with `Arc::clone(&config)`
  - In `src/gateway/mod.rs::run_gateway()` and `run_gateway_with_listener()`: change `config: Config` parameter to `config: Arc<Config>` (or accept `Config` and Arc-wrap internally)
  - In `src/channels/mod.rs::start_channels()` and `doctor_channels()`: same Arc<Config> treatment
  - Decide per-function: entry points take `Arc<Config>`, leaf functions that only read should take `&Config` (deref through Arc)
  - Update all callers — primarily `src/main.rs` and `src/daemon/mod.rs`

  **Must NOT do**:
  - Do not change Config struct definition
  - Do not change Config TOML serialization/deserialization
  - Do not add Arc<Config> to functions that only read config once (use `&Config` via deref)
  - Do not change function behavior

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []
    - Requires careful analysis of ownership patterns across multiple modules

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential (must complete before T11)
  - **Blocks**: Task 11 (agent loop), Task 12 (integration tests), Tasks 13-17 (decomposition)
  - **Blocked By**: Tasks 6, 7 (shared functions must be stable first)

  **References**:
  - `src/daemon/mod.rs:26-81` — 5+ Config clones at daemon startup
  - `src/gateway/mod.rs:91-272` — run_gateway and run_gateway_with_listener take owned Config
  - `src/channels/mod.rs:126,265` — doctor_channels and start_channels take owned Config
  - `src/main.rs` — Primary caller of all entry points
  - Metis finding: "Config struct has 16 fields including nested sub-configs with PathBuf, Vec<String>, and Option<String> — non-trivial allocation per clone"

  **Acceptance Criteria**:
  - [ ] daemon/mod.rs uses `Arc<Config>` — zero full Config clones
  - [ ] gateway entry functions accept `Arc<Config>` or `&Config`
  - [ ] channels entry functions accept `Arc<Config>` or `&Config`
  - [ ] `cargo test` → all pass
  - [ ] Feature Matrix Gate passes

  **QA Scenarios**:
  ```
  Scenario: Daemon uses Arc<Config> without full clones
    Tool: Bash
    Steps:
      1. Run: grep -n 'config\.clone()' src/daemon/mod.rs
      2. Assert zero matches (should be Arc::clone or &config)
      3. Run: cargo test 2>&1
    Expected Result: No config.clone() in daemon, all tests pass
    Evidence: .sisyphus/evidence/task-10-arc-config.txt

  Scenario: Full build and feature gate check
    Tool: Bash
    Steps:
      1. Run: cargo check --lib && cargo check --bin asteroniris
      2. Run: cargo check --no-default-features
      3. Run: cargo check --features "email,vector-search,tui,media"
    Expected Result: All compile
    Evidence: .sisyphus/evidence/task-10-feature-check.txt
  ```

  **Commit**: YES
  - Message: `refactor(config): migrate to Arc<Config> at entry points`

- [ ] 11. Propagate &Config / Arc<Config> through agent loop

  **What to do**:
  - In `src/agent/loop_/mod.rs::run()`: Accept `Arc<Config>` (or `Config` and Arc-wrap internally) instead of owned `Config`
  - Eliminate `config.workspace_dir.clone()` calls by passing `&Path` where workspace_dir is needed
  - Eliminate `config.persona.clone()` in test code or use Arc for persona config
  - In `execute_main_session_turn_*` functions: verify they take `&Config` (already do), no changes needed
  - Update `memory::enqueue_consolidation_task` to take `&Path` instead of owned `PathBuf` for workspace_dir (line 447)

  **Must NOT do**:
  - Do not change the agent conversation loop logic
  - Do not change how providers are called

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential (after T10, before T12)
  - **Blocks**: Task 12, Task 19
  - **Blocked By**: Task 10

  **References**:
  - `src/agent/loop_/mod.rs:466-651` — `run()` function taking owned Config
  - `src/agent/loop_/mod.rs:447` — `config.workspace_dir.clone()` in consolidation
  - `src/memory/consolidation.rs` — `enqueue_consolidation_task` signature

  **Acceptance Criteria**:
  - [ ] agent `run()` no longer takes owned Config or creates unnecessary clones
  - [ ] `workspace_dir.clone()` eliminated where possible
  - [ ] `cargo test` → all pass (including integration tests: `cargo test --test agent`)

  **QA Scenarios**:
  ```
  Scenario: Agent integration tests pass
    Tool: Bash
    Steps:
      1. Run: cargo test --test agent 2>&1
      2. Run: cargo test --lib -- agent 2>&1
    Expected Result: All pass
    Evidence: .sisyphus/evidence/task-11-agent-config.txt
  ```

  **Commit**: YES (groups with T10)

- [ ] 12. Update integration tests for Config reference changes

  **What to do**:
  - Review all 6 integration test binaries (`tests/agent.rs`, `tests/gateway.rs`, `tests/memory.rs`, `tests/persona.rs`, `tests/runtime.rs`, `tests/project.rs`) for Config usage
  - Update any test code that creates `Config` and passes it owned where signatures now expect `Arc<Config>` or `&Config`
  - Use `#[path = "..."]` attribute pattern for test module includes (per AGENTS.md)
  - Verify all integration tests pass

  **Must NOT do**:
  - Do not change test assertions or expected behavior
  - Do not skip any integration tests

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential (after T11)
  - **Blocks**: None directly (but Wave 4 should start after this)
  - **Blocked By**: Task 11

  **References**:
  - `tests/agent.rs`, `tests/gateway.rs`, `tests/memory.rs`, `tests/persona.rs`, `tests/runtime.rs`, `tests/project.rs`
  - `AGENTS.md` — "Integration test routers use explicit `#[path = "subdir/file.rs"]` attributes"

  **Acceptance Criteria**:
  - [ ] `cargo test --test agent` → pass
  - [ ] `cargo test --test gateway` → pass
  - [ ] `cargo test --test memory` → pass
  - [ ] `cargo test --test persona` → pass
  - [ ] `cargo test --test runtime` → pass
  - [ ] `cargo test --test project` → pass

  **QA Scenarios**:
  ```
  Scenario: All integration tests pass
    Tool: Bash
    Steps:
      1. Run: cargo test --tests 2>&1
      2. Assert "test result: ok" for all 6 binaries
    Expected Result: All integration tests pass
    Evidence: .sisyphus/evidence/task-12-integration-tests.txt
  ```

  **Commit**: YES (groups with T10-T11)

---

### Wave 4: Module Decomposition (parallel, after Wave 3)

- [ ] 13. Decompose providers/mod.rs → factory.rs + scrub.rs

  **What to do**:
  - Extract `create_provider()` (the 174-line match statement) and all provider creation functions into `src/providers/factory.rs`
  - Extract `scrub_secret_patterns()`, `sanitize_api_error()`, `scrub_after_marker()`, `is_secret_char()`, `token_end()`, `api_error()` into `src/providers/scrub.rs`
  - Extract `resolve_api_key()` into factory.rs alongside create_provider
  - Keep `mod.rs` as thin facade: `pub mod` declarations + `pub use` re-exports
  - **CRITICAL**: Preserve ALL existing `pub use` re-exports — add new ones, never remove
  - **CRITICAL**: Verify dual crate root: `cargo check --lib && cargo check --bin asteroniris`

  **Must NOT do**:
  - Do not change any function signatures or behavior
  - Do not remove any `pub use` re-exports from mod.rs
  - Do not rename any public types/functions

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4 (with Tasks 14-17)
  - **Blocks**: Task 18 (Cow<str> implementation in scrub.rs)
  - **Blocked By**: Tasks 4 (tests extracted), 10 (Arc<Config> stable)

  **References**:
  - `src/providers/mod.rs:199-373` — create_provider match statement (174 lines)
  - `src/providers/mod.rs:22-108` — scrub functions
  - `src/providers/mod.rs:146-195` — resolve_api_key
  - `AGENTS.md` — "mod.rs is a thin facade with `pub mod` + `pub use` re-exports"
  - Metis directive: "Facade mod.rs files MUST preserve all existing pub use re-exports"

  **Acceptance Criteria**:
  - [ ] `src/providers/factory.rs` exists with create_provider + related functions
  - [ ] `src/providers/scrub.rs` exists with scrub functions
  - [ ] `src/providers/mod.rs` is thin facade (re-exports only, <50 lines)
  - [ ] `cargo test --lib -- providers` → all pass
  - [ ] `cargo check --lib && cargo check --bin asteroniris` → both pass
  - [ ] Feature Matrix Gate passes

  **QA Scenarios**:
  ```
  Scenario: Providers decomposition preserves behavior
    Tool: Bash
    Steps:
      1. Run: cargo test --lib -- providers 2>&1
      2. Run: cargo check --lib && cargo check --bin asteroniris 2>&1
      3. Run: wc -l src/providers/mod.rs (should be <50)
    Expected Result: All tests pass, mod.rs is thin, both crate roots compile
    Evidence: .sisyphus/evidence/task-13-providers-decompose.txt
  ```

  **Commit**: YES (groups with T14-T17)
  - Message: `refactor(providers,security,skills): decompose fat mod.rs into sub-modules`

- [ ] 14. Decompose security/policy/mod.rs → enforcement sub-module

  **What to do**:
  - After test extraction (Task 1), assess remaining production code (~138 lines)
  - If still large: extract SecurityPolicy impl methods into `src/security/policy/enforcement.rs`
  - Keep mod.rs as thin facade with struct definition + re-exports
  - **CRITICAL**: Run security regression gate after changes
  - **CRITICAL**: All 26 integration test import paths must still work

  **Must NOT do**:
  - Do not change SecurityPolicy behavior
  - Do not change deny-by-default allowlist logic
  - Do not remove pub use re-exports

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4
  - **Blocks**: None
  - **Blocked By**: Task 1 (tests extracted), Task 10

  **References**:
  - `src/security/policy/mod.rs` — After test extraction, ~138 lines remain
  - Security regression gate commands in Verification Strategy
  - Metis: "26 integration test files import via deep paths like `asteroniris::security::policy::TenantPolicyContext`"

  **Acceptance Criteria**:
  - [ ] mod.rs is thin facade
  - [ ] Security regression gate passes
  - [ ] All integration tests pass

  **QA Scenarios**:
  ```
  Scenario: Security regression gate
    Tool: Bash
    Steps:
      1. Run all security regression gate commands (see Verification Strategy)
      2. Run: cargo test --tests 2>&1
    Expected Result: All pass
    Evidence: .sisyphus/evidence/task-14-security-decompose.txt
  ```

  **Commit**: YES (groups with T13)

- [ ] 15. Decompose skills/mod.rs → loader.rs

  **What to do**:
  - Extract `load_skills()` and related loading logic into `src/skills/loader.rs`
  - Keep mod.rs as thin facade
  - Verify dual crate root (skills is in BOTH main.rs and lib.rs module sets)

  **Must NOT do**:
  - Do not change skill loading behavior
  - Do not remove pub use re-exports

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4
  - **Blocks**: None
  - **Blocked By**: Task 5 (tests extracted), Task 10

  **References**:
  - `src/skills/mod.rs:75-395` — load_skills and formatting logic (~320 production lines after test extraction)

  **Acceptance Criteria**:
  - [ ] mod.rs is thin facade
  - [ ] `cargo test --lib -- skills` → all pass
  - [ ] Dual crate root check passes

  **QA Scenarios**:
  ```
  Scenario: Skills decomposition
    Tool: Bash
    Steps:
      1. Run: cargo test --lib -- skills 2>&1
      2. Run: cargo check --lib && cargo check --bin asteroniris 2>&1
    Expected Result: All pass
    Evidence: .sisyphus/evidence/task-15-skills-decompose.txt
  ```

  **Commit**: YES (groups with T13-T14)

- [ ] 16. Split channels/imessage.rs into sub-modules

  **What to do**:
  - `src/channels/imessage.rs` is 933 LOC in a single file
  - Create `src/channels/imessage/` directory with: `mod.rs` (thin facade), `handler.rs` (message handling), `auth.rs` (AppleScript/auth logic)
  - Move tests to `src/channels/imessage/tests.rs` (42 tests)
  - Preserve `pub use` of `IMessageChannel` from `channels/mod.rs`

  **Must NOT do**:
  - Do not change iMessage behavior
  - Do not break `channels/mod.rs` re-export of `IMessageChannel`

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4
  - **Blocks**: None
  - **Blocked By**: Task 10

  **References**:
  - `src/channels/imessage.rs` — 933 LOC, 42 tests
  - `src/channels/mod.rs:40` — `pub use imessage::IMessageChannel;`

  **Acceptance Criteria**:
  - [ ] `src/channels/imessage/` directory with proper sub-modules
  - [ ] All 42 tests pass
  - [ ] `channels::IMessageChannel` still accessible

  **QA Scenarios**:
  ```
  Scenario: iMessage channel split
    Tool: Bash
    Steps:
      1. Run: cargo test --lib -- channels::imessage 2>&1
      2. Run: cargo check --lib 2>&1
    Expected Result: All pass
    Evidence: .sisyphus/evidence/task-16-imessage-split.txt
  ```

  **Commit**: YES (groups with T17)
  - Message: `refactor(channels): split imessage and telegram into sub-modules`

- [ ] 17. Split channels/telegram.rs into sub-modules

  **What to do**:
  - `src/channels/telegram.rs` is 836 LOC in a single file
  - Create `src/channels/telegram/` directory with: `mod.rs`, `handler.rs`, `api.rs`
  - Move tests to `src/channels/telegram/tests.rs` (16 tests)
  - Preserve `pub use` of `TelegramChannel` from `channels/mod.rs`

  **Must NOT do**:
  - Same guardrails as Task 16

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4 (with T16)
  - **Blocks**: None
  - **Blocked By**: Task 10

  **References**:
  - `src/channels/telegram.rs` — 836 LOC, 16 tests
  - `src/channels/mod.rs:48` — `pub use telegram::TelegramChannel;`

  **Acceptance Criteria**:
  - [ ] `src/channels/telegram/` directory with proper sub-modules
  - [ ] All 16 tests pass

  **QA Scenarios**:
  ```
  Scenario: Telegram channel split
    Tool: Bash
    Steps:
      1. Run: cargo test --lib -- channels::telegram 2>&1
      2. Run: cargo check --lib 2>&1
    Expected Result: All pass
    Evidence: .sisyphus/evidence/task-17-telegram-split.txt
  ```

  **Commit**: YES (groups with T16)

---

### Wave 5: Data Flow Optimization (parallel, after Wave 4)

- [ ] 18. Implement Cow<str> for scrub_secret_patterns()

  **What to do**:
  - In `src/providers/scrub.rs` (after Task 13 decomposition):
  - Refactor `scrub_after_marker()` to return `bool` indicating whether it modified the string
  - Add a `needs_scrubbing()` fast-path check that scans for any prefix/marker pattern WITHOUT allocating
  - Change `scrub_secret_patterns()` signature to return `Cow<'_, str>`:
    - If no patterns found: return `Cow::Borrowed(input)` (zero allocation)
    - If patterns found: allocate once, mutate, return `Cow::Owned(scrubbed)`
  - Update `sanitize_api_error()` to work with `Cow<str>` return
  - Update all 5 call sites to handle `Cow<str>`

  **Must NOT do**:
  - Do not change what gets scrubbed — same patterns, same behavior
  - Do not break any scrub_* test assertions

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []
    - Non-trivial: helper function mutation tracking, Cow lifetime management

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 5 (with Tasks 19, 20)
  - **Blocks**: None
  - **Blocked By**: Task 13 (scrub.rs must exist)

  **References**:
  - `src/providers/scrub.rs` (after T13) — scrub_secret_patterns, scrub_after_marker, sanitize_api_error
  - Metis: "A Cow implementation needs the helper to return whether it modified anything. scrub_after_marker must return bool."
  - 5 production call sites including `gateway/handlers.rs:179`

  **Acceptance Criteria**:
  - [ ] `scrub_secret_patterns()` returns `Cow<'_, str>`
  - [ ] Zero allocation when no secrets found (verified by test)
  - [ ] All 54 provider tests pass
  - [ ] All scrub/sanitize assertions unchanged

  **QA Scenarios**:
  ```
  Scenario: Zero-allocation fast path works
    Tool: Bash
    Steps:
      1. Run: cargo test --lib -- providers 2>&1
      2. Assert all sanitize/scrub tests pass with same assertions
    Expected Result: All tests pass, behavior identical
    Evidence: .sisyphus/evidence/task-18-cow-scrub.txt

  Scenario: Scrubbing still catches all patterns
    Tool: Bash
    Steps:
      1. Run: cargo test --lib -- providers::tests::sanitize 2>&1
      2. Assert all pattern tests pass (sk-, xoxb-, Bearer, api_key=, etc.)
    Expected Result: All pattern tests pass
    Evidence: .sisyphus/evidence/task-18-scrub-patterns.txt
  ```

  **Commit**: YES
  - Message: `perf(providers): Cow<str> for scrub_secret_patterns zero-alloc fast path`

- [ ] 19. Reduce Arc cloning in agent loop hot path

  **What to do**:
  - In `src/agent/loop_/mod.rs`: Review all `mem.clone()` calls (6 instances)
  - Where `Arc<dyn Memory>` is passed to functions that only need `&dyn Memory`, change to pass `mem.as_ref()`
  - Where Arc clone is genuinely needed (spawned tasks, closures), keep `Arc::clone(&mem)` with explicit comment
  - Review `observer.clone()` — if observer is only used for recording, pass `&Arc<dyn Observer>` instead
  - Reduce `config.persona.state_mirror_filename.clone()` — pass `&str` instead

  **Must NOT do**:
  - Do not change agent conversation loop behavior
  - Do not remove Arc where ownership transfer is needed (spawned tasks)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 5 (with Tasks 18, 20)
  - **Blocks**: None
  - **Blocked By**: Task 11 (Config migration stable)

  **References**:
  - `src/agent/loop_/mod.rs:181,397,446,498,601,628` — 6 `mem.clone()` instances
  - `src/agent/loop_/mod.rs:449` — `observer.clone()`
  - `src/agent/loop_/mod.rs:572` — `config.persona.state_mirror_filename.clone()`

  **Acceptance Criteria**:
  - [ ] Unnecessary Arc clones eliminated (pass reference where possible)
  - [ ] Remaining Arc clones are `Arc::clone()` with comments explaining why
  - [ ] `cargo test --test agent` → all pass
  - [ ] `cargo test --lib -- agent` → all pass

  **QA Scenarios**:
  ```
  Scenario: Agent tests still pass after clone reduction
    Tool: Bash
    Steps:
      1. Run: cargo test --test agent 2>&1
      2. Run: cargo test --lib -- agent 2>&1
    Expected Result: All pass
    Evidence: .sisyphus/evidence/task-19-agent-clones.txt
  ```

  **Commit**: YES
  - Message: `perf(agent,tools): reduce hot-path cloning`

- [ ] 20. Optimize tool registry creation

  **What to do**:
  - In `src/tools/mod.rs::all_tools()`: Replace repeated `security.clone()` with explicit `Arc::clone(security)` (stylistic but signals intent)
  - In `src/tools/mod.rs::all_tools()`: Move `browser_config.allowed_domains.clone()` and `browser_config.session_name.clone()` to only execute when browser is enabled (already partially done, verify)
  - Consider changing `all_tools()` to take `&Arc<SecurityPolicy>` instead of `&Arc<SecurityPolicy>` (it already takes reference — verify no unnecessary clones)

  **Must NOT do**:
  - Do not change tool behavior or registration order

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 5
  - **Blocks**: None
  - **Blocked By**: Tasks 13 (decomposition stable)

  **References**:
  - `src/tools/mod.rs:51-82` — all_tools function with 8+ security.clone() calls

  **Acceptance Criteria**:
  - [ ] Arc::clone used explicitly for clarity
  - [ ] `cargo test --lib -- tools` → all pass

  **QA Scenarios**:
  ```
  Scenario: Tool registry tests pass
    Tool: Bash
    Steps:
      1. Run: cargo test --lib -- tools 2>&1
    Expected Result: All pass
    Evidence: .sisyphus/evidence/task-20-tools-optimize.txt
  ```

  **Commit**: YES (groups with T19)

---

### Wave 6: Cleanup & Polish (parallel, after Wave 5)

- [ ] 21. Audit and resolve dead_code suppressions in memory/*

  **What to do**:
  - Review all 16 `#[allow(dead_code)]` suppressions across:
    - `src/memory/sqlite/codec.rs` (2 suppressions)
    - `src/memory/sqlite/search.rs` (2 suppressions)
    - `src/memory/sqlite/repository.rs` (3 suppressions)
    - Other memory/* files
  - For each suppression: determine if the item is:
    - (a) Genuinely unused → remove the item entirely
    - (b) Used in SQL string interpolation or serde → keep suppression, add comment explaining why
    - (c) Used only by feature-gated code → add proper `#[cfg(feature = "...")]` instead of dead_code
  - Run `cargo test --test memory` to verify

  **Must NOT do**:
  - Do not remove items used in SQL string interpolation without verifying
  - Do not break any memory backend behavior

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 6 (with Tasks 22, 23)
  - **Blocks**: None
  - **Blocked By**: Wave 5 complete

  **References**:
  - `src/memory/sqlite/codec.rs`, `search.rs`, `repository.rs` — dead_code suppressions
  - `src/memory/lancedb/mod.rs` — 1 suppression
  - `src/memory/markdown.rs` — 2 suppressions

  **Acceptance Criteria**:
  - [ ] Each dead_code suppression resolved: item removed OR comment added explaining why kept
  - [ ] dead_code suppression count reduced (track before/after)
  - [ ] `cargo test --test memory` → all pass
  - [ ] `cargo check --features "vector-search"` → pass

  **QA Scenarios**:
  ```
  Scenario: Memory tests pass after dead code audit
    Tool: Bash
    Steps:
      1. Run: grep -rn '#\[allow(dead_code)\]' src/memory/ | wc -l (before count)
      2. Run: cargo test --test memory 2>&1
      3. Run: cargo check --features "vector-search" 2>&1
    Expected Result: Count reduced, tests pass
    Evidence: .sisyphus/evidence/task-21-dead-code.txt
  ```

  **Commit**: YES
  - Message: `refactor(memory): resolve dead_code suppressions`

- [ ] 22. Test optimization: extract/deduplicate test helpers

  **What to do**:
  - Identify common test patterns across the extracted test files:
    - `TempDir::new().unwrap()` + MemoryConfig creation (repeated in 10+ test files)
    - `SecurityPolicy::default()` creation (repeated in 8+ test files)
    - `CountingProvider` mock (duplicated in gateway tests, potentially useful elsewhere)
  - Create `src/test_helpers.rs` (or use `tests/support/`) for shared test utilities if patterns are truly common
  - Evaluate if any test deduplication makes sense without over-engineering
  - Review test files that are themselves large (channels/whatsapp/tests.rs at 898 LOC) for potential splits

  **Must NOT do**:
  - Do not over-engineer test infrastructure
  - Do not change test behavior or assertions
  - Do not reduce test coverage

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 6
  - **Blocks**: None
  - **Blocked By**: Wave 5

  **References**:
  - All extracted test files from Wave 1
  - `tests/support/` — existing shared test support directory
  - `src/channels/whatsapp/tests.rs` — 898 LOC test file

  **Acceptance Criteria**:
  - [ ] Common test patterns identified and shared where beneficial
  - [ ] `cargo test` → all 953+ tests pass
  - [ ] No test coverage reduction

  **QA Scenarios**:
  ```
  Scenario: Full test suite passes after optimization
    Tool: Bash
    Steps:
      1. Run: cargo test 2>&1
      2. Assert "test result: ok" and test count >= 953
    Expected Result: All tests pass
    Evidence: .sisyphus/evidence/task-22-test-optimize.txt
  ```

  **Commit**: YES
  - Message: `refactor(tests): optimize test helpers and dedup`

- [ ] 23. Remove resolved clippy suppressions (too_many_lines, too_many_arguments)

  **What to do**:
  - After all decomposition and refactoring, check if `#[allow(clippy::too_many_lines)]` annotations are still needed
  - Functions that were decomposed (like `start_channels`, `doctor_channels`, `create_provider`) may now be under the line limit
  - Check if `#[allow(clippy::too_many_arguments)]` in agent loop is resolved by the params struct pattern
  - Remove any suppressions where the underlying issue has been fixed
  - Track: baseline suppression count vs final count

  **Must NOT do**:
  - Do not add new suppressions to work around issues
  - Do not remove suppressions if the function still exceeds limits

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 6
  - **Blocks**: None
  - **Blocked By**: Wave 5

  **References**:
  - `src/channels/mod.rs:125,264` — `#[allow(clippy::too_many_lines)]` on doctor/start
  - `src/agent/loop_/mod.rs:242,270,305,465` — Multiple suppressions
  - `src/providers/mod.rs:198` — `#[allow(clippy::too_many_lines)]` on create_provider

  **Acceptance Criteria**:
  - [ ] Suppression count reduced from baseline
  - [ ] `cargo clippy -- -D warnings` → clean
  - [ ] No new suppressions added

  **QA Scenarios**:
  ```
  Scenario: Clippy suppression audit
    Tool: Bash
    Steps:
      1. Run: grep -rn 'clippy::too_many_lines\|clippy::too_many_arguments' src/ | wc -l
      2. Assert count is lower than baseline (establish baseline at start)
      3. Run: cargo clippy -- -D warnings 2>&1
    Expected Result: Count reduced, clippy clean
    Evidence: .sisyphus/evidence/task-23-clippy-cleanup.txt
  ```

  **Commit**: YES (groups with T21-T22)

---

## Final Verification Wave

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (`cargo test`, read changed files). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in .sisyphus/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo fmt -- --check` + `cargo clippy -- -D warnings` + `cargo test`. Review all changed files for: dead code, `as any`/`@ts-ignore`, empty catches, console.log in prod (N/A for Rust), commented-out code, unused imports. Check AI slop: excessive comments, over-abstraction, generic names. Verify clippy suppression count did not increase from baseline.
  Output: `Build [PASS/FAIL] | Lint [PASS/FAIL] | Tests [N pass/N fail] | Suppressions [baseline→current] | VERDICT`

- [ ] F3. **Full Regression QA** — `unspecified-high`
  Run complete verification suite from clean state:
  ```bash
  cargo fmt -- --check
  cargo clippy -- -D warnings
  cargo test
  cargo check --no-default-features
  cargo check --features "email,vector-search,tui,media"
  cargo check --lib
  cargo check --bin asteroniris
  ```
  Additionally run security regression gate commands. Save all output to `.sisyphus/evidence/final-qa/`.
  Output: `Standard [PASS/FAIL] | Features [PASS/FAIL] | Security [PASS/FAIL] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff (`git log`/`git diff`). Verify 1:1 — everything in spec was built (no missing), nothing beyond spec was built (no creep). Check "Must NOT do" compliance: no new features, no new deps, no Config TOML format changes, no security model changes. Verify health+heartbeat+doctor were NOT merged. Flag unaccounted changes.
  Output: `Tasks [N/N compliant] | Guardrails [CLEAN/N violations] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

Each wave should produce 1-2 coherent commits:

- **Wave 1**: `refactor(tests): extract test modules from fat source files`
- **Wave 2**: `refactor(channels): deduplicate channel factory and tool descriptions` + `fix(skills,email): remove production unwrap and clean feature stub`
- **Wave 3**: `refactor(config): migrate to Arc<Config> at entry points`
- **Wave 4**: `refactor(providers,security,skills): decompose fat mod.rs into sub-modules` + `refactor(channels): split imessage and telegram into sub-modules`
- **Wave 5**: `perf(providers): Cow<str> for scrub_secret_patterns` + `perf(agent,tools): reduce hot-path cloning`
- **Wave 6**: `refactor(memory): resolve dead_code suppressions` + `refactor(tests): optimize test helpers and dedup`

---

## Success Criteria

### Verification Commands
```bash
cargo fmt -- --check               # Expected: no diff
cargo clippy -- -D warnings        # Expected: 0 warnings
cargo test                         # Expected: all pass (except known inventory_scope_lock)
cargo check --no-default-features  # Expected: success
cargo check --features "email,vector-search,tui,media"  # Expected: success
cargo check --lib                  # Expected: success
cargo check --bin asteroniris      # Expected: success
```

### Final Checklist
- [ ] All "Must Have" present (tests pass, security preserved, config compat, re-exports preserved)
- [ ] All "Must NOT Have" absent (no new features, no new deps, no Config changes, no security changes)
- [ ] All 953+ tests pass
- [ ] Clippy suppression count same or lower than baseline
- [ ] Feature matrix compilation successful
- [ ] Security regression gate passes
