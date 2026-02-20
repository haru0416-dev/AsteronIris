<h1 align="center">AsteronIris</h1>

<p align="center">
  <strong>Secure, extensible AI assistant built in Rust</strong><br>
  CLI &middot; Daemon &middot; Multi-Channel I/O &middot; Pluggable Memory &middot; Hardened Gateway
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
  <a href="https://www.rust-lang.org"><img alt="Rust" src="https://img.shields.io/badge/rust-2024_edition-orange.svg"></a>
  <img alt="Platform" src="https://img.shields.io/badge/platform-linux%20%7C%20macos%20%7C%20windows-lightgrey.svg">
</p>

> [!WARNING]
> **This project is under active development.** APIs, configuration schema, and
> behavior may change without notice. Not recommended for production use yet.

---

## Highlights

| | Feature | Description |
|---|---|---|
| &#9889; | **Fast CLI** | Rust-native binary with onboarding wizard and built-in diagnostics |
| &#129302; | **Agent Mode** | Interactive or single-message execution with tool use and reflection |
| &#127760; | **Gateway** | Axum HTTP server with pairing auth, request limits, and timeout guardrails |
| &#128126; | **Daemon** | Long-running supervisor — gateway + channels + heartbeat + scheduler |
| &#128451; | **Memory** | Pluggable backends: SQLite, LanceDB, Markdown, None |
| &#128172; | **Channels** | CLI, Telegram, Discord, Slack, iMessage, Matrix, WhatsApp, Email, IRC |
| &#128274; | **Security** | Deny-by-default policy, encrypted vault, workspace-scoped access |

---

## Quick Start

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) stable toolchain
- Git

### Install

```bash
git clone https://github.com/haru0416-dev/AsteronIris.git
cd AsteronIris
cargo build --release
```

### First Run

```bash
# Initialize workspace and config
asteroniris onboard

# Interactive agent
asteroniris agent

# One-shot message
asteroniris agent --message "Summarize today tasks"
```

> [!TIP]
> Run `asteroniris doctor` at any time to check your setup and diagnose issues.

---

## Usage

### Core Commands

| Command | Description |
|---------|-------------|
| `asteroniris onboard` | Initialize or reconfigure workspace |
| `asteroniris agent` | Run the assistant loop |
| `asteroniris gateway` | Start the HTTP gateway |
| `asteroniris daemon` | Run the long-lived autonomous runtime |
| `asteroniris doctor` | Run diagnostics |
| `asteroniris status` | Show effective config and runtime summary |

<details>
<summary><strong>More commands</strong></summary>

| Command | Description |
|---------|-------------|
| `asteroniris channel list\|start\|doctor` | Channel management |
| `asteroniris cron list\|add\|remove` | Scheduler management |
| `asteroniris auth list\|status\|login\|oauth-login\|oauth-status` | Provider auth profiles |
| `asteroniris skills list\|install\|remove` | Skill management |
| `asteroniris integrations info <name>` | Integration details |
| `asteroniris service install\|start\|stop\|status\|uninstall` | OS service lifecycle |

</details>

### Gateway

```bash
asteroniris gateway --host 127.0.0.1 --port 8080
```

---

## Configuration

Default paths:

```
~/.asteroniris/config.toml    # Configuration
~/.asteroniris/workspace      # Workspace data
```

<details>
<summary><strong>Config sections</strong></summary>

| Section | Purpose |
|---------|---------|
| `[memory]` | Backend and retention policy |
| `[gateway]` | Bind host/port and pairing |
| `[channels]` | Tokens and allowlists |
| `[autonomy]` | Command/path policy and limits |
| `[observability]` | Backend (`none` or `log`) |
| `[runtime]` | Runtime settings |
| `[reliability]` | Retry and resilience |
| `[heartbeat]` | Heartbeat interval and behavior |

</details>

<details>
<summary><strong>Environment overrides</strong></summary>

| Variable | Purpose |
|----------|---------|
| `ASTERONIRIS_API_KEY` | LLM provider API key |
| `ASTERONIRIS_PROVIDER` | Provider name (e.g. `openrouter`) |
| `ASTERONIRIS_MODEL` | Model identifier |
| `ASTERONIRIS_WORKSPACE` | Workspace directory override |
| `ASTERONIRIS_GATEWAY_HOST` | Gateway bind host |
| `ASTERONIRIS_GATEWAY_PORT` | Gateway bind port |
| `ASTERONIRIS_TEMPERATURE` | Sampling temperature |

See [`.env.example`](.env.example) for a ready-to-use template.

</details>

### Autonomy Rollout Gates

Autonomy extensions ship safe by default (`[autonomy.rollout]` in `config.toml`).
All gates default to **off**. Use `asteroniris status` or `asteroniris doctor` to
inspect the current rollout posture.

---

## Security

> [!IMPORTANT]
> AsteronIris follows a **deny-by-default** security posture. All shell commands,
> file access, and network binds require explicit allowlisting.

- Gateway binds to **localhost only** — public bind is blocked unless explicitly allowed
- Request size (64 KB) and timeout (30 s) limits enforced
- [ChaCha20-Poly1305](https://en.wikipedia.org/wiki/ChaCha20-Poly1305) encrypted secret vault
- Workspace-scoped file and memory access
- Secret scrubbing on all LLM I/O

See [`SECURITY.md`](SECURITY.md) for the full security policy and vulnerability
reporting instructions.

---

## Development

```bash
cargo fmt                     # Format
cargo clippy -- -D warnings   # Lint (CI treats warnings as errors)
cargo test                    # All tests
cargo test-dev                # 4-thread parallel (faster local)
```

```bash
# Docker build
docker build -t asteroniris .
```

> [!NOTE]
> A pre-push hook enforces `fmt + clippy + test`. Enable it with:
> ```bash
> git config core.hooksPath .githooks
> ```

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full development workflow
and [`AGENTS.md`](AGENTS.md) for architecture and code-style reference.

---

## Project Structure

```
src/
├── agent/        # Conversation loop, tool execution, reflection
├── channels/     # Channel integrations (CLI, Telegram, Discord, …)
├── config/       # TOML schema + env-var overrides
├── daemon/       # Supervisor: gateway + channels + heartbeat + cron
├── gateway/      # Axum HTTP server (pairing, webhooks, autosave)
├── memory/       # Memory backends (SQLite, LanceDB, Markdown)
├── providers/    # LLM provider factory + secret scrubbing
├── security/     # Policy, pairing auth, encrypted vault, writeback guard
├── skillforge/   # Skill discovery, evaluation, integration
└── tools/        # Tool registry (shell, file, memory, browser, composio)
```

---

## License

[MIT](LICENSE)
