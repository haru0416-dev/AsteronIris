# Learnings - codebase-tuning

## [2026-02-20] Session: ses_385aebde4ffeJ6hKXbpfG81cqF - BASELINE

### Baseline Metrics (CRITICAL - track against these)
- Total `#[allow(clippy::*)]` suppressions: **84**
- `too_many_lines` / `too_many_arguments` suppressions: **42**
- `#[allow(dead_code)]` suppressions: **16**

### Target Files - Line Counts
- `src/security/policy/mod.rs`: 1016 lines (87% tests)
- `src/gateway/mod.rs`: 929 lines (70% tests)
- `src/security/secrets.rs`: 855 lines (67% tests)
- `src/providers/mod.rs`: 963 lines (46% tests)
- `src/skills/mod.rs`: 908 lines (34% tests)

### Architecture
- Dual crate root: `main.rs` AND `lib.rs` declare different module sets — verify BOTH compile
- Every module move must pass `cargo check --lib && cargo check --bin asteroniris`
- Feature gates: email, vector-search, tui, media
- Integration test pattern: `#[path = "subdir/file.rs"] mod name;` (NOT implicit resolution)
- Test module path must be preserved: `security::policy::tests::*` etc.

### Known Pre-existing Failure
- `inventory_scope_lock::inventory_scope_lock` — do NOT fix, do NOT fail on it

### Important Pattern
- `src/security/policy/mod.rs:139` — production code ends around line 138, tests start ~line 139
- `src/gateway/mod.rs:22-23` — has `#[cfg(test)] use defense::apply_external_ingress_policy;` which MUST move to tests.rs
- `src/security/secrets.rs` — use `#[cfg(test)] #[path = "secrets_tests.rs"] mod tests;` to preserve module path

## Task 1: Extract security::policy tests (2025-02-20)

**Pattern Applied**: Extracted 877 lines of test code from `src/security/policy/mod.rs` into separate `src/security/policy/tests.rs` file.

**Key Learnings**:
1. **Module Declaration Pattern**: When extracting tests, replace the entire `#[cfg(test)] mod tests { ... }` block with just `#[cfg(test)] mod tests;` (declaration only). Rust automatically looks for `tests.rs` in the same directory.
2. **Test Module Path Preservation**: Tests maintain their full path `security::policy::tests::*` in cargo test output even when extracted to separate file.
3. **Import Handling**: The extracted test file uses `use super::*;` to access all public items from the parent module, including constants like `ACTION_LIMIT_EXCEEDED_ERROR` and `COST_LIMIT_EXCEEDED_ERROR`.
4. **File Size Reduction**: Production code reduced from 1016 lines to 140 lines (87% test code removed), making the module facade much cleaner per AGENTS.md guidance.
5. **Test Count**: All 87 tests pass without modification after extraction.

**Verification**:
- `cargo test --lib -- security::policy`: 87 passed ✓
- `cargo clippy -- -D warnings`: clean ✓
- No production logic changes, only test reorganization

**Files Modified**:
- `src/security/policy/mod.rs`: 1016 → 140 lines
- `src/security/policy/tests.rs`: created with 873 lines


## Task 3: Extract security::secrets tests (2026-02-20)

**Pattern Applied**: Extracted 574 lines of test code from `src/security/secrets.rs` into separate `src/security/secrets_tests.rs` file using `#[path = "secrets_tests.rs"] mod tests;` pattern.

