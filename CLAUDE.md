# CLAUDE.md

Comprehensive guidance for AI coding agents (including Claude Code) operating
in the **AsteronIris** repository — a secure, extensible AI assistant built in
Rust (2024 edition, stable toolchain).

> This file is the single source of truth. [`AGENTS.md`](AGENTS.md) redirects
> here.

---

## Prerequisites

- Rust **stable** toolchain (see `rust-toolchain.toml`)
- [protoc](https://grpc.io/docs/protoc-installation/) **v29+** (required by build/CI)

---

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
cargo test --test memory -- comparison --exact  # exact match
BACKEND=sqlite cargo test --release --test memory -- throughput --nocapture

# Cargo aliases (defined in .cargo/config.toml)
cargo build-fast                          # trimmed feature set build
cargo build-minimal                       # bundled-sqlite only
cargo test-fast                           # trimmed feature set test
cargo check-all                           # check with all features
cargo coverage                            # llvm-cov HTML report

# Security audits
cargo audit
cargo deny check advisories licenses sources
```

**Pre-push hook** (`.githooks/pre-push`) enforces `fmt + clippy + test`.
Install: `git config core.hooksPath .githooks`

CI coverage enforces a **40% line threshold** and skips
`inventory_scope_lock::inventory_scope_lock`.

---

## Architecture

Trait + factory dispatch everywhere. Each subsystem defines a trait and a
factory function. All paths below are relative to `src/`.

| Module | Trait / Entry | Role |
|--------|--------------|------|
| `config/` | `Config` | TOML schema + env-var overrides |
| `security/` | `SecurityPolicy` | Deny-by-default allowlist, pairing, vault, writeback guard |
| `llm/` | `Provider` trait | LLM provider factory + secret scrubbing (OpenRouter, OpenAI, Anthropic, Gemini, compatible) |
| `memory/` | `Memory` trait, `create_memory()` | SQLite / LanceDB / Markdown backends |
| `session/` | Session management | Conversation session lifecycle |
| `tools/` | `Tool` trait, `default_tools()` / `all_tools()` | Shell, file, memory, browser, composio |
| `prompt/` | Prompt construction | System prompt engineering |
| `agent/` | Conversation loop | Tool execution, reflection |
| `persona/` | Persona system | Identity, behavior, preferences |
| `subagents/` | Sub-agent coordination | Delegated tasks |
| `eval/` | Evaluation | Quality assessment |
| `planner/` | Planning | Orchestration and task planning |
| `process/` | Process model | Process abstraction and lifecycle |
| `transport/channels/` | `Channel` trait | CLI / Telegram / Discord / Slack / WhatsApp / Matrix / Email / IRC / iMessage |
| `transport/gateway/` | Axum HTTP server | Pairing, webhooks, autosave (64 KB body, 30 s timeout) |
| `platform/daemon/` | Supervisor | Gateway + channels + heartbeat + cron |
| `platform/cron/` | Scheduler | Cron-style task scheduling |
| `platform/service/` | OS service | Service lifecycle (install/start/stop) |
| `plugins/skillforge/` | Skill pipeline | Discovery, evaluation, integration |
| `plugins/skills/` | Skill implementations | Built-in skills |
| `plugins/mcp/` | MCP client | Model Context Protocol integration |
| `plugins/integrations/` | Integration registry | External service connectors |
| `runtime/` | Runtime services | Diagnostics, observability, tunnel, usage tracking, evolution |
| `app/` | Application dispatch | Command routing |
| `cli/commands/` | CLI surface | Command definitions |
| `onboard/` | Setup wizard | First-run onboarding flow |
| `media/` | Media processing | Detection, processing, storage |
| `ui/` | Terminal UI | Shared TUI styles (ratatui) |
| `utils/` | Helpers | Links, text utilities |

### Conventions

- `mod.rs` is a thin facade: `pub mod` + `pub use` re-exports. Logic in
  focused sub-modules.
- Factory pattern: `create_<subsystem>()` returns `Box<dyn Trait>`. Callers
  wrap `Box` into `Arc` at the sharing boundary.
- Tools: `Vec<Box<dyn Tool>>` from `all_tools()` / `default_tools()`,
  registered into `ToolRegistry`.

---

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

No `rustfmt.toml` — default rustfmt. Run `cargo fmt` before committing.
Indent: **4 spaces** for `.rs` / `.sh` / `Dockerfile`, **2 spaces** for
`.toml` / `.yml` / `.yaml` / `.json`.
LF line endings, UTF-8, trailing newline (see `.editorconfig`).
Max line length: **100 characters** for `.rs` files.

### Imports

`cargo fmt` manages ordering. Group: `crate::` then external crates
(alphabetized) then `std::`.
Merge from same crate: `use serde::{Deserialize, Serialize};`

### Types & Naming

- Structs/enums: `PascalCase`. Functions: `snake_case`. Constants:
  `SCREAMING_SNAKE_CASE`.
- `#[serde(rename_all = "snake_case")]` on serialized enums.
- `#[serde(tag = "kind", rename_all = "snake_case")]` for tagged enums.
- Standard derives: `#[derive(Debug, Clone, Serialize, Deserialize)]`. Add
  `PartialEq, Eq` for comparisons.
- `strum::Display` + `#[strum(serialize_all = "snake_case")]` for string
  representation.

### Error Handling

- `anyhow::Result<T>` for fallible public functions. `anyhow::bail!()` for
  early exits.
- `thiserror::Error` for structured errors at library boundaries.
- **No** `unwrap()` / `expect()` in production code (OK in tests). Empty catch
  blocks forbidden.

### Async & Constructors

- Dyn-safe async traits: `fn method(&self) ->
  Pin<Box<dyn Future<Output = R> + Send + '_>>`, impl:
  `Box::pin(async move { ... })`. Non-dyn traits: native `async fn in trait`.
- `Arc<dyn Trait>` across async boundaries. Tokio `rt-multi-thread`.
- Rust 2024: `if let` chains are used (e.g. `if let Some(x) = opt && cond`).
- `fn new(...)` with `impl Into<String>` for string params.
- Builder: `fn with_field(mut self, value: T) -> Self`. Named constructors for
  variants.

### Feature Gates

Default features: `discord`, `telegram`, `slack`, `matrix`, `irc`, `whatsapp`,
`imessage`, `email`, `vector-search`, `tui`, `bundled-sqlite`, `media`,
`link-extraction`.

Optional: `lancedb` (vector-search), `ratatui`/`crossterm` (tui),
`lettre`/`mail-parser` (email), `infer`/`mime` (media), `scraper`
(link-extraction), `rmcp` (mcp), `fastembed` (embeddings).

---

## Test Structure

Seven integration test binaries under `tests/`:
`memory.rs`, `gateway.rs`, `agent.rs`, `persona.rs`, `runtime.rs`,
`project.rs`, `taste.rs`.

**Critical**: Integration tests use explicit `#[path]` — implicit directory
resolution does NOT work:

```rust
// tests/memory.rs — correct pattern
#[path = "support/memory_harness.rs"]
mod memory_harness;
#[path = "memory/comparison.rs"]
mod comparison;
```

Child modules: `use super::memory_harness;`. Shared helpers live in
`tests/support/`.

- Unit tests: `#[cfg(test)] mod tests` at file bottom. `tokio::test` for
  async.
- `tempfile::TempDir` for filesystem isolation. `unwrap()` / `expect()` OK
  in tests.
- **Known skip**: `inventory_scope_lock::inventory_scope_lock` (skillforge
  data drift — not a regression).
- `BACKEND=<sqlite|lancedb|markdown>` env var selects memory backend in
  integration tests.

---

## Security (Non-Negotiable)

1. **Deny-by-default** shell/file allowlist (`security/policy/`)
2. **Public bind refused** unless tunnel or explicit opt-in
   (`security/pairing.rs`)
3. **ChaCha20-Poly1305** encrypted vault (`security/secrets.rs`)
4. **Writeback guard** prevents persona self-corruption
   (`security/writeback_guard/`)
5. **Secret scrubbing** strips tokens/keys from LLM I/O

---

## Dependencies

Release profile: `opt-level = "z"`, LTO, `codegen-units = 1`,
`panic = "abort"`, strip symbols.

Justify new deps, minimise features, disable defaults.

Allowed licenses: MIT, Apache-2.0, BSD-2/3-Clause, ISC, MPL-2.0, Zlib,
BSL-1.0, 0BSD, CC0-1.0 (see `deny.toml`).

---

## Commits

Conventional Commits: `feat:`, `fix:`, `docs:`, `test:`, `refactor:`,
`chore:`.
Small, coherent, one logical change per commit.

---

## Adding a Channel

Implement `Channel` trait in `src/transport/channels/<name>.rs`, add to
factory in `src/transport/channels/factory.rs`, re-export from
`src/transport/channels/mod.rs`. Keep `mod.rs` as thin facade with re-exports
only.
