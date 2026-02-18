# Contributing to AsteronIris

Thanks for contributing.

## Development Setup

```bash
git clone https://github.com/haru0416-dev/AsteronIris.git
cd asteroniris

# Optional but recommended: enforce pre-push checks
git config core.hooksPath .githooks

cargo build
cargo fmt -- --check
cargo clippy -- -D warnings
cargo test
```

## Project Shape

Core extension points are trait-based:

- `src/providers/` -> model providers
- `src/channels/` -> messaging channels
  - Keep `src/channels/mod.rs` as a facade + re-exports; place channel-specific provider additions in `src/channels/providers/<channel>.rs`.
- `src/tools/` -> tool surface
- `src/memory/` -> memory backends
- `src/observability/` -> observability backends
- `src/tunnel/` -> tunnel adapters

When adding a feature, prefer extending an existing trait boundary over introducing cross-cutting logic.

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
- [ ] Docs updated (`README.md`, `docs/SPECS.md`, or others) when behavior changed
- [ ] No unnecessary new dependencies

## Commit Convention

Conventional Commits are recommended:

- `feat: ...`
- `fix: ...`
- `docs: ...`
- `test: ...`
- `refactor: ...`
- `chore: ...`

## Security Reporting

Do not open public issues for vulnerabilities.

Use:

- GitHub Security Advisories: <https://github.com/haru0416-dev/AsteronIris/security/advisories/new>
- See policy details in `SECURITY.md`

## License

By contributing, you agree your contributions are licensed under MIT.