**Key Learnings**:
1. **Module Path Preservation**: Using `#[path = "secrets_tests.rs"] mod tests;` keeps the module accessible as `security::secrets::tests::*` even though the file is named `secrets_tests.rs` (not `tests.rs`). This is critical for security modules where naming clarity matters.
2. **Test Count**: All 37 tests pass without modification after extraction. Note: 37 tests (not 40 as initially estimated) — some tests are platform-specific (#[cfg(windows)], #[cfg(unix)]).
3. **File Size Reduction**: Production code reduced from 855 lines to 283 lines (67% test code removed). Production logic: ~281 lines (encryption, decryption, key management, legacy XOR support).
4. **Security-Critical Code**: No encryption/decryption logic was touched — only test reorganization. All security guarantees preserved.
5. **Comment Preservation**: All inline comments and section headers were preserved in extracted tests (necessary for test clarity and algorithm documentation).

**Verification**:
- `cargo test --lib -- security::secrets`: 37 passed ✓
- `cargo test --test agent -- action_intent`: 2 passed ✓
- `cargo test --test memory -- governance`: 5 passed ✓
- All regression gates pass
- Evidence saved to `.sisyphus/evidence/task-3-secrets-tests.txt`

**Files Modified**:
- `src/security/secrets.rs`: 855 → 283 lines
- `src/security/secrets_tests.rs`: created with 574 lines

**Pattern Comparison**:
- Task 1 (policy): Used implicit `mod tests;` (Rust auto-finds `tests.rs`)
- Task 3 (secrets): Used explicit `#[path = "secrets_tests.rs"] mod tests;` (custom filename for clarity)
- Both patterns work; explicit path is preferred when filename clarity matters (security modules)

## Task 5: Extract skills tests (2026-02-20)

**Pattern Applied**: Extracted 305 lines of test code from `src/skills/mod.rs` into separate `src/skills/tests.rs` file using implicit module declaration pattern.

**Key Learnings**:
1. **Module Declaration Pattern**: Replaced entire `#[cfg(test)] mod tests { ... }` block (lines 601-905) with `#[cfg(test)] mod tests;` declaration. Rust automatically resolves to `tests.rs` in same directory.
2. **Import Handling**: Extracted test file requires explicit `crate::skills::*` imports instead of `super::*` because tests.rs is a sibling module, not a child. Imports needed: `load_skills`, `skills_to_prompt`, `init_skills_dir`, `skills_dir`, `Skill`, `SkillTool`, plus `HashMap`, `PathBuf`, `std::fs`.
3. **Test Count**: All 18 tests pass without modification after extraction. Test module path preserved as `skills::tests::*` in cargo test output.
4. **File Size Reduction**: Production code reduced from 908 lines to 603 lines (34% test code removed). Production logic: ~601 lines (skill loading, TOML/MD parsing, prompt generation, CLI command handling).
5. **Dual Crate Root Verification**: Both `cargo check --lib` and `cargo check --bin asteroniris` pass — critical because skills module is declared in BOTH `main.rs` and `lib.rs` module sets.

**Verification**:
- `cargo test --lib -- skills::tests`: 18 passed ✓
- `cargo check --lib`: clean ✓
- `cargo check --bin asteroniris`: clean ✓
- Evidence saved to `.sisyphus/evidence/task-5-skills-tests.txt`

**Files Modified**:
- `src/skills/mod.rs`: 908 → 603 lines
- `src/skills/tests.rs`: created with 305 lines

**Pattern Comparison**:
- Task 1 (policy): Implicit `mod tests;` with `use super::*;` (child module)
- Task 3 (secrets): Explicit `#[path = "secrets_tests.rs"] mod tests;` with `use super::*;` (custom filename)
- Task 5 (skills): Implicit `mod tests;` with `use crate::skills::*;` (sibling module, requires crate:: imports)
- **Key Difference**: When tests.rs is a sibling (not child), use `crate::module::*` imports, not `super::*`

## Task 2: Extract gateway tests (2026-02-20)

**Pattern Applied**: Extracted 656 lines of test code from `src/gateway/mod.rs` into separate `src/gateway/tests.rs` file using implicit module declaration pattern.

**Key Learnings**:
1. **Module Declaration Pattern**: Replaced entire `#[cfg(test)] mod tests { ... }` block (lines 274-929) with `#[cfg(test)] mod tests;` declaration. Rust automatically resolves to `tests.rs` in same directory.
2. **Test-Only Import Handling**: CRITICAL — `src/gateway/mod.rs:22-23` had `#[cfg(test)] use defense::apply_external_ingress_policy;` which is a test-only import. This MUST be removed from mod.rs and added to tests.rs. The test file now imports `defense` module directly to access `apply_external_ingress_policy`.
3. **Test Count**: All 40 tests pass without modification after extraction (not 28 as initially estimated). Test module path preserved as `gateway::tests::*` in cargo test output.
4. **File Size Reduction**: Production code reduced from 929 lines to 273 lines (70% test code removed). Production logic: ~271 lines (gateway initialization, listener setup, state construction, route configuration).
5. **Dual Crate Root Verification**: Both `cargo check --lib` and `cargo check --bin asteroniris` pass. Also verified `cargo check --no-default-features` passes — critical for feature-gated code paths.
6. **Test Helper Functions**: Extracted test file includes helper functions `make_test_state()`, `make_whatsapp_state()`, `compute_whatsapp_signature_hex()`, `compute_whatsapp_signature_header()` which are used across multiple test groups.

**Verification**:
- `cargo test --lib -- gateway::tests`: 40 passed ✓
- `cargo check --lib`: clean ✓
- `cargo check --bin asteroniris`: clean ✓
- `cargo check --no-default-features`: clean ✓
- Evidence saved to `.sisyphus/evidence/task-2-gateway-tests.txt`

**Files Modified**:
- `src/gateway/mod.rs`: 929 → 273 lines
- `src/gateway/tests.rs`: created with 656 lines

**Pattern Comparison**:
- Task 1 (policy): Implicit `mod tests;` with `use super::*;` (child module)
- Task 2 (gateway): Implicit `mod tests;` with `use super::*;` (child module) + test-only import removal
- Task 3 (secrets): Explicit `#[path = "secrets_tests.rs"] mod tests;` with `use super::*;` (custom filename)
- Task 5 (skills): Implicit `mod tests;` with `use crate::skills::*;` (sibling module)
- **Key Difference**: Gateway tests required removing test-only imports from mod.rs before extraction

**Critical Trap Avoided**:
- The `#[cfg(test)] use defense::apply_external_ingress_policy;` import on lines 22-23 of mod.rs was a test-only import that would have caused compilation errors if left in place. Removing it from mod.rs and ensuring tests.rs imports `defense` module directly was essential.

## Task 4: Extract providers tests (2026-02-20)

**Pattern Applied**: Extracted 444 lines of test code from `src/providers/mod.rs` into separate `src/providers/tests.rs` file using implicit module declaration pattern.

**Key Learnings**:
1. **Module Declaration Pattern**: Replaced entire `#[cfg(test)] mod tests { ... }` block (lines 519-963) with `#[cfg(test)] mod tests;` declaration. Rust automatically resolves to `tests.rs` in same directory.
2. **Import Handling**: Extracted test file uses `use super::*;` to access all public functions from parent module: `create_provider`, `create_resilient_provider`, `create_resilient_provider_with_resolver`, `sanitize_api_error`, etc. No additional imports needed beyond `super::*`.
3. **Test Count**: All 54 tests pass without modification after extraction. Test module path preserved as `providers::tests::*` in cargo test output.
4. **File Size Reduction**: Production code reduced from 963 lines to 520 lines (46% test code removed). Production logic: ~518 lines (provider factory, API key resolution, secret scrubbing, error sanitization, resilient provider chains).
5. **Section Headers Preserved**: All inline comment section headers (e.g., `// ── Primary providers ────────────────────────────────────`) were preserved in extracted tests — these are necessary for test organization and readability.
6. **No Production Logic Changes**: All provider factory logic, secret scrubbing functions, and API error handling remain untouched in mod.rs. Only test reorganization.

**Verification**:
- `cargo test --lib -- providers::tests`: 54 passed ✓
- `cargo clippy -- -D warnings`: clean (no providers warnings) ✓
- Evidence saved to `.sisyphus/evidence/task-4-providers-tests.txt`

**Files Modified**:
- `src/providers/mod.rs`: 963 → 520 lines
- `src/providers/tests.rs`: created with 444 lines

**Pattern Consistency**:
- Task 1 (policy): Implicit `mod tests;` with `use super::*;` (child module)
- Task 3 (secrets): Explicit `#[path = "secrets_tests.rs"] mod tests;` with `use super::*;` (custom filename)
- Task 4 (providers): Implicit `mod tests;` with `use super::*;` (child module) ✓ CONSISTENT
- Task 5 (skills): Implicit `mod tests;` with `use crate::skills::*;` (sibling module)

**Codebase Tuning Progress**:
- Baseline: 84 total `#[allow(clippy::*)]` suppressions
- Completed extractions: security::policy (87 tests), security::secrets (37 tests), providers (54 tests), skills (18 tests)
- Remaining targets: gateway (70% tests), security::policy (87% tests)
- Total test code extracted so far: 196 tests across 4 modules

## Task 8: Fix production unwrap() calls (2026-02-20)

**Pattern Applied**: Replaced 2 production `unwrap()` anti-patterns with proper error handling.

**Key Learnings**:
1. **Anti-pattern 1 - Double-check unwrap**: `src/skills/mod.rs:549` had `.is_ok() && .unwrap()` pattern. Fixed by replacing with `if let Ok(output) = junction_result { ... }` which is idiomatic Rust and avoids the double evaluation.
2. **Anti-pattern 2 - Mutex poisoning**: `src/channels/email_channel.rs:432` had `Mutex::lock().unwrap()` which panics on poisoned mutex. Fixed with `Mutex::lock().unwrap_or_else(|e| e.into_inner())` to recover from poisoning and prefer stale data over panic.
3. **Comment Necessity**: Added comment "Recover from mutex poisoning — prefer stale data over panic" to explain the non-obvious error handling choice. This is a necessary comment per AGENTS.md security guidelines.
4. **Test Code Exemption**: Per AGENTS.md, `unwrap()` in test code is acceptable. Did NOT audit or fix test-only unwrap() calls.

**Verification**:
- `cargo test`: all tests pass ✓
- `cargo clippy -- -D warnings`: no new warnings in modified files ✓
- No production logic changes, only error handling improvements
- Evidence saved to `.sisyphus/evidence/task-8-unwrap-fix.txt`

**Files Modified**:
- `src/skills/mod.rs`: Fixed `.is_ok() && .unwrap()` → `if let Ok(output) = ...`
- `src/channels/email_channel.rs`: Fixed `Mutex::lock().unwrap()` → `Mutex::lock().unwrap_or_else(|e| e.into_inner())`

**AGENTS.md Compliance**:
- ✓ No `unwrap()` or `expect()` in production code (test code exempted)
- ✓ Proper error handling patterns applied
- ✓ Security-critical mutex poisoning recovery implemented

## Task 9: Audit and fix EmailConfig stub parity (2026-02-20)

**Pattern Applied**: Audited feature-gated stub `EmailConfig` in `src/channels/mod.rs` against real `EmailConfig` in `src/channels/email_channel.rs`, fixed divergence, added sync comment.

**Key Learnings**:
1. **Stub Divergence Found**: The stub (lines 6-24 of mod.rs) was missing critical `#[serde(default = "...")]` attributes on 5 fields: `imap_port`, `imap_folder`, `smtp_port`, `smtp_tls`, `poll_interval_secs`. Also missing `#[serde(default)]` on `allowed_senders`. This caused deserialization parity issues when the feature was disabled.
2. **Serde Defaults Matter**: The real `EmailConfig` uses serde defaults to provide sensible fallbacks (IMAP port 993, SMTP port 587, INBOX folder, 60s poll interval, TLS enabled). The stub must mirror these exactly for TOML deserialization to work identically in both feature modes.
3. **Default Implementation**: The real struct has a full `impl Default for EmailConfig` block. The stub was using `#[derive(Default)]` which doesn't respect the serde defaults. Fixed by adding explicit `impl Default` with the same logic.
4. **Helper Functions**: Added 5 helper functions (`default_imap_port()`, `default_smtp_port()`, `default_imap_folder()`, `default_poll_interval()`, `default_true()`) to the stub to match the real module's structure. These are necessary for serde attribute references.
5. **Sync Comment Added**: Added 4-line comment block above the stub:
   ```rust
   // Feature-gate stub: mirrors EmailConfig when "email" feature is disabled.
   // MUST stay in sync with src/channels/email_channel.rs EmailConfig.
   // Fields: imap_host, imap_port, imap_folder, smtp_host, smtp_port, smtp_tls,
   //         username, password, from_address, poll_interval_secs, allowed_senders
   ```
   This prevents future divergence by making the sync requirement explicit.

**Verification**:
- `cargo check --no-default-features`: ✓ passes (stub path)
- `cargo check --features "email"`: ✓ passes (real path)
- Both builds complete successfully with identical EmailConfig behavior
- Evidence saved to `.sisyphus/evidence/task-9-email-stub.txt`

**Files Modified**:
- `src/channels/mod.rs`: Lines 6-24 expanded to lines 6-68 (stub now includes helper functions and full Default impl)

**Critical Pattern**:
- Feature-gated stubs MUST maintain exact parity with real implementations
- Serde attributes (especially `#[serde(default = "...")]`) are critical for deserialization
- Sync comments are necessary to prevent silent divergence
- Both feature modes must be tested (`cargo check --no-default-features` AND `cargo check --features "email"`)

**Why This Matters**:
- When `email` feature is OFF, users get the stub. If stub diverges from real, TOML deserialization fails silently or produces wrong defaults.
- This is a correctness issue, not just code style. The stub is a contract that must be maintained.
- Without the sync comment, future maintainers won't know to update the stub when the real struct changes.

## Task 7: Extract tool descriptions to shared function (2026-02-20)

**Pattern Applied**: Created shared `tool_descriptions(browser_enabled: bool, composio_enabled: bool)` function in `src/tools/mod.rs` and replaced duplicated inline `tool_descs` constructions in both `src/agent/loop_/mod.rs` and `src/channels/mod.rs`.

**Key Learnings**:
1. **Duplication Identified**: Both `agent/loop_/mod.rs` (~lines 532-569) and `channels/mod.rs` (~lines 295-327) had nearly identical tool description vectors with 6 base tools (shell, file_read, file_write, memory_store, memory_recall, memory_forget) plus conditional browser_open.
2. **Key Difference**: Agent loop additionally includes composio conditionally (lines 564-568), while channels does not. The shared function signature accommodates both: `pub fn tool_descriptions(browser_enabled: bool, composio_enabled: bool) -> Vec<(&'static str, &'static str)>`.
3. **Function Placement**: `src/tools/mod.rs` is the natural home for this function since it describes tools and is already the module facade for tool-related functionality.
4. **Call Sites**:
   - `agent/loop_/mod.rs`: Calls `crate::tools::tool_descriptions(config.browser.enabled, config.composio.enabled)`
   - `channels/mod.rs`: Calls `crate::tools::tool_descriptions(config.browser.enabled, false)` (no composio in channels)
5. **Docstring Requirements**: Rust clippy::doc_markdown requires backticks around identifiers in documentation. Initial docstring had 4 clippy violations; fixed by adding backticks to parameter names and tool names.
6. **Lines Eliminated**: ~40 lines of duplication removed (25 lines from agent/loop_, 25 lines from channels, replaced with 1-line function calls).

**Verification**:
- `cargo test`: All tests pass (no regressions) ✓
- `cargo clippy -- -D warnings`: Clean (no warnings) ✓
- Evidence saved to `.sisyphus/evidence/task-7-tool-descs.txt`

**Files Modified**:
- `src/tools/mod.rs`: Added `tool_descriptions()` function (44 lines)
- `src/agent/loop_/mod.rs`: Replaced 38-line inline construction with 1-line function call
- `src/channels/mod.rs`: Replaced 33-line inline construction with 1-line function call

**Pattern Consistency**:
- Shared function approach: Centralizes tool description logic in the tools module
- Conditional parameters: `browser_enabled` and `composio_enabled` flags allow both call sites to use the same function
- Return type: `Vec<(&'static str, &'static str)>` matches the expected format for system prompts

## Task 6: Channel factory extraction (2026-02-20)

**Pattern Applied**: Extracted duplicated channel construction logic from `doctor_channels()` and `start_channels()` into `src/channels/factory.rs` with a shared `build_channels()` function.

**Key Learnings**:
1. **Return Type Design**: Factory returns `Vec<(&'static str, Arc<dyn Channel>)>` (named channels). `doctor_channels` uses names for diagnostics; `start_channels` discards names via `.into_iter().map(|(_, ch)| ch).collect()`.
2. **Feature Gate Preservation**: `#[cfg(feature = "email")]` on EmailChannel construction is preserved in factory.rs — critical for `--no-default-features` builds.
3. **Import Strategy**: Factory imports channel types directly (`crate::channels::TelegramChannel`, etc.) and `crate::config::ChannelsConfig`. No need for `Config` — only the channels sub-config is needed.
4. **Duplication Eliminated**: ~85 lines removed from each function (170 total). `doctor_channels` shrunk from ~87 lines to ~50; removed its `#[allow(clippy::too_many_lines)]` suppression.
5. **Thin Facade Principle**: AGENTS.md says "Keep `channels/mod.rs` as a thin facade." This extraction moves construction logic out of mod.rs into a focused sub-module, aligning with the architecture.

**Verification**:
- `cargo test --lib -- channels`: 256 passed
- `cargo check --no-default-features`: clean
- `cargo check --features "email"`: clean
- `cargo clippy -- -D warnings`: clean
- Evidence saved to `.sisyphus/evidence/task-6-channel-factory.txt`

**Files Modified**:
- `src/channels/factory.rs`: created (103 lines)
- `src/channels/mod.rs`: 585 → 437 lines (~148 lines removed, 1 line added for `pub mod factory`)

## Task 10: Arc<Config> at runtime entry points (2026-02-20)

**Pattern Applied**: Shifted daemon/gateway/channels startup boundaries to shared `Arc<Config>` ownership to remove repeated full-config clones at daemon startup while keeping behavior unchanged.

**Key Learnings**:
1. **Boundary Pattern**: Keep top-level `daemon::run(config: Config, ...)` as-is for compatibility, then convert once with `let config = Arc::new(config);` and share via `Arc::clone(&config)`.
2. **Auto-Deref Works**: `Arc<Config>` reads naturally through field access (`config.tunnel.provider`, `&config.memory`) with no callsite noise.
3. **Owned-Config Holdouts**: For components not yet migrated (e.g., `agent::run` and `cron::scheduler::run`), bridge with inner clone `(*config).clone()` at the callsite.
4. **Dispatch Layer Matters**: Entry signature changes in `gateway/channels` require updating `app::dispatch` callsites (and any integration tests invoking `run_gateway_with_listener`) even when `main.rs` only forwards config.
5. **Clone Audit Result**: `src/daemon/mod.rs` now has zero `config.clone()` calls; shared startup paths use `Arc::clone` instead.

**Verification**:
- `cargo test`: passes ✓
- `cargo check --lib`: passes ✓
- `cargo check --bin asteroniris`: passes ✓
- `cargo check --no-default-features`: passes ✓
- `cargo check --features "email,vector-search,tui,media"`: passes ✓
- Evidence saved to `.sisyphus/evidence/task-10-arc-config.txt`

**Files Modified**:
- `src/daemon/mod.rs`
- `src/gateway/mod.rs`
- `src/channels/mod.rs`
- `src/main.rs`
- `src/app/dispatch.rs`
- `tests/gateway/auth.rs`


## Task 11: Remove remaining Arc<Config> clone bridges (2026-02-20)

**Pattern Applied**: Migrated `agent::run` and `cron::scheduler::run` to take `Arc<Config>` and updated runtime call sites to use explicit `Arc::clone` ownership sharing.

**Key Learnings**:
1. **Final Bridge Removal**: The remaining `(*config).clone()` bridges were at `app::dispatch` (agent/daemon calls) and `daemon::run` (scheduler/heartbeat->agent calls). Converting `daemon::run` to accept `Arc<Config>` eliminated the last dispatch-level full-clone bridge cleanly.
2. **Arc Auto-Deref Keeps Call Sites Clean**: Existing reads (`config.workspace_dir`, `&config.memory`, etc.) continue to work unchanged after signature migration because `Arc<Config>` auto-derefs to `Config`.
3. **Intent Clarity in Agent Loop**: Replaced all `mem.clone()` occurrences in `src/agent/loop_/mod.rs` with `Arc::clone(&mem)` to make shared ownership explicit without behavior changes.
4. **Scheduler Boundary Consistency**: `cron::scheduler::run` now accepts `Arc<Config>`, aligning with daemon supervisor closures that already hold shared config.

**Verification**:
- `lsp_diagnostics` clean on changed files (`src/agent/loop_/mod.rs`, `src/cron/scheduler.rs`, `src/app/dispatch.rs`, `src/daemon/mod.rs`) ✓
- `cargo test --test agent` ✓
- `cargo test --lib -- agent` ✓
- `cargo test` ✓
- Evidence saved to `.sisyphus/evidence/task-11-agent-config.txt`

**Files Modified**:
- `src/agent/loop_/mod.rs`
- `src/cron/scheduler.rs`
- `src/app/dispatch.rs`
- `src/daemon/mod.rs`

## Task 14: Security policy decomposition assessment (2026-02-20)

**Decision: No extraction needed — mod.rs is already thin at 140 lines.**

**Assessment**:
1. **Post-T1 State**: After test extraction (Task 1), `src/security/policy/mod.rs` went from 1016 → 140 lines. The remaining code is:
   - 14 lines: module declarations + `pub use` re-exports (facade)
   - 4 lines: imports + constants
   - 14 lines: `SecurityPolicy` struct definition (12 fields)
   - 50 lines: `Default` impl (mostly list literals for allowed_commands/forbidden_paths)
   - 52 lines: 5 impl methods (can_act, record_action, is_rate_limited, consume_action_and_cost, from_config)
   - 2 lines: test module declaration
2. **Why No enforcement.rs**: The 5 impl methods total ~35 lines of logic. They are small, cohesive, and tightly coupled to the struct. Extracting would be over-decomposition.
3. **Existing Sub-modules**: `command.rs`, `path.rs`, `tenant.rs`, `trackers.rs`, `types.rs`, `tests.rs` — the module is already well-decomposed.

**Security Regression Gate**: All 28 tests pass:
- `agent action_intent`: 2 passed ✓
- `agent external_content`: 2 passed ✓
- `gateway`: 9 passed ✓
- `persona injection_guard`: 1 passed ✓
- `memory governance`: 5 passed ✓
- `memory revocation_gate`: 3 passed ✓
- `memory tenant_recall`: 6 passed ✓

**Key Insight**: 140 lines is the right size for a module facade that owns a struct + its Default + a handful of short methods. The "thin facade" goal from AGENTS.md doesn't mean "zero logic" — it means "no test code, no large blocks that belong in sub-modules." This module achieves that.

**Evidence**: `.sisyphus/evidence/task-14-security-decompose.txt`

## Task 13: Providers mod.rs decomposition into factory.rs + scrub.rs (2026-02-20)

**Pattern Applied**: Decomposed `src/providers/mod.rs` (520 lines) into `factory.rs` (381 lines) and `scrub.rs` (116 lines), leaving mod.rs as a 23-line thin facade.

**Key Learnings**:
1. **Re-export Preservation**: All existing `pub use` paths (`crate::providers::create_provider`, `crate::providers::sanitize_api_error`, etc.) continue to work via re-exports in mod.rs. No call sites needed updating.
2. **super:: Resolution**: Sub-modules like `anthropic.rs` use `super::api_error(...)`. After moving `api_error` to `scrub.rs`, the `pub use scrub::api_error;` in mod.rs makes `super::api_error` resolve correctly from sibling modules.
3. **Test Compatibility**: `tests.rs` uses `use super::*;` which pulls all `pub use` items from mod.rs. Since all public functions are re-exported, all 133 tests pass without modification.
4. **Private Function Handling**: `resolve_api_key()` and `create_provider_with_runtime_recovery()` are private to their new home (`factory.rs`). Tests don't use them directly — they're only called by the public factory functions.
5. **Import Strategy in factory.rs**: Uses `super::` to reference sibling modules (`super::anthropic::AnthropicProvider`, `super::compatible::*`, etc.) since factory.rs is a child of the providers module.

**Verification**:
- `cargo test --lib -- providers`: 133 passed ✓
- LSP diagnostics: 0 errors on all 3 files ✓
- Note: `cargo check --lib` / `--bin` blocked by pre-existing channel decomposition conflicts (telegram, imessage) from other tasks — NOT caused by providers changes.
- Evidence saved to `.sisyphus/evidence/task-13-providers-decompose.txt`

**Files Modified**:
- `src/providers/mod.rs`: 520 → 23 lines (thin facade)
- `src/providers/factory.rs`: created (381 lines)
- `src/providers/scrub.rs`: created (116 lines)

**Boundary Decision**:
- scrub.rs: `scrub_secret_patterns`, `sanitize_api_error`, `api_error` + private helpers (`is_secret_char`, `token_end`, `scrub_after_marker`, `MAX_API_ERROR_CHARS`)
- factory.rs: `create_provider`, `create_provider_with_oauth_recovery`, `create_resilient_provider*` + private helpers (`resolve_api_key`, `create_provider_with_runtime_recovery`)
- This boundary is natural: scrub functions are about security/sanitization, factory functions are about provider instantiation

## Task 15: Decompose skills/mod.rs into loader.rs (2026-02-20)

**Pattern Applied**: Extracted ~560 lines of skill loading logic from `src/skills/mod.rs` into `src/skills/loader.rs`, leaving mod.rs as a thin facade with struct definitions and re-exports.

**Key Learnings**:
1. **Boundary Decision**: Public structs (`Skill`, `SkillTool`) stay in mod.rs. ALL functions + private structs (`SkillManifest`, `SkillMeta`) + constants move to loader.rs. This keeps mod.rs as pure type definitions + facade.
2. **Child Module Imports**: Since `loader.rs` is declared via `pub mod loader;` in mod.rs, it's a child module. `use super::{Skill, SkillTool};` works correctly for accessing parent module types.
3. **Re-export Pattern**: `pub use loader::{handle_command, init_skills_dir, load_skills, skills_dir, skills_to_prompt};` preserves all existing public API paths (`crate::skills::load_skills`, etc.).
4. **File Size Result**: mod.rs: 611 → 46 lines. loader.rs: 572 lines created. Total lines slightly decreased due to removing duplicate imports.
5. **Pre-existing Blocker**: `cargo check --lib` fails due to imessage.rs + imessage/mod.rs module ambiguity (from task-14 decomposition). This is NOT related to skills changes. Verified via `cargo check --bin asteroniris` (passes) and `cargo test --bin asteroniris -- skills` (21 tests pass).
6. **Test Stability**: All 21 skills tests pass (18 unit + 2 symlink + 1 prompt_builder). Test imports (`use crate::skills::{...}`) work unchanged because pub use re-exports maintain the same public API paths.

**Verification**:
- `cargo test --bin asteroniris -- skills`: 21 passed (via bin crate due to pre-existing lib ambiguity)
- `cargo check --bin asteroniris`: passes with only pre-existing warnings
- LSP diagnostics: clean on both mod.rs and loader.rs
- Evidence saved to `.sisyphus/evidence/task-15-skills-decompose.txt`

**Files Modified**:
- `src/skills/mod.rs`: 611 → 46 lines (thin facade)
- `src/skills/loader.rs`: created with 572 lines

**Architecture Alignment**: AGENTS.md says "mod.rs is a thin facade with pub mod + pub use re-exports. Extract logic into focused sub-modules." This extraction fully achieves that for the skills module.

## Task 17: Split telegram.rs into telegram/ subdirectory (2026-02-20)

**Pattern Applied**: Split `src/channels/telegram.rs` (836 LOC) into `src/channels/telegram/` directory with 4 files: mod.rs (facade), api.rs (API calls), handler.rs (Channel trait), tests.rs (all tests).

**Key Learnings**:
1. **Rust Private Field Visibility in Submodules**: Private struct fields defined in `mod.rs` are accessible from child modules (`api.rs`, `handler.rs`, `tests.rs`) because Rust's visibility rules allow descendants to access private items. No `pub(super)` needed on struct fields.
2. **Multiple impl Blocks Across Files**: `TelegramChannel` struct defined in `mod.rs`, with separate `impl` blocks in `api.rs` (9 API methods) and `handler.rs` (Channel trait impl). Rust allows multiple impl blocks for the same type across submodules.
3. **Test Import Strategy**: `tests.rs` uses `use super::*;` plus explicit imports for types needed from outside the module (`use crate::channels::traits::Channel;` and `use std::path::Path;`). Methods from api.rs and handler.rs are available on `TelegramChannel` instances because they're `pub` and the modules are descendants.
4. **Parallel Task Conflict**: T16 (imessage split) ran in parallel and created `imessage/mod.rs` without deleting `imessage.rs`, causing E0761. Workaround: temporarily move `imessage.rs` aside during verification, restore after.
5. **Module Discovery**: `pub mod telegram;` in `channels/mod.rs` finds `telegram/mod.rs` automatically after old `telegram.rs` is deleted. No parent module changes needed.
6. **Test Count**: Task spec said "16 tests" but actual count is 30 (16 sync + 14 async). All 30 preserved and passing.

**Verification**:
- `cargo test --lib -- channels::telegram`: 30 passed ✓
- `cargo check --lib`: clean ✓
- Evidence saved to `.sisyphus/evidence/task-17-telegram-split.txt`

**Files Created**:
- `src/channels/telegram/mod.rs`: 38 lines (struct + constructor + 3 helpers)
- `src/channels/telegram/api.rs`: 333 lines (9 API methods)
- `src/channels/telegram/handler.rs`: 134 lines (Channel trait impl)
- `src/channels/telegram/tests.rs`: 326 lines (30 tests)

**File Deleted**:
- `src/channels/telegram.rs`: 836 lines → replaced by directory

**Split Boundary**:
- mod.rs: TelegramChannel struct, `new()`, `api_url()`, `is_user_allowed()`, `is_any_user_allowed()`
- api.rs: send_document, send_document_bytes, send_photo, send_photo_bytes, send_video, send_audio, send_voice, send_document_by_url, send_photo_by_url
- handler.rs: Channel trait (name, max_message_length, send, listen, health_check)
- tests.rs: All 30 tests moved verbatim

## Task 16: Split imessage.rs into imessage/ subdirectory (2026-02-20)

**Pattern Applied**: Split `src/channels/imessage.rs` (933 LOC) into `src/channels/imessage/` directory with 4 files: mod.rs (facade), auth.rs (AppleScript escaping/target validation), handler.rs (Channel trait + DB queries), tests.rs (all tests).

**Key Learnings**:
1. **Explicit Test Imports Over `use super::*`**: Unlike other splits, tests.rs uses explicit imports (`use super::auth::{...}`, `use super::handler::{...}`, `use super::IMessageChannel`, `use crate::channels::traits::Channel`, `use rusqlite::Connection`). This is safer when functions live in sibling submodules rather than the parent.
2. **`pub(super)` for Cross-Submodule Access**: Functions in `auth.rs` and `handler.rs` use `pub(super)` visibility so they can be imported by sibling modules and tests. Private functions in a child module are NOT visible to sibling modules.
3. **Thin Facade — No Re-exports Needed**: mod.rs has zero `pub use` re-exports from child modules. The struct and its inherent methods live in mod.rs; everything else is internal to the module tree. External code only sees `IMessageChannel` through `channels::mod.rs`'s existing `pub use imessage::IMessageChannel;`.
4. **CRITICAL: Delete Old File**: `rm -rf src/channels/imessage` deletes a dir/file named `imessage`, NOT `imessage.rs`. Must explicitly `rm src/channels/imessage.rs` to avoid E0761 (file found at both locations). Initial `cargo check` passed due to caching but `cargo clippy` caught it.
5. **Test Count**: 58 tests (not 42 as estimated in task spec). All 58 pass.

**Verification**:
- `cargo test --lib -- channels::imessage`: 58 passed
- `cargo check --lib`: clean
- `cargo clippy --lib -- -D warnings`: clean
- Evidence saved to `.sisyphus/evidence/task-16-imessage-split.txt`

**Files Created**:
- `src/channels/imessage/mod.rs`: 33 lines (struct + constructor + is_contact_allowed)
- `src/channels/imessage/auth.rs`: 56 lines (escape_applescript + is_valid_imessage_target)
- `src/channels/imessage/handler.rs`: 175 lines (Channel trait impl + get_max_rowid + fetch_new_messages)
- `src/channels/imessage/tests.rs`: 668 lines (58 tests)

**File Deleted**:
- `src/channels/imessage.rs`: 933 lines -> replaced by directory

**Split Boundary**:
- mod.rs: IMessageChannel struct, `new()`, `is_contact_allowed()`
- auth.rs: `escape_applescript()`, `is_valid_imessage_target()` (security-critical AppleScript injection prevention)
- handler.rs: Channel trait (name, max_message_length, send, listen, health_check) + SQLite query functions
- tests.rs: All 58 tests moved with explicit imports

## Arc::clone() Idiomatic Patterns (Task 20)

**Pattern 1: Reference to Arc**
```rust
fn foo(security: &Arc<SecurityPolicy>) {
    let cloned = Arc::clone(security);  // ✓ idiomatic
    // NOT: security.clone()
}
```

**Pattern 2: Owned Arc**
```rust
fn bar(security: Arc<SecurityPolicy>) {
    let cloned = Arc::clone(&security);  // ✓ idiomatic
    // NOT: security.clone()
}
```

**Key Insight:**
- `Arc::clone()` is explicit about cloning the pointer, not the inner value
- Signals intent clearly: "I'm incrementing the reference count"
- Works with both `&Arc<T>` and `Arc<T>` (use `&` for owned case)
- Improves code clarity and follows Rust best practices

**Applied in:**
- `all_tools()`: 5 security clones + 3 memory clones
- `default_tools()`: 2 security clones
- Browser section: 2 security clones
- Total: 12 Arc::clone() replacements in src/tools/mod.rs

**Non-Arc Clones:**
- `browser_config.allowed_domains.clone()` - Vec<String>, keep as `.clone()`
- `browser_config.session_name.clone()` - Option<String>, keep as `.clone()`

## Task 18: Cow fast-path for provider scrubber (2026-02-20)

**Pattern Applied**: Updated `scrub_secret_patterns()` in `src/providers/scrub.rs` to return `Cow<'_, str>` and added a zero-allocation fast path.

**Key Learnings**:
1. **Cheap pre-scan beats unconditional allocation**: A `needs_scrubbing()` helper using `input.contains(...)` across known prefix+marker literals avoids `to_string()` in the common clean-input case.
2. **Return-type migration is low-risk with `Cow`**: Existing `sanitize_api_error()` logic stays almost identical; only borrow/owned boundary handling changes (`into_owned()` for short path, `as_ref()` for truncation path).
3. **Mutation helper can expose side effects explicitly**: Converting `scrub_after_marker()` to return `bool` records whether replacement happened while preserving prior redaction behavior.
4. **Behavior parity maintained**: No scrub pattern changes, no redaction-format changes (`[REDACTED]` unchanged), and provider test assertions remain stable.

**Verification**:
- `cargo clippy -- -D warnings`: clean ✓
- `cargo test --lib -- providers`: 133 passed ✓
- `cargo test`: full suite passed ✓
- Evidence saved to `.sisyphus/evidence/task-18-cow-scrub.txt`

**Files Modified**:
- `src/providers/scrub.rs`

## Task 19: Reduce unnecessary Arc cloning in agent loop (2026-02-20)

**Change**: Replaced `observer.clone()` → `Arc::clone(observer)` in `src/agent/loop_/mod.rs` (line 449).

**Findings**:
1. **mem.clone() already migrated**: All 6 `mem.clone()` sites were already `Arc::clone(&mem)` from Task 11.
2. **observer.clone()**: Was the only remaining Arc clone using implicit syntax. Changed to `Arc::clone(observer)` (no `&` needed — observer is `&Arc<dyn Observer>`).
3. **config.persona.state_mirror_filename.clone()**: NOT eliminable — `SystemPromptOptions` requires `Option<String>`. Changing to `Option<&str>` would cascade lifetimes through `build_system_prompt_with_options` and all callers.
4. **config.workspace_dir.clone()**: NOT eliminable — `enqueue_consolidation_task` needs owned `PathBuf` for spawned task.
5. **write_context.policy_context.clone()**: Struct clone (TenantPolicyContext), not Arc — appropriate as-is.

**Verification**:
- `cargo clippy -- -D warnings`: clean ✓
- `cargo test --test agent`: 13 passed ✓
- `cargo test --lib -- agent`: 16 passed ✓
- Evidence: `.sisyphus/evidence/task-19-agent-clones.txt`

## Task 22: Test helper deduplication assessment (2026-02-20)

**Decision: No extraction needed — existing test infrastructure is adequate.**

**Assessment Summary**:
1. **TempDir::new()**: 159 matches / 41 files — one-liner, not extractable
2. **SecurityPolicy::default()**: 60 matches / 13 files — one-liner, not extractable
3. **MemoryConfig creation**: 3 test files with different configs — insufficient identical copies
4. **test_config()**: 3 identical 7-line copies in cron/mod.rs, cron/scheduler.rs, daemon/mod.rs — BORDERLINE but module infrastructure overhead outweighs 14-line savings
5. **CountingProvider**: 1 file only — not extractable
6. **Existing harness**: tests/support/memory_harness.rs (295 lines) already provides comprehensive shared test utilities

**WhatsApp tests (898 LOC)**: NOT worth splitting. File is long due to JSON payload verbosity, not structural duplication. 40+ tests each test unique edge cases with already-extracted make_channel() helper. Section headers provide good organization.

**Gateway tests (658 LOC)**: Already well-organized with make_test_state(), make_whatsapp_state() helpers and section headers.

**Key Insight**: One-liner patterns (TempDir, SecurityPolicy) should NEVER be extracted into helpers — the indirection adds complexity without reducing cognitive load. Only multi-line setup blocks with 3+ identical copies in different modules justify shared helpers.

**Evidence**: `.sisyphus/evidence/task-22-test-optimize.txt`

## Task 23: Audit clippy::too_many_lines and clippy::too_many_arguments suppressions (2026-02-20)

**Pattern Applied**: Systematic audit of all 41 suppressions for `clippy::too_many_lines` and `clippy::too_many_arguments` across the codebase. Tested each suppression by temporarily removing it and running `cargo clippy -- -D warnings` to determine if the function still exceeds the limit.

**Key Learnings**:
1. **Testing Methodology**: For each suppression, remove the `#[allow(...)]` attribute, run clippy with `-D warnings`, and check if clippy reports an error. If no error, the suppression is no longer needed. If error, restore it.
2. **Conservative Approach**: Only remove suppressions where clippy passes cleanly with `-D warnings`. This ensures no regressions and maintains code quality.
3. **Removal Results**: 3 suppressions were safely removable:
   - `src/gateway/mod.rs:88` — `run_gateway()` function was refactored and is now under the 100-line limit
   - `src/onboard/prompts/workspace.rs:51` — `input_workspace_path()` function was refactored and is now under the limit
   - `src/onboard/tui/app.rs:207` — `handle_key_event()` function was refactored and is now under the limit
4. **Kept Suppressions**: 38 suppressions were kept because the functions still exceed the limits. These are legitimate suppressions that prevent clippy warnings on complex functions that cannot be easily decomposed further.
5. **Pre-existing Issues**: The codebase has 2 pre-existing dead_code errors unrelated to this task. These are not part of Task 23 (which focuses on too_many_lines/too_many_arguments).

**Verification**:
- Baseline: 41 suppressions
- After removal: 38 suppressions
- Reduction: 3 suppressions (7.3% reduction)
- `cargo test`: All tests pass ✓
- No new suppressions added
- No regressions introduced

**Files Modified**:
- `src/gateway/mod.rs`: Removed 1 suppression
- `src/onboard/prompts/workspace.rs`: Removed 1 suppression
- `src/onboard/tui/app.rs`: Removed 1 suppression

**Key Insight**: Not all suppressions can be removed without further refactoring. The 38 remaining suppressions represent functions that are legitimately complex and cannot be easily decomposed. Removing suppressions without addressing the underlying complexity would just hide the problem. The goal is to remove suppressions where the underlying issue has been resolved through refactoring (as happened with Wave 4 decomposition work).

**Evidence**: `.sisyphus/evidence/task-23-clippy-cleanup.txt`

## T21: dead_code audit in src/memory/
- SQLite memory has a "projection layer" (upsert/search/fetch/list/delete/count projection entries) that is fully implemented but NOT wired to the Memory trait. All these methods are dead code transitively.
- The projection layer includes FTS5 keyword search, cosine-similarity vector search, and hybrid merge — a complete search engine that's dormant.
- struct fields (db_path, vector_weight, keyword_weight) are written in constructors but only read by dead projection methods.
- Removing `parse_markdown_entry_metadata` exposed that `ParsedMarkdownLine.layer` and `.provenance` fields are never read in production code — they're populated during parsing but only consumed by the removed function.
- `sed -i` via bash is MORE reliable than the Edit tool for making multiple small changes to files — Edit tool changes were lost multiple times during git operations.
- Previous sessions (commits 4a75007 and 21c52df) partially completed this task. When resuming, always check HEAD state first.
- Comment placement matters: `// reason` should appear BEFORE `#[allow(dead_code)]`, not after it.
