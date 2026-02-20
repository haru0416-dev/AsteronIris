# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Format
cargo fmt
cargo fmt -- --check          # CI check (no writes)

# Lint (CI-strict — fail on any warning)
cargo clippy -- -D warnings

# Test
cargo test                    # all tests
cargo test-dev                # 4-thread parallel (faster)
cargo test-dev-tests          # 4-thread, integration tests only
cargo test --test memory      # single integration binary
cargo test --test memory -- comparison  # single test by name
BACKEND=sqlite cargo test --release --test memory -- throughput --nocapture

# Security
cargo audit
cargo deny check advisories licenses sources
```

## Architecture

### Module Map (`src/lib.rs` top-level)

| Module | Role |
|--------|------|
| `agent` | Conversation loop, tool execution, reflection |
| `gateway` | Axum HTTP server (pairing, webhooks, autosave) |
| `daemon` | Supervisor: gateway + channels + heartbeat + cron |
| `memory` | SQLite / LanceDB / Markdown backends behind `Memory` trait |
| `providers` | LLM provider factory + secret scrubbing |
| `channels` | CLI / Telegram / Discord / Slack / WhatsApp / Matrix / Email / IRC |
| `security` | Policy (allowlist/path/tenant), pairing auth, encrypted vault, writeback guard |
| `skillforge` | Skill discovery, evaluation, integration pipeline |
| `runtime` | Native / Docker adapter |
| `onboard` | First-run wizard + workspace scaffold |
| `config` | TOML schema + env-var overrides |
| `tools` | Tool registry (shell, file, memory, browser, composio) |
| `integrations` | External service adapters (inventory etc.) |

### Key Patterns

**Traits + Factory dispatch** — every extensible system defines a trait and a factory:
- `Memory` trait → `create_memory()` dispatches on `"sqlite" | "lancedb" | "markdown" | "none"`
- `Provider` trait → `create_resilient_provider_with_oauth_recovery()`
- `Channel` trait → runtime-registered dispatch in `channels/mod.rs`
- `Tool` trait → `default_tools()` / `all_tools()`

**Modular responsibility** — large files are actively being split into focused sub-modules.
Pattern: keep `mod.rs` as a thin facade, extract `handlers.rs / autosave.rs / defense.rs / tests.rs` etc.

**Security layers (non-negotiable):**
1. Deny-by-default shell/file allowlist (`security/policy/`)
2. Public gateway bind refused unless tunnel or explicit opt-in (`security/pairing.rs`)
3. ChaCha20Poly1305 secret vault (`security/secrets.rs`)
4. Writeback guard prevents persona self-corruption (`security/writeback_guard.rs`)
5. Provider output sanitised — `scrub_secret_patterns()` strips tokens/keys from LLM I/O

### Test Structure

Six integration binaries under `tests/`:

```
tests/memory.rs   tests/gateway.rs   tests/agent.rs
tests/persona.rs  tests/runtime.rs   tests/project.rs
```

**Critical:** integration test router files use **explicit `#[path = "subdir/file.rs"]`** attributes — implicit directory-based module resolution does not work for integration test crate roots.

Harness files are referenced as `use super::memory_harness;` inside child modules (not re-declared with `#[path]`).

`temp_sqlite()` helper lives in `tests/memory.rs`.

**Known pre-existing failure:** `inventory_scope_lock::inventory_scope_lock` (skillforge_unimplemented data drift — not a regression).

### Gateway (Axum)

- Max body: 64 KB | Request timeout: 30 s
- Routes: `GET /health`, `POST /pair`, `POST /webhook`, `GET|POST /whatsapp/*`
- Sub-modules: `autosave.rs`, `defense.rs`, `handlers.rs`, `signature.rs`

### Memory Backends

| Backend | Forget modes | Vector search |
|---------|-------------|---------------|
| SQLite  | Soft / Hard / Tombstone | No |
| LanceDB | Degraded | Yes |
| Markdown | Append-only | No |

### Error Handling

- Use `anyhow::Result<T>` for fallible operations
- Use `anyhow::bail!()` for early exit
- No `unwrap()` / `expect()` in production paths (tests/setup OK)

### Lint Baseline (`src/lib.rs`)

```rust
#![warn(clippy::all, clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc, clippy::missing_panics_doc,
    clippy::module_name_repetitions, clippy::struct_field_names,
    clippy::must_use_candidate, clippy::new_without_default,
    clippy::return_self_not_must_use, dead_code,
    clippy::unnecessary_literal_bound,
)]
```

### Release Profile

`opt-level = "z"`, LTO, `codegen-units = 1`, `panic = "abort"`, strip symbols.
Before adding a dependency: justify it, minimise features, disable defaults.

### Commit Conventions

`feat / fix / docs / test / refactor` prefix. Keep changes small and coherent.
Pre-push hook (`git config core.hooksPath .githooks`) enforces fmt + clippy + test.
