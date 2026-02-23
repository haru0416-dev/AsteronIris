# Wave 4 Decisions

<!-- Append only. Format: ## [TIMESTAMP] Task: {task-id}\n{content} -->

## [2026-02-23] Session Start
- Module path: src/core/taste/ (not src/taste/)
- Feature gate: taste = [] NOT in defaults
- BT params: eta=4.0, lambda=0.5, clamp ±35, log-space, L2 reg
- Rating threshold: n_comparisons >= 5
- 3 axes only: Coherence, Hierarchy, Intentionality
- 4C: instance-based CoordinationManager (not global static)
- Tests-after strategy
