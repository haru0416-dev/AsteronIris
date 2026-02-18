# AsteronIris

AsteronIris is a Rust-first AI assistant with a CLI, long-running daemon mode,
multi-channel I/O, memory backends, and a hardened local gateway.

## Highlights

- Fast Rust CLI (`asteroniris`) with onboarding and diagnostics
- Agent mode for interactive or single-message execution
- Gateway mode with pairing auth, request limits, and timeout guardrails
- Daemon mode that runs gateway + channels + heartbeat + scheduler
- Memory backends: `sqlite`, `lancedb`, `markdown`, `none`
- Channel integrations: CLI, Telegram, Discord, Slack, iMessage, Matrix, WhatsApp, Email, IRC
- Channel implementation note: future channel-specific providers should live under `src/channels/providers/<channel>.rs` to keep `channels/mod.rs` as a thin facade.
- Security controls: workspace-scoped policy, secret storage, deny-by-default checks

## Requirements

- Rust stable toolchain
- Git
- Linux/macOS/Windows supported

Optional (depending on features/workflows):

- Docker (container build/test)
- `protoc` (for dependency graphs that compile `lance-encoding`)

## Install

Clone and build:

```bash
git clone https://github.com/haru0416-dev/AsteronIris.git
cd AsteronIris
cargo build --release
```

Run binary:

```bash
./target/release/asteroniris --help
```

## Quick Start

Initialize workspace/config:

```bash
asteroniris onboard
```

Run agent in interactive mode:

```bash
asteroniris agent
```

Run agent with one message:

```bash
asteroniris agent --message "Summarize today tasks"
```

Start local gateway:

```bash
asteroniris gateway --host 127.0.0.1 --port 8080
```

Check runtime status:

```bash
asteroniris status
```

## Core Commands

- `asteroniris onboard` - initialize/reconfigure
- `asteroniris agent` - run assistant loop
- `asteroniris gateway` - start HTTP gateway
- `asteroniris daemon` - run long-lived autonomous runtime
- `asteroniris doctor` - diagnostics
- `asteroniris status` - effective config/runtime summary
- `asteroniris channel list|start|doctor` - channel management
- `asteroniris cron list|add|remove` - scheduler management
- `asteroniris auth list|status|login|oauth-login|oauth-status` - provider auth profile management
- `asteroniris skills list|install|remove` - skill management
- `asteroniris integrations info <name>` - integration details
- `asteroniris service install|start|stop|status|uninstall` - OS service lifecycle

## Configuration

By default:

- Config: `~/.asteroniris/config.toml`
- Workspace: `~/.asteroniris/workspace`

Common config sections:

- `[memory]` backend and retention
- `[gateway]` bind host/port and pairing
- `[channels]` tokens and allowlists
- `[autonomy]` command/path policy and limits
- `[observability]` backend (`none` or `log`)
- `[runtime]`, `[reliability]`, `[heartbeat]`

Useful environment overrides:

- `ASTERONIRIS_API_KEY`
- `ASTERONIRIS_PROVIDER`
- `ASTERONIRIS_MODEL`
- `ASTERONIRIS_WORKSPACE`
- `ASTERONIRIS_GATEWAY_HOST`
- `ASTERONIRIS_GATEWAY_PORT`
- `ASTERONIRIS_TEMPERATURE`

### Autonomy Rollout Gates

Autonomy extensions ship safe by default. New rollout controls live under
`[autonomy.rollout]` in `config.toml`:

- `stage = "off" | "audit-only" | "sanitize"` (default: `off`)
- `verify_repair_enabled = false` (default)
- `contradiction_weighting_enabled = false` (default)
- `intent_audit_anomaly_detection_enabled = false` (default)

`asteroniris status` and `asteroniris doctor` both report these values using:

- `Rollout stage: off|audit-only|sanitize`
- `Rollout gates: verify_repair=on|off, contradiction_weighting=on|off, intent_audit_anomaly_detection=on|off`

Operator rollout/SLO/rollback runbook is maintained privately and excluded from the public repository.

## Security Notes

- Gateway defaults to localhost bind and pairing enabled
- Public bind is blocked unless explicitly allowed
- Request size and timeout limits are enforced in gateway mode
- Memory and tool actions are designed to stay workspace-scoped by default

## Development

Format:

```bash
cargo fmt
```

Lint:

```bash
cargo clippy -- -D warnings
```

Test:

```bash
cargo test
```

Single integration test examples:

```bash
cargo test --test memory_comparison
BACKEND=sqlite cargo test --release --test memory_throughput -- --nocapture
```

Docker build:

```bash
docker build -t asteroniris .
```

## License

MIT
