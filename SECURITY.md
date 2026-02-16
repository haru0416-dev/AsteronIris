# Security Policy

## Supported Versions

| Version | Supported |
|---|---|
| 0.1.x | yes |

## Reporting a Vulnerability

Do not disclose vulnerabilities in public issues.

Report privately via:

1. GitHub Security Advisories: <https://github.com/haru0416-dev/AsteronIris/security/advisories/new>
2. Maintainer contact through GitHub private channels

Include:

- clear description
- reproduction steps
- impact assessment
- suggested fix (optional)

## Response Targets

- Acknowledgment: within 48 hours
- Initial triage: within 7 days
- Critical fix target: within 14 days

## Security Model (Current)

Defense-in-depth defaults include:

- workspace-scoped file access
- path traversal protections
- command allowlists and forbidden paths
- gateway pairing/token flow
- localhost-first bind posture
- autonomy level controls (`readonly`, `supervised`, `full`)

## Operational Guidance

- Keep `workspace_only = true` unless you have a strict reason not to.
- Keep `gateway.allow_public_bind = false` unless tunnel/public exposure is intentional.
- Use pairing and token auth for webhook/gateway access.
- Avoid broad allowlists in channels unless needed for controlled testing.

## Verification

Before release or deployment hardening checks:

```bash
cargo fmt -- --check
cargo clippy -- -D warnings
cargo test
```

For container workflows, verify image/user/runtime settings using your deployment controls and CI policy.
