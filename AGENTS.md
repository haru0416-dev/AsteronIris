# AGENTS.md

Guidance for AI coding agents operating in the AsteronIris repository.
Rust 2024 edition, stable toolchain, clippy pedantic, anyhow/thiserror errors.

## Prerequisites

- Rust stable toolchain (see `rust-toolchain.toml`)
- [protoc](https://grpc.io/docs/protoc-installation/) v29+ (required by build/CI)

## Commands

```bash
# Format
cargo fmt                                 # auto-format
cargo fmt -- --check                      # CI check (no writes)

# Lint — CI treats warnings as errors
cargo clippy -- -D warnings

# Test
cargo test                                # all tests
cargo test-dev                            # 4-thread parallel (faster local)
cargo test-dev-tests                      # 4-thread, integration tests only
cargo test --test memory                  # single integration binary
cargo test --test memory -- comparison    # single test by name substring
cargo test --test memory -- comparison --exact  # exact test name match
BACKEND=sqlite cargo test --release --test memory -- throughput --nocapture

# Other aliases (defined in .cargo/config.toml)
cargo build-fast                          # trimmed feature set build
cargo build-minimal                       # bundled-sqlite only
cargo check-all                           # check with all features
cargo coverage                            # llvm-cov HTML report

# Security audits
cargo audit
cargo deny check advisories licenses sources
```

Pre-push hook (`.githooks/pre-push`) enforces fmt + clippy + test.
Install: `git config core.hooksPath .githooks`

## Architecture

Trait + factory dispatch everywhere. Each subsystem defines a trait and a factory function:

| Module | Trait / Entry | Role |
|--------|--------------|------|
| `core/` | Domain facade | AI core namespace (agent, memory, providers, tools, planner, sessions, persona, eval) |
| `intelligence/memory/` | `Memory` trait, `create_memory()` | SQLite / LanceDB / Markdown backends |
| `intelligence/providers/` | `Provider` trait | LLM provider factory + secret scrubbing |
| `channels/` | `Channel` trait | CLI / Telegram / Discord / Slack / WhatsApp / Matrix / Email / IRC |
| `intelligence/tools/` | `Tool` trait, `default_tools()` / `all_tools()` | Shell, file, memory, browser, composio |
| `security/` | `SecurityPolicy` | Deny-by-default allowlist, pairing, vault, writeback guard |
| `transport/gateway/` | Axum HTTP server | Pairing, webhooks, autosave (64KB body, 30s timeout) |
| `intelligence/agent/` | Conversation loop | Tool execution, reflection |
| `platform/daemon/` | Supervisor | Gateway + channels + heartbeat + cron |
| `config/` | `Config` | TOML schema + env-var overrides |
| `plugins/skillforge/` | Skill pipeline | Discovery, evaluation, integration |

### Module Structure

`mod.rs` is a thin facade with `pub mod` + `pub use` re-exports.
Extract logic into focused sub-modules: `handlers.rs`, `autosave.rs`, `defense.rs`, `tests.rs`, etc.

## Code Style

### Lint Baseline (src/lib.rs)

```rust
#![warn(clippy::all, clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc, clippy::missing_panics_doc,
    clippy::unnecessary_literal_bound,
    clippy::module_name_repetitions, clippy::struct_field_names,
    clippy::must_use_candidate, clippy::new_without_default,
    clippy::return_self_not_must_use,
)]
```

### Formatting

No `rustfmt.toml` — default rustfmt settings apply. Run `cargo fmt` before committing.
Indentation: 4 spaces for `.rs`/`.sh`/`Dockerfile`, 2 spaces for `.toml`/`.yml`/`.yaml`/`.json`.
LF line endings, UTF-8, trailing newline required (see `.editorconfig`).

### Imports

`cargo fmt` manages ordering. Typical grouping seen in the codebase:
1. `crate::` imports first
2. External crates alphabetized (`anyhow`, `async_trait`, `serde`, etc.)
3. `std::` at the end

Use braced imports to merge from the same crate: `use serde::{Deserialize, Serialize};`

### Types & Naming

- Structs/enums: `PascalCase`. Enum variants: `PascalCase`.
- Functions/methods: `snake_case`. Constants: `SCREAMING_SNAKE_CASE`.
- `#[serde(rename_all = "snake_case")]` on all serialized enums.
- `#[serde(tag = "kind", rename_all = "snake_case")]` for tagged enums.
- `#[derive(Debug, Clone, Serialize, Deserialize)]` is the standard derive set.
- `#[derive(..., PartialEq, Eq)]` for enums used in comparisons.
- `strum::Display` + `#[strum(serialize_all = "snake_case")]` when string representation needed.
- `#[must_use]` on pure functions that return values.

### Error Handling

- `anyhow::Result<T>` for all fallible public functions.
- `anyhow::bail!("message")` for early-exit errors.
- `thiserror::Error` for structured error enums at library boundaries.
- **No** `unwrap()` or `expect()` in production code. Tests and setup are OK.
- Empty catch blocks are forbidden.

### Async Patterns

- `#[async_trait]` from the `async_trait` crate for async trait methods.
- `Arc<dyn Trait>` for shared trait objects across async boundaries.
- Tokio runtime with `rt-multi-thread`.
- Rust 2024 edition: `if let` chains are used (e.g. `if let Some(x) = opt && cond`).

### Constructor & Builder Patterns

- `fn new(...)` for primary constructors. Accept `impl Into<String>` for string params.
- Builder methods: `fn with_field(mut self, value: T) -> Self` returning `self`.
- Named constructors for variants: `fn source_reference(...)`, `fn inferred_claim(...)`.

### Feature Gates

- `#[cfg(feature = "...")]` for optional modules/code.
- Default features: `email`, `vector-search`, `tui`, `bundled-sqlite`, `media`, `link-extraction`.
- Feature-gated modules: `lancedb` (vector-search), `ratatui`/`crossterm` (tui), `lettre`/`mail-parser` (email), `infer`/`mime` (media), `scraper` (link-extraction), `rmcp` (mcp).

## Test Structure

Six integration test binaries under `tests/`:

```
tests/memory.rs    tests/gateway.rs    tests/agent.rs
tests/persona.rs   tests/runtime.rs    tests/project.rs
```

**Critical**: Integration test routers use explicit `#[path = "subdir/file.rs"]` attributes.
Implicit directory-based module resolution does NOT work for integration test crate roots.

```rust
// tests/memory.rs — correct pattern
#[path = "support/memory_harness.rs"]
mod memory_harness;

#[path = "memory/comparison.rs"]
mod comparison;
```

Harness files are referenced via `use super::memory_harness;` inside child modules.
Shared helpers (e.g., `temp_sqlite()`) live in the root test file (`tests/memory.rs`).

### Writing Tests

- Unit tests: `#[cfg(test)] mod tests` at bottom of source file.
- Use `tempfile::TempDir` for filesystem isolation.
- `unwrap()` / `expect()` are acceptable in tests.
- `tokio::test` for async test functions.

**Known pre-existing failure**: `inventory_scope_lock::inventory_scope_lock`
(skillforge data drift — not a regression, do not attempt to fix).

## Security (Non-Negotiable)

These layers must never be bypassed or weakened:

1. **Deny-by-default** shell/file allowlist (`security/policy/`)
2. **Public gateway bind** refused unless tunnel or explicit opt-in (`security/pairing.rs`)
3. **ChaCha20Poly1305** encrypted secret vault (`security/secrets.rs`)
4. **Writeback guard** prevents persona self-corruption (`security/writeback_guard.rs`)
5. **Secret scrubbing** — `scrub_secret_patterns()` strips tokens/keys from LLM I/O

## Dependencies

Release profile: `opt-level = "z"`, LTO, `codegen-units = 1`, `panic = "abort"`, strip symbols.
Before adding a dependency: justify it, minimise features, disable default features.
Allowed licenses: MIT, Apache-2.0, BSD-2/3-Clause, ISC, MPL-2.0, Zlib, BSL-1.0, 0BSD, CC0-1.0 (see `deny.toml`).

## Commits

Conventional Commits format: `feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`.
Keep changes small and coherent. One logical change per commit.

## Channel Implementation

New channel-specific providers go in `src/channels/providers/<channel>.rs`.
Keep `channels/mod.rs` as a thin facade with re-exports only.
