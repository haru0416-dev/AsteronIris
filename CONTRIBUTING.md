# Contributing to AsteronIris

Thanks for contributing.

## Development Setup

```bash
git clone https://github.com/haru0416-dev/AsteronIris.git
cd AsteronIris

# Recommended: enforce pre-push checks (fmt + clippy + test)
git config core.hooksPath .githooks

cargo build
cargo fmt -- --check
cargo clippy -- -D warnings
cargo test
```

For architecture details, code style, and test conventions see
[`AGENTS.md`](AGENTS.md).

## Project Shape

Core extension points are trait-based:

| Directory | Purpose |
|-----------|---------|
| `src/providers/` | Model providers |
| `src/channels/` | Messaging channels |
| `src/tools/` | Tool surface |
| `src/memory/` | Memory backends |
| `src/security/` | Security policy, vault, writeback guard |
| `src/observability/` | Observability backends |
| `src/tunnel/` | Tunnel adapters |

When adding a feature, prefer extending an existing trait boundary over
introducing cross-cutting logic.

## Contribution Flow

1. Create a focused branch.
2. Keep changes small and coherent.
3. Add or update tests for behavior changes.
4. Update docs when user-facing behavior changes.
5. Open a PR with clear rationale.

## Pull Request Checklist

- [ ] `cargo fmt -- --check`
- [ ] `cargo clippy -- -D warnings`
- [ ] `cargo test`
- [ ] Docs updated when user-facing behavior changed
- [ ] No unnecessary new dependencies

## Commit Convention

Conventional Commits format:

- `feat:` — new feature
- `fix:` — bug fix
- `docs:` — documentation only
- `test:` — add or update tests
- `refactor:` — code change that neither fixes a bug nor adds a feature
- `chore:` — maintenance, deps, CI

## Security Reporting

Do not open public issues for vulnerabilities.

- GitHub Security Advisories: <https://github.com/haru0416-dev/AsteronIris/security/advisories/new>
- See [`SECURITY.md`](SECURITY.md) for full policy details.

## License

By contributing, you agree your contributions are licensed under the
[MIT License](LICENSE).
