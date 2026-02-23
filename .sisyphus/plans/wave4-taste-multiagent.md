# Wave 4: Taste Engine + Multi-Agent Coordination

## TL;DR

> **Quick Summary**: Implement the Taste Engine (aesthetic evaluation via LLM-based critic + pair comparison learning) and Multi-Agent coordination layer (role-based agents, session sharing, parallel dispatch) — the two remaining pillars of AsteronIris's Wave 4 roadmap.
> 
> **Deliverables**:
> - `src/core/taste/` module with feature gate `taste`: types, engine trait+factory, LLM critic (3 axes), text+UI domain adapters
> - `taste.evaluate` and `taste.compare` tools registered in ToolRegistry
> - TasteStore (SQLite persistence) + TasteLearner (Bradley-Terry online model)
> - Multi-agent role abstraction, session coordination, parallel dispatch+aggregation in `src/core/subagents/`
> - TasteConfig in TOML schema, integration tests in `tests/taste.rs`
> 
> **Estimated Effort**: Large (~17d)
> **Parallel Execution**: YES — 5 waves + final verification
> **Critical Path**: T1→T2→T5→T9→T10→T13→T14→T16→F1-F4

---

## Context

### Original Request
docs/IMPLEMENTATION_PLAN.md の Wave 4 を実装。Taste Engine (4A: Text+UI critic, 4B: Pair comparison learning) + Multi-Agent (4C: Role/Session/Parallel dispatch フル実装)。

### Interview Summary
**Key Discussions**:
- Scope: Wave 4 only (Wave 5 excluded)
- Test strategy: Tests-after (implementation first)
- 4C: Full implementation (not just interfaces)
- Module path: `src/core/taste/` (codebase convention, not `src/taste/` as design doc says)

**Research Findings**:
- Feature gate pattern: `taste = []` in Cargo.toml, `#[cfg(feature = "taste")]` in lib.rs
- Tool trait: name/description/parameters_schema/execute in src/core/tools/traits.rs
- Subagent runtime: 269 LOC, global static, spawn/run_inline/get/list/cancel
- Planner: 1,725 LOC, Plan/PlanStep/DagContract, PlanExecutor with StepRunner trait
- Session model: Session (id, channel, user_id, state, metadata), SessionStore for persistence
- Bradley-Terry: ~50 lines custom code, no crate dependency. Use LMSYS parameters (η=4.0, λ=0.5, clamp ±35)

### Metis Review
**Identified Gaps** (addressed):
- Critic soft confidence vs binary judgment → Use soft `[0.0, 1.0]` with binary fallback
- BT model specifics → LMSYS parameters: η=4.0, λ=0.5, sigmoid clamp ±35, log-space, L2 regularization
- TasteStore schema → append-only `taste_comparisons` + mutable `taste_ratings` cache (replayable)
- 4C subagent runtime uses global statics → refactor to instance-based if needed, or extend with role layer
- MUST NOT surface ratings until n_comparisons ≥ 5 per item
- Critic prompt template MUST include rubric definitions to prevent axis bleed (LLM-Rubric pattern)
- Multi-agent fan-out needs timeout + partial-failure handling

---

## Work Objectives

### Core Objective
Build the Taste Engine (aesthetic evaluation) and Multi-Agent coordination layer for AsteronIris, enabling LLM-driven quality assessment of text/UI artifacts with pair comparison learning, plus role-based multi-agent collaboration.

### Concrete Deliverables
- `src/core/taste/` module: types.rs, engine.rs, critic.rs, adapter.rs, store.rs, learner.rs, mod.rs
- `src/config/schema/taste.rs` — TasteConfig with serde defaults
- `src/core/tools/taste_evaluate.rs` + `src/core/tools/taste_compare.rs` — Tool impls
- `src/core/subagents/` extensions: roles.rs, coordination.rs, dispatch.rs
- `tests/taste.rs` integration test binary
- Feature gate `taste` in Cargo.toml

### Definition of Done
- [ ] `cargo fmt -- --check` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo test` passes (all existing 2,074+ tests + new taste/multi-agent tests)
- [ ] `cargo check --no-default-features` passes (taste OFF)
- [ ] `cargo check --features "taste"` passes (taste ON)
- [ ] `cargo check --lib && cargo check --bin asteroniris` passes

### Must Have
- All 2,074+ existing tests pass
- Security behavior preserved (deny-by-default, scrubbing on taste LLM I/O)
- `taste` feature gate works: disabled by default, `--features taste` enables
- `[taste] enabled = false` config disables engine even when feature compiled in
- TasteEngine trait follows existing trait + factory pattern
- Tool registration follows existing all_tools()/tool_descriptions() pattern
- Bradley-Terry uses log-space parameters, sigmoid clamp ±35, η=4.0, λ=0.5
- Multi-agent parallel dispatch has per-role timeout + partial-failure handling
- taste.evaluate returns 3-axis scores (Coherence/Hierarchy/Intentionality) + suggestions
- taste.compare persists to SQLite + updates BT ratings

### Must NOT Have (Guardrails)
- **No Image/Video/Audio perceivers** — Text + UI only (Phase 1-3)
- **No neural/embedding model dependencies** — LLM-only critic, no torch/onnx/candle
- **No all 7 axes** — Start with 3 (Coherence/Hierarchy/Intentionality). 4 more in Wave 5
- **No batch MLE (L-BFGS-B)** — Online BT updates only
- **No `skillratings` or BT crate** — Custom ~50 line implementation
- **No breaking changes to existing subagent API** — Extend, don't replace
- **No breaking Session/Provider/Memory traits** — Add methods with default impls only
- **No new external dependencies** except what's strictly needed (serde_json already present)
- **No TrueSkill** — Start with BT. TrueSkill is Phase 5
- **No surfacing ratings with fewer than 5 comparisons** — minimum sample threshold

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES (cargo test, 2,074 unit + 6 integration binaries)
- **Automated tests**: Tests-after (implementation first, tests follow in same task)
- **Framework**: cargo test (built-in)

### QA Policy
Every task MUST end with the **Standard Verification Gate**:
```bash
cargo fmt -- --check
cargo clippy -- -D warnings
cargo test
```

For feature-gated changes, add **Feature Matrix Gate**:
```bash
cargo check --no-default-features
cargo check --features "taste"
cargo check --lib
cargo check --bin asteroniris
```

Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Foundation — scaffolding + types + config):
├── Task 1: Feature gate + Cargo.toml + TasteConfig schema [quick]
├── Task 2: Type definitions (Artifact, TasteContext, TasteReport, Axis, Suggestion) [quick]
├── Task 3: TasteEngine trait + factory + mod.rs facade [quick]
└── Task 4: Multi-agent Role types + coordination types [quick]

Wave 2 (Core Engine — parallel implementation):
├── Task 5: LLM-based UniversalCritic (3 axes + rubric prompt) [deep]
├── Task 6: Text Domain Adapter (TextOp suggestions) [unspecified-high]
├── Task 7: UI Domain Adapter (UiOp suggestions) [unspecified-high]
└── Task 8: Multi-agent session coordination layer [deep]

Wave 3 (Tools + Storage — parallel):
├── Task 9: taste.evaluate Tool implementation + registration [unspecified-high]
├── Task 10: TasteStore SQLite persistence (comparisons + ratings tables) [unspecified-high]
├── Task 11: Multi-agent parallel dispatch + result aggregation [deep]
└── Task 12: taste.evaluate integration tests [unspecified-high]

Wave 4 (Learning + Multi-agent tests):
├── Task 13: TasteLearner (Bradley-Terry online model) [deep]
├── Task 14: taste.compare Tool implementation + registration [unspecified-high]
├── Task 15: Multi-agent integration tests [unspecified-high]
└── Task 16: Taste integration test binary (tests/taste.rs) [unspecified-high]

Wave 5 (Polish + Full QA):
├── Task 17: Feature gate verification + doc comments [quick]
└── Task 18: Full feature matrix regression [unspecified-high]

Wave FINAL (After ALL tasks — independent review, 4 parallel):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Full regression QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)

Critical Path: T1→T2→T5→T9→T10→T13→T14→T16→F1-F4
Parallel Speedup: ~60% faster than sequential
Max Concurrent: 4 (Waves 2, 3)
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave |
|------|-----------|--------|------|
| 1 | — | 2, 3, 5-7, 9-10, 17 | 1 |
| 2 | 1 | 3, 5-7, 9, 13-14 | 1 |
| 3 | 1, 2 | 5-7, 9, 14 | 1 |
| 4 | — | 8, 11, 15 | 1 |
| 5 | 2, 3 | 6, 7, 9, 12 | 2 |
| 6 | 2, 5 | 9, 12 | 2 |
| 7 | 2, 5 | 9, 12 | 2 |
| 8 | 4 | 11, 15 | 2 |
| 9 | 3, 5, 6, 7 | 12, 16 | 3 |
| 10 | 1 | 13, 14 | 3 |
| 11 | 8 | 15 | 3 |
| 12 | 9 | 16 | 3 |
| 13 | 10 | 14 | 4 |
| 14 | 3, 10, 13 | 16 | 4 |
| 15 | 11, 8 | 18 | 4 |
| 16 | 9, 12, 14 | 18 | 4 |
| 17 | 1-16 | 18 | 5 |
| 18 | 17 | F1-F4 | 5 |
| F1-F4 | 18 | — | FINAL |

### Agent Dispatch Summary

- **Wave 1**: 4 tasks → T1-T3 `quick`, T4 `quick`
- **Wave 2**: 4 tasks → T5 `deep`, T6-T7 `unspecified-high`, T8 `deep`
- **Wave 3**: 4 tasks → T9 `unspecified-high`, T10 `unspecified-high`, T11 `deep`, T12 `unspecified-high`
- **Wave 4**: 4 tasks → T13 `deep`, T14 `unspecified-high`, T15-T16 `unspecified-high`
- **Wave 5**: 2 tasks → T17 `quick`, T18 `unspecified-high`
- **FINAL**: 4 tasks → F1 `oracle`, F2-F3 `unspecified-high`, F4 `deep`

---

## TODOs

> Implementation + Test = ONE Task. Never separate.
> EVERY task MUST have: Recommended Agent Profile + Parallelization info + QA Scenarios.

### Wave 1 — Foundation (scaffolding + types + config)

- [ ] 1. Feature Gate + Cargo.toml + TasteConfig Schema

  **What to do**:
  - Add `taste = []` to `[features]` in `Cargo.toml`. Do NOT add to `default` features.
  - Create `src/config/schema/taste.rs` with `TasteConfig` struct:
    ```rust
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct TasteConfig {
        #[serde(default)]
        pub enabled: bool,
        #[serde(default = "default_taste_backend")]
        pub backend: String,  // "llm"
        #[serde(default = "default_taste_axes")]
        pub axes: Vec<String>,  // ["coherence", "hierarchy", "intentionality"]
        #[serde(default)]
        pub text_enabled: bool,  // default true when taste enabled
        #[serde(default)]
        pub ui_enabled: bool,    // default true when taste enabled
    }
    ```
    Implement `Default` for `TasteConfig` with `enabled: false`, `backend: "llm"`, `axes: ["coherence", "hierarchy", "intentionality"]`, `text_enabled: true`, `ui_enabled: true`.
  - Register in `src/config/schema/mod.rs`: add `mod taste;` and `pub use taste::TasteConfig;`
  - Add `#[serde(default)] pub taste: TasteConfig` field to `Config` struct in `src/config/schema/core/types.rs`
  - Add `taste: TasteConfig::default()` to `Config::default()` impl
  - Add import of `TasteConfig` in `src/config/schema/core/types.rs` from `super::super::TasteConfig`
  - Create empty `src/core/taste/mod.rs` with placeholder comment
  - Add `#[cfg(feature = "taste")] pub mod taste;` to `src/core/mod.rs`

  **Must NOT do**:
  - Do NOT add `taste` to default features
  - Do NOT add any runtime logic yet — config + gates only
  - Do NOT add image/video/audio config fields

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Small changes across 5 files, no complex logic
  - **Skills**: `[]`
  - **Skills Evaluated but Omitted**:
    - `playwright`: No browser interaction needed

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T4)
  - **Parallel Group**: Wave 1
  - **Blocks**: T2, T3, T5, T6, T7, T9, T10, T17
  - **Blocked By**: None (can start immediately)

  **References**:

  **Pattern References**:
  - `Cargo.toml` features section — existing feature gate patterns (e.g., `mcp`, `media`, `tui`)
  - `src/config/schema/core/types.rs:11-77` — Config struct with `#[serde(default)]` fields pattern
  - `src/config/schema/core/types.rs:311-344` — Config::default() impl showing all field defaults
  - `src/config/schema/mod.rs:1-33` — Module registration + pub use re-export pattern
  - `src/core/mod.rs:1-9` — Core module registration pattern

  **API/Type References**:
  - `src/config/schema/core/types.rs:150-158` — BrowserConfig as a minimal config struct pattern (enabled + fields + Default derive)

  **WHY Each Reference Matters**:
  - Cargo.toml features: Must follow exact same empty-array syntax as existing features
  - Config struct: Must match serde(default) annotation pattern for TOML deserialization
  - config/schema/mod.rs: Must follow mod + pub use pattern for new config modules
  - core/mod.rs: Must add cfg-gated module in same style as other modules

  **Acceptance Criteria**:
  - [ ] `cargo check` passes (taste not in defaults, so no taste code compiled)
  - [ ] `cargo check --features taste` passes (empty taste module compiles)
  - [ ] `cargo check --no-default-features` passes
  - [ ] TasteConfig round-trips through TOML (serialize + deserialize)
  - [ ] `Config::default().taste.enabled` is `false`

  **QA Scenarios:**

  ```
  Scenario: Feature gate compiles without taste
    Tool: Bash
    Preconditions: Clean build state
    Steps:
      1. Run `cargo check --no-default-features`
      2. Run `cargo check` (default features, taste not included)
    Expected Result: Both commands exit 0
    Failure Indicators: Compilation error mentioning `taste`
    Evidence: .sisyphus/evidence/task-1-feature-gate-no-taste.txt

  Scenario: Feature gate compiles with taste
    Tool: Bash
    Preconditions: Clean build state
    Steps:
      1. Run `cargo check --features taste`
    Expected Result: Exit 0, no errors
    Failure Indicators: Unresolved import or missing module
    Evidence: .sisyphus/evidence/task-1-feature-gate-with-taste.txt

  Scenario: TasteConfig TOML round-trip
    Tool: Bash (cargo test)
    Preconditions: Test written as part of this task
    Steps:
      1. Write a unit test in `src/config/schema/taste.rs` that creates TasteConfig::default(), serializes to TOML, deserializes, asserts fields match
      2. Run `cargo test --features taste -- taste`
    Expected Result: Test passes, all default values preserved
    Failure Indicators: Assertion failure on any field
    Evidence: .sisyphus/evidence/task-1-toml-roundtrip.txt
  ```

  **Evidence to Capture:**
  - [ ] task-1-feature-gate-no-taste.txt
  - [ ] task-1-feature-gate-with-taste.txt
  - [ ] task-1-toml-roundtrip.txt

  **Commit**: YES (groups with T2, T3, T4)
  - Message: `feat(taste): add feature gate, TasteConfig schema, and module scaffold`
  - Files: `Cargo.toml`, `src/config/schema/taste.rs`, `src/config/schema/mod.rs`, `src/config/schema/core/types.rs`, `src/core/taste/mod.rs`, `src/core/mod.rs`
  - Pre-commit: `cargo check --features taste && cargo test --features taste`

---

- [ ] 2. Type Definitions (Artifact, TasteContext, TasteReport, Axis, Suggestion, PairComparison)

  **What to do**:
  - Create `src/core/taste/types.rs` with all core taste types:
    - `Artifact` enum: only `Text { content: String, format: Option<TextFormat> }` and `Ui { description: String, metadata: Option<serde_json::Value> }` variants. NO Image/Audio/Video.
    - `TextFormat` enum: `Plain`, `Markdown`, `Html` with `#[serde(rename_all = "snake_case")]`
    - `Domain` enum: `Text`, `Ui`, `General` with `#[serde(rename_all = "snake_case")]` + `strum::Display` + `#[strum(serialize_all = "snake_case")]` + `Default` (default = General)
    - `Axis` enum: `Coherence`, `Hierarchy`, `Intentionality` (only 3, NOT 7) with `#[serde(rename_all = "snake_case")]` + `strum::Display` + `Ord/PartialOrd/Eq/PartialEq/Hash` for BTreeMap key
    - `TasteContext` struct: domain (Domain), genre (Option<String>), purpose (Option<String>), audience (Option<String>), constraints (Vec<String>), extra (serde_json::Map<String, serde_json::Value>) — with Default derive
    - `AxisScores` type alias: `BTreeMap<Axis, f64>`
    - `TasteReport` struct: axis (AxisScores), domain (Domain), suggestions (Vec<Suggestion>), raw_critique (Option<String>)
    - `Suggestion` tagged enum: `General { title, rationale, priority }`, `Text { op: TextOp, rationale, priority }`, `Ui { op: UiOp, rationale, priority }` with `#[serde(tag = "kind", rename_all = "snake_case")]`
    - `Priority` enum: `High`, `Medium`, `Low` with serde + strum
    - `TextOp` enum: `RestructureArgument`, `AdjustDensity`, `UnifyStyle`, `AddOutline`, `Other(String)` with serde rename_all
    - `UiOp` enum: `AdjustLayout`, `ImproveHierarchy`, `AddContrast`, `RefineSpacing`, `Other(String)` with serde rename_all
    - `Winner` enum: `Left`, `Right`, `Tie`, `Abstain` with serde
    - `PairComparison` struct: domain, ctx (TasteContext), left_id (String), right_id (String), winner (Winner), rationale (Option<String>), created_at_ms (u64)
    - All types: `#[derive(Debug, Clone, Serialize, Deserialize)]`. Add `PartialEq` where useful for testing.
  - Add `pub mod types;` and `pub use types::*;` to `src/core/taste/mod.rs`

  **Must NOT do**:
  - Do NOT add Image/Video/Audio/Interaction Artifact variants
  - Do NOT add Rhythm/Contrast/Craft/Novelty axes
  - Do NOT add VideoOp/AudioOp suggestion variants
  - Do NOT implement any logic — pure data definitions only

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Pure type definitions, no complex logic, single file creation
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on T1)
  - **Parallel Group**: Wave 1 (after T1)
  - **Blocks**: T3, T5, T6, T7, T9, T13, T14
  - **Blocked By**: T1 (needs taste module to exist)

  **References**:

  **Pattern References**:
  - `src/core/sessions/types.rs` — Session/SessionState enum/struct pattern with serde derives
  - `src/core/planner/types.rs` — Plan/PlanStep/StepAction tagged enum pattern
  - `docs/taste-engine-design.md:264-315` — Canonical type definitions from design doc (§7 主要データ型)

  **API/Type References**:
  - `docs/taste-engine-design.md:286-289` — Axis enum (design doc has 7, we use only 3)
  - `docs/taste-engine-design.md:298-305` — Suggestion tagged enum pattern
  - `docs/taste-engine-design.md:306-314` — PairComparison struct

  **WHY Each Reference Matters**:
  - sessions/types.rs: Shows codebase convention for serde-annotated enums with rename_all
  - planner/types.rs: Shows tagged enum pattern (#[serde(tag = "kind")])
  - taste-engine-design.md: Source of truth for type shape, but adapted (3 axes, 2 domains only)

  **Acceptance Criteria**:
  - [ ] All types compile with `cargo check --features taste`
  - [ ] All types serialize/deserialize through serde_json round-trip
  - [ ] Axis is usable as BTreeMap key (Ord + Eq + Hash)
  - [ ] Only 3 axes exist (Coherence, Hierarchy, Intentionality)
  - [ ] Only Text + Ui artifact variants exist

  **QA Scenarios:**

  ```
  Scenario: Types compile and are accessible
    Tool: Bash
    Preconditions: T1 complete
    Steps:
      1. Run `cargo check --features taste`
      2. Verify `src/core/taste/types.rs` exists and contains Artifact, TasteContext, TasteReport, Axis
    Expected Result: Compilation succeeds, all types defined
    Failure Indicators: Missing type error, unresolved import
    Evidence: .sisyphus/evidence/task-2-types-compile.txt

  Scenario: Serde round-trip for all types
    Tool: Bash (cargo test)
    Preconditions: Unit tests written in types.rs
    Steps:
      1. Write unit tests: create each type, serialize to JSON, deserialize, assert equality
      2. Run `cargo test --features taste -- taste`
    Expected Result: All round-trip tests pass
    Failure Indicators: Serde error on tagged enum or BTreeMap<Axis, f64>
    Evidence: .sisyphus/evidence/task-2-serde-roundtrip.txt

  Scenario: Axis enum has exactly 3 variants
    Tool: Bash (grep)
    Preconditions: types.rs written
    Steps:
      1. Count Axis variants in src/core/taste/types.rs (expect exactly Coherence, Hierarchy, Intentionality)
      2. Verify NO Rhythm, Contrast, Craft, Novelty
    Expected Result: Exactly 3 variants, no forbidden variants
    Failure Indicators: Extra axis variant found
    Evidence: .sisyphus/evidence/task-2-axis-count.txt
  ```

  **Evidence to Capture:**
  - [ ] task-2-types-compile.txt
  - [ ] task-2-serde-roundtrip.txt
  - [ ] task-2-axis-count.txt

  **Commit**: YES (groups with T1, T3, T4)
  - Message: `feat(taste): add core type definitions (Artifact, TasteReport, Axis, PairComparison)`
  - Files: `src/core/taste/types.rs`, `src/core/taste/mod.rs`
  - Pre-commit: `cargo check --features taste && cargo test --features taste`

---

- [ ] 3. TasteEngine Trait + Factory + mod.rs Facade

  **What to do**:
  - Create `src/core/taste/engine.rs` with:
    ```rust
    #[async_trait]
    pub trait TasteEngine: Send + Sync {
        async fn evaluate(&self, artifact: &Artifact, ctx: &TasteContext) -> anyhow::Result<TasteReport>;
        async fn compare(&self, comparison: &PairComparison) -> anyhow::Result<()>;
        fn enabled(&self) -> bool;
    }
    ```
  - Add `DefaultTasteEngine` struct that holds:
    - `config: TasteConfig`
    - `critic: Arc<dyn UniversalCritic>` (forward-declare trait, impl in T5)
    - `adapters: HashMap<Domain, Arc<dyn DomainAdapter>>` (forward-declare trait, impl in T6/T7)
    - `store: Option<Arc<dyn TasteStore>>` (forward-declare trait, impl in T10)
    - `learner: Option<Arc<dyn TasteLearner>>` (forward-declare trait, impl in T13)
  - Add factory function: `pub fn create_taste_engine(config: &TasteConfig) -> anyhow::Result<Arc<dyn TasteEngine>>`
    - For now, return an error or a stub that returns enabled=false (real wiring in T9 when all components exist)
  - Forward-declare internal traits in separate files (just trait definition, no impl):
    - `src/core/taste/critic.rs`: `pub(crate) trait UniversalCritic: Send + Sync { async fn critique(&self, artifact: &Artifact, ctx: &TasteContext) -> Result<CritiqueResult>; }` + `CritiqueResult` struct
    - `src/core/taste/adapter.rs`: `pub(crate) trait DomainAdapter: Send + Sync { fn domain(&self) -> Domain; fn suggest(&self, critique: &CritiqueResult, ctx: &TasteContext) -> Vec<Suggestion>; }`
    - `src/core/taste/store.rs`: `pub(crate) trait TasteStore: Send + Sync { async fn save_comparison(...); async fn get_ratings(...); ... }`
    - `src/core/taste/learner.rs`: `pub(crate) trait TasteLearner: Send + Sync { fn update(...); fn get_rating(...); ... }`
  - Update `src/core/taste/mod.rs` to be a thin facade:
    ```rust
    pub mod types;
    pub mod engine;
    pub(crate) mod critic;
    pub(crate) mod adapter;
    pub(crate) mod store;
    pub(crate) mod learner;
    pub use types::*;
    pub use engine::{TasteEngine, create_taste_engine};
    ```

  **Must NOT do**:
  - Do NOT implement actual evaluate/compare logic — stubs only
  - Do NOT add neural/embedding dependencies
  - Do NOT break existing code (everything behind #[cfg(feature = "taste")])

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Trait definitions + stub factory, no complex algorithm logic
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on T1, T2)
  - **Parallel Group**: Wave 1 (after T2)
  - **Blocks**: T5, T6, T7, T9, T14
  - **Blocked By**: T1, T2

  **References**:

  **Pattern References**:
  - `src/core/memory/mod.rs` — Memory trait + create_memory() factory pattern
  - `src/core/providers/mod.rs` — Provider trait pattern (async_trait, Send + Sync)
  - `docs/taste-engine-design.md:252-258` — TasteEngine trait signature from design doc
  - `docs/taste-engine-design.md:318-324` — Factory function pattern

  **API/Type References**:
  - `src/core/taste/types.rs` — All types from T2 (Artifact, TasteContext, TasteReport, etc.)

  **WHY Each Reference Matters**:
  - memory/mod.rs: Canonical example of trait + factory + Arc<dyn Trait> pattern
  - providers/mod.rs: Shows async_trait usage convention
  - taste-engine-design.md: Source of truth for trait API signatures

  **Acceptance Criteria**:
  - [ ] TasteEngine trait defined with evaluate + compare + enabled methods
  - [ ] DefaultTasteEngine struct defined with correct field types
  - [ ] create_taste_engine factory compiles
  - [ ] Internal traits (UniversalCritic, DomainAdapter, TasteStore, TasteLearner) defined
  - [ ] mod.rs re-exports all public types correctly
  - [ ] `cargo check --features taste` passes

  **QA Scenarios:**

  ```
  Scenario: Trait and factory compile
    Tool: Bash
    Preconditions: T1, T2 complete
    Steps:
      1. Run `cargo check --features taste`
      2. Verify `src/core/taste/engine.rs` contains `pub trait TasteEngine`
      3. Verify `src/core/taste/mod.rs` re-exports TasteEngine
    Expected Result: Clean compilation
    Failure Indicators: Unresolved type, missing import, trait object safety issue
    Evidence: .sisyphus/evidence/task-3-trait-compile.txt

  Scenario: Internal traits are crate-private
    Tool: Bash (grep)
    Preconditions: All files written
    Steps:
      1. Verify critic.rs, adapter.rs, store.rs, learner.rs use `pub(crate)` not `pub` for traits
    Expected Result: Traits are pub(crate), not pub
    Failure Indicators: pub trait without (crate) qualifier
    Evidence: .sisyphus/evidence/task-3-visibility.txt
  ```

  **Evidence to Capture:**
  - [ ] task-3-trait-compile.txt
  - [ ] task-3-visibility.txt

  **Commit**: YES (groups with T1, T2, T4)
  - Message: `feat(taste): add TasteEngine trait, factory, and internal trait definitions`
  - Files: `src/core/taste/engine.rs`, `src/core/taste/critic.rs`, `src/core/taste/adapter.rs`, `src/core/taste/store.rs`, `src/core/taste/learner.rs`, `src/core/taste/mod.rs`
  - Pre-commit: `cargo check --features taste`

---

- [ ] 4. Multi-Agent Role Types + Coordination Types

  **What to do**:
  - Create `src/core/subagents/roles.rs` with:
    - `AgentRole` enum: `Planner`, `Executor`, `Reviewer`, `Critic`, `Custom(String)` with `#[serde(rename_all = "snake_case")]` + strum::Display
    - `RoleConfig` struct: role (AgentRole), system_prompt_override (Option<String>), model_override (Option<String>), temperature_override (Option<f64>), timeout_secs (Option<u64>)
    - `RoleAssignment` struct: run_id (String), role (AgentRole), config (RoleConfig), assigned_at (String)
  - Create `src/core/subagents/coordination.rs` with:
    - `CoordinationSession` struct: session_id (String), roles (Vec<RoleAssignment>), shared_context (SharedContext), created_at (String)
    - `SharedContext` struct: messages (Vec<ContextMessage>), artifacts (HashMap<String, serde_json::Value>), metadata (serde_json::Map<String, serde_json::Value>)
    - `ContextMessage` struct: role (AgentRole), content (String), timestamp (String)
    - `DispatchResult` struct: run_id (String), role (AgentRole), status (SubagentRunStatus), output (Option<String>), error (Option<String>), elapsed_ms (u64)
    - `AggregatedResult` struct: session_id (String), results (Vec<DispatchResult>), total_elapsed_ms (u64), all_succeeded (bool)
  - Create `src/core/subagents/dispatch.rs` with only type stubs + placeholder functions:
    - `pub async fn dispatch_parallel(session: &CoordinationSession, tasks: Vec<(AgentRole, String)>) -> Result<AggregatedResult>` — return `bail!("not implemented")` for now
  - Update `src/core/subagents/mod.rs` to add:
    ```rust
    pub mod roles;
    pub mod coordination;
    pub mod dispatch;
    ```
    Keep ALL existing functions/types unchanged.

  **Must NOT do**:
  - Do NOT modify existing subagent functions (configure_runtime, run_inline, spawn, get, list, cancel)
  - Do NOT remove or rename existing types (SubagentRuntimeConfig, SubagentRunStatus, etc.)
  - Do NOT add implementation logic for dispatch — types + stubs only

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Type definitions + module registration, no complex logic
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T1, T2, T3)
  - **Parallel Group**: Wave 1
  - **Blocks**: T8, T11, T15
  - **Blocked By**: None (can start immediately, independent of taste tasks)

  **References**:

  **Pattern References**:
  - `src/core/subagents/mod.rs:10-16` — SubagentRuntimeConfig struct pattern
  - `src/core/subagents/mod.rs:18-25` — SubagentRunStatus enum with serde rename_all
  - `src/core/subagents/mod.rs:27-38` — SubagentRunSnapshot struct pattern
  - `docs/IMPLEMENTATION_PLAN.md:317-323` — 4C task descriptions (Role/Session/Parallel)

  **API/Type References**:
  - `src/core/subagents/mod.rs:20-25` — SubagentRunStatus reused in DispatchResult
  - `src/core/sessions/types.rs` — Session model for context sharing inspiration

  **WHY Each Reference Matters**:
  - subagents/mod.rs: MUST extend without breaking — new modules added alongside existing code
  - SubagentRunStatus: Reuse in DispatchResult to maintain consistency
  - sessions/types.rs: Inspiration for SharedContext structure

  **Acceptance Criteria**:
  - [ ] All existing subagent tests still pass: `cargo test -- subagent`
  - [ ] New types compile: `cargo check`
  - [ ] AgentRole has exactly 5 variants (Planner, Executor, Reviewer, Critic, Custom)
  - [ ] No modifications to existing functions in subagents/mod.rs
  - [ ] dispatch_parallel exists as a stub that returns error

  **QA Scenarios:**

  ```
  Scenario: Existing subagent tests pass
    Tool: Bash
    Preconditions: None
    Steps:
      1. Run `cargo test -- subagent`
    Expected Result: All existing subagent tests pass (2 tests)
    Failure Indicators: Any test failure
    Evidence: .sisyphus/evidence/task-4-existing-tests.txt

  Scenario: New types compile and serialize
    Tool: Bash
    Preconditions: roles.rs, coordination.rs, dispatch.rs written
    Steps:
      1. Run `cargo check`
      2. Write unit test: create AgentRole::Planner, serialize to JSON, verify string is "planner"
      3. Run `cargo test -- role`
    Expected Result: Compilation clean, serialization correct
    Failure Indicators: Compilation error or wrong serde output
    Evidence: .sisyphus/evidence/task-4-new-types.txt

  Scenario: No breaking changes to existing subagent API
    Tool: Bash (grep)
    Preconditions: mod.rs updated
    Steps:
      1. Verify `configure_runtime`, `run_inline`, `spawn`, `get`, `list`, `cancel` still exist in mod.rs
      2. Verify their signatures are unchanged
    Expected Result: All 6 functions present with original signatures
    Failure Indicators: Missing function or changed parameter types
    Evidence: .sisyphus/evidence/task-4-api-preserved.txt
  ```

  **Evidence to Capture:**
  - [ ] task-4-existing-tests.txt
  - [ ] task-4-new-types.txt
  - [ ] task-4-api-preserved.txt

  **Commit**: YES (groups with T1, T2, T3)
  - Message: `feat(subagents): add multi-agent role, coordination, and dispatch types`
  - Files: `src/core/subagents/roles.rs`, `src/core/subagents/coordination.rs`, `src/core/subagents/dispatch.rs`, `src/core/subagents/mod.rs`
  - Pre-commit: `cargo test -- subagent`

---


### Wave 2 — Core Engine (parallel implementation)

- [ ] 5. LLM-based UniversalCritic (3 axes + rubric prompt)

  **What to do**:
  - Implement `LlmCritic` struct in `src/core/taste/critic.rs` that implements `UniversalCritic` trait:
    - Takes `Arc<dyn Provider>` for LLM calls
    - `critique()` method: constructs a structured prompt with rubric definitions for each of the 3 axes
    - Returns `CritiqueResult { axis_scores: AxisScores, raw_response: String, confidence: f64 }`
  - **Prompt engineering (CRITICAL)**:
    - Each axis MUST have explicit rubric definition in the prompt to prevent axis bleed (LLM-Rubric pattern from Metis review)
    - Coherence rubric: "Elements belong to the same worldview/style. Score 0.0=fragmented, 1.0=seamless unity"
    - Hierarchy rubric: "Primary focus is instantly identifiable. Score 0.0=everything equal weight, 1.0=clear visual/logical hierarchy"
    - Intentionality rubric: "Deliberate choices are visible vs accidental assembly. Score 0.0=generic template, 1.0=every element purposefully chosen"
    - Prompt template returns JSON: `{ "coherence": 0.0-1.0, "hierarchy": 0.0-1.0, "intentionality": 0.0-1.0, "rationale": "..." }`
  - Apply `scrub_secret_patterns()` from `src/core/providers/scrub.rs` to LLM input/output
  - Parse LLM JSON response into `CritiqueResult` with graceful error handling (fallback to zero scores if parse fails)
  - Confidence: soft `[0.0, 1.0]` from LLM, with binary fallback if LLM doesn't provide
  - Add constructor: `LlmCritic::new(provider: Arc<dyn Provider>, model: String)`

  **Must NOT do**:
  - Do NOT use neural/embedding models — LLM text completion only
  - Do NOT add all 7 axes — only Coherence, Hierarchy, Intentionality
  - Do NOT skip rubric definitions in the prompt — axis bleed is a known issue
  - Do NOT panic on LLM parse failure — return zero scores with error logged

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Prompt engineering + LLM integration + structured output parsing requires careful design
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T6, T7, T8)
  - **Parallel Group**: Wave 2
  - **Blocks**: T6, T7, T9, T12
  - **Blocked By**: T2, T3

  **References**:

  **Pattern References**:
  - `src/core/providers/scrub.rs` — Secret scrubbing pattern (`scrub_secret_patterns()` returns `Cow<str>`)
  - `src/core/providers/mod.rs` — Provider trait `chat_with_system()` method signature
  - `src/core/subagents/mod.rs:96-108` — `run_inline()` as example of calling provider.chat_with_system()
  - `docs/taste-engine-design.md:89-116` — §3.2 Universal Critic design (7 axes described, use only 3)

  **External References**:
  - LLM-Rubric pattern (Gambhir et al., 2024): Each axis needs explicit scoring rubric to prevent cross-axis contamination
  - `docs/taste-engine-design.md:113` — LLM-Rubric calibration requirement

  **WHY Each Reference Matters**:
  - providers/scrub.rs: MUST scrub secrets from taste LLM I/O (security requirement)
  - Provider trait: Need chat_with_system signature for LLM calls
  - run_inline: Shows how to construct LLM calls with system prompt + message
  - LLM-Rubric: Without explicit rubrics, LLM mixes axes (e.g., scores coherence based on hierarchy)

  **Acceptance Criteria**:
  - [ ] LlmCritic implements UniversalCritic trait
  - [ ] Prompt includes explicit rubric for each of the 3 axes
  - [ ] LLM output is JSON-parsed into axis scores
  - [ ] Secret scrubbing applied to both input and output
  - [ ] Parse failure returns graceful fallback (not panic)
  - [ ] `cargo check --features taste` passes

  **QA Scenarios:**

  ```
  Scenario: Critic prompt contains rubric definitions
    Tool: Bash (grep)
    Preconditions: critic.rs implemented
    Steps:
      1. Search src/core/taste/critic.rs for "coherence" rubric text
      2. Search for "hierarchy" rubric text
      3. Search for "intentionality" rubric text
    Expected Result: All 3 rubric definitions present in prompt template
    Failure Indicators: Missing rubric for any axis
    Evidence: .sisyphus/evidence/task-5-rubric-check.txt

  Scenario: JSON parsing handles malformed LLM output
    Tool: Bash (cargo test)
    Preconditions: Unit test written
    Steps:
      1. Write test: call parse function with malformed JSON string
      2. Assert it returns CritiqueResult with zero scores (not panic)
      3. Run `cargo test --features taste -- critic`
    Expected Result: Graceful fallback, no panic
    Failure Indicators: Panic or unwrap failure
    Evidence: .sisyphus/evidence/task-5-parse-fallback.txt

  Scenario: Secret scrubbing is applied
    Tool: Bash (grep)
    Preconditions: critic.rs implemented
    Steps:
      1. Search src/core/taste/critic.rs for `scrub_secret_patterns` usage
    Expected Result: At least one call to scrub_secret_patterns
    Failure Indicators: No scrubbing call found
    Evidence: .sisyphus/evidence/task-5-scrubbing.txt
  ```

  **Evidence to Capture:**
  - [ ] task-5-rubric-check.txt
  - [ ] task-5-parse-fallback.txt
  - [ ] task-5-scrubbing.txt

  **Commit**: YES
  - Message: `feat(taste): implement LLM-based UniversalCritic with 3-axis rubric scoring`
  - Files: `src/core/taste/critic.rs`
  - Pre-commit: `cargo check --features taste && cargo test --features taste`

---

- [ ] 6. Text Domain Adapter (TextOp suggestions)

  **What to do**:
  - Implement `TextAdapter` struct in `src/core/taste/adapter.rs` that implements `DomainAdapter`:
    - `domain()` returns `Domain::Text`
    - `suggest()` method: takes `CritiqueResult` + `TasteContext`, returns `Vec<Suggestion>`
    - For each axis with score < 0.6, generate a `Suggestion::Text` with appropriate `TextOp`:
      - Low Coherence → `TextOp::UnifyStyle` or `TextOp::RestructureArgument`
      - Low Hierarchy → `TextOp::AddOutline` or `TextOp::RestructureArgument`
      - Low Intentionality → `TextOp::AdjustDensity` or `TextOp::UnifyStyle`
    - Each suggestion includes `rationale` (derived from critique) and `priority` (based on score deficit)
    - Priority mapping: score < 0.3 → High, < 0.5 → Medium, < 0.6 → Low
  - Add constructor: `TextAdapter::new()`

  **Must NOT do**:
  - Do NOT add VideoOp or AudioOp — text domain only
  - Do NOT call LLM for suggestions — rule-based mapping from scores only
  - Do NOT generate suggestions for axes with score >= 0.6

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Rule-based logic with clear spec, moderate complexity
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T5, T7, T8)
  - **Parallel Group**: Wave 2
  - **Blocks**: T9, T12
  - **Blocked By**: T2, T5

  **References**:

  **Pattern References**:
  - `docs/taste-engine-design.md:119-136` — §3.3 Domain Adapter design + TextOp correction operators
  - `src/core/taste/adapter.rs` — DomainAdapter trait definition from T3
  - `src/core/taste/types.rs` — TextOp enum variants from T2

  **WHY Each Reference Matters**:
  - taste-engine-design.md §3.3: Source of truth for adapter design principle (感性は普遍コアで判定、改善はアダプタで実行)
  - DomainAdapter trait: Must implement this interface exactly
  - TextOp variants: Map to these specific operations

  **Acceptance Criteria**:
  - [ ] TextAdapter implements DomainAdapter
  - [ ] Returns Domain::Text from domain()
  - [ ] Generates suggestions only for axes with score < 0.6
  - [ ] Priority correctly mapped from score deficit
  - [ ] `cargo check --features taste` passes

  **QA Scenarios:**

  ```
  Scenario: Low scores generate text suggestions
    Tool: Bash (cargo test)
    Preconditions: Unit test written
    Steps:
      1. Create CritiqueResult with all scores at 0.2
      2. Call TextAdapter::suggest()
      3. Assert >= 3 suggestions returned (one per low axis)
      4. Assert all suggestions are Suggestion::Text variant
      5. Assert all priorities are High (score 0.2 < 0.3)
    Expected Result: 3+ Text suggestions, all High priority
    Failure Indicators: Wrong variant, wrong count, wrong priority
    Evidence: .sisyphus/evidence/task-6-low-scores.txt

  Scenario: High scores generate no suggestions
    Tool: Bash (cargo test)
    Preconditions: Unit test written
    Steps:
      1. Create CritiqueResult with all scores at 0.9
      2. Call TextAdapter::suggest()
      3. Assert empty suggestions vec
    Expected Result: No suggestions generated
    Failure Indicators: Unexpected suggestions for high-scoring axes
    Evidence: .sisyphus/evidence/task-6-high-scores.txt
  ```

  **Evidence to Capture:**
  - [ ] task-6-low-scores.txt
  - [ ] task-6-high-scores.txt

  **Commit**: YES (groups with T7)
  - Message: `feat(taste): add text and UI domain adapters`
  - Files: `src/core/taste/adapter.rs`
  - Pre-commit: `cargo check --features taste && cargo test --features taste`

---

- [ ] 7. UI Domain Adapter (UiOp suggestions)

  **What to do**:
  - Add `UiAdapter` struct to `src/core/taste/adapter.rs` that implements `DomainAdapter`:
    - `domain()` returns `Domain::Ui`
    - `suggest()` method: takes `CritiqueResult` + `TasteContext`, returns `Vec<Suggestion>`
    - For each axis with score < 0.6, generate a `Suggestion::Ui` with appropriate `UiOp`:
      - Low Coherence → `UiOp::RefineSpacing` or `UiOp::AddContrast`
      - Low Hierarchy → `UiOp::ImproveHierarchy` or `UiOp::AdjustLayout`
      - Low Intentionality → `UiOp::AdjustLayout` or `UiOp::AddContrast`
    - Same priority mapping as TextAdapter: score < 0.3 → High, < 0.5 → Medium, < 0.6 → Low
  - Add constructor: `UiAdapter::new()`

  **Must NOT do**:
  - Do NOT add VideoOp or AudioOp
  - Do NOT call LLM for suggestions — rule-based mapping only
  - Do NOT duplicate TextAdapter logic — consider extracting shared priority logic

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Similar pattern to T6 but with UI-specific mappings
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T5, T6, T8)
  - **Parallel Group**: Wave 2
  - **Blocks**: T9, T12
  - **Blocked By**: T2, T5

  **References**:

  **Pattern References**:
  - `docs/taste-engine-design.md:127-134` — UiOp correction operators (レイアウト再設計、コンポーネント再編、etc.)
  - `src/core/taste/adapter.rs` — TextAdapter pattern from T6 (follow same structure)
  - `src/core/taste/types.rs` — UiOp enum variants from T2

  **WHY Each Reference Matters**:
  - taste-engine-design.md: Source of truth for UI-specific suggestions
  - TextAdapter: Same pattern — share priority logic if possible

  **Acceptance Criteria**:
  - [ ] UiAdapter implements DomainAdapter
  - [ ] Returns Domain::Ui from domain()
  - [ ] Generates Suggestion::Ui with UiOp variants
  - [ ] Same priority threshold as TextAdapter
  - [ ] `cargo check --features taste` passes

  **QA Scenarios:**

  ```
  Scenario: UI adapter generates UI-specific suggestions
    Tool: Bash (cargo test)
    Preconditions: Unit test written
    Steps:
      1. Create CritiqueResult with hierarchy score 0.2, others 0.8
      2. Call UiAdapter::suggest()
      3. Assert exactly 1 suggestion returned
      4. Assert it's Suggestion::Ui variant with UiOp::ImproveHierarchy or AdjustLayout
    Expected Result: Single UI suggestion for low hierarchy
    Failure Indicators: Wrong suggestion variant or count
    Evidence: .sisyphus/evidence/task-7-ui-suggestions.txt
  ```

  **Evidence to Capture:**
  - [ ] task-7-ui-suggestions.txt

  **Commit**: YES (groups with T6)
  - Message: `feat(taste): add text and UI domain adapters`
  - Files: `src/core/taste/adapter.rs`
  - Pre-commit: `cargo check --features taste && cargo test --features taste`

---

- [ ] 8. Multi-Agent Session Coordination Layer

  **What to do**:
  - Implement coordination logic in `src/core/subagents/coordination.rs`:
    - `CoordinationManager` struct (not a trait — concrete implementation):
      - `new()` → creates manager with empty sessions HashMap
      - `create_session(roles: Vec<RoleConfig>) -> Result<CoordinationSession>`: Creates session, assigns roles, generates session_id
      - `get_session(session_id: &str) -> Option<&CoordinationSession>`
      - `add_context_message(session_id: &str, role: AgentRole, content: String) -> Result<()>`
      - `add_artifact(session_id: &str, key: String, value: serde_json::Value) -> Result<()>`
      - `get_shared_context(session_id: &str) -> Option<&SharedContext>`
      - `close_session(session_id: &str) -> Result<()>`
    - All methods handle missing sessions gracefully (bail!, not panic)
    - Session ID format: `coord_{uuid}` (follows subagent_run pattern)
  - Write unit tests: create session, add context, retrieve, close

  **Must NOT do**:
  - Do NOT modify existing subagent functions
  - Do NOT make CoordinationManager a global static (pass as parameter)
  - Do NOT persist sessions to disk — in-memory only for now

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Session management with thread-safety considerations, multiple methods
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T5, T6, T7)
  - **Parallel Group**: Wave 2
  - **Blocks**: T11, T15
  - **Blocked By**: T4

  **References**:

  **Pattern References**:
  - `src/core/subagents/mod.rs:45-54` — Global static pattern (we're NOT following this — use instance-based)
  - `src/core/subagents/mod.rs:110-167` — spawn() for session creation pattern
  - `src/core/sessions/types.rs` — Session model (id, state, metadata) for design inspiration
  - `src/core/subagents/coordination.rs` — Type definitions from T4

  **WHY Each Reference Matters**:
  - subagents/mod.rs spawn(): Pattern for ID generation and entry tracking
  - sessions/types.rs: Design inspiration for session lifecycle (Active/Archived)
  - coordination.rs types: Must use the types defined in T4

  **Acceptance Criteria**:
  - [ ] CoordinationManager creates, retrieves, and closes sessions
  - [ ] Context messages accumulate correctly
  - [ ] Artifacts can be stored and retrieved
  - [ ] Missing session returns error (not panic)
  - [ ] All existing subagent tests still pass

  **QA Scenarios:**

  ```
  Scenario: Full session lifecycle
    Tool: Bash (cargo test)
    Preconditions: Unit tests written
    Steps:
      1. Create CoordinationManager
      2. Create session with 2 roles (Planner, Executor)
      3. Add context message from Planner
      4. Add artifact
      5. Retrieve shared context, assert 1 message + 1 artifact
      6. Close session
      7. Verify get_session returns None after close
    Expected Result: Full lifecycle works
    Failure Indicators: Any assertion failure
    Evidence: .sisyphus/evidence/task-8-session-lifecycle.txt

  Scenario: Missing session returns error
    Tool: Bash (cargo test)
    Preconditions: Unit test written
    Steps:
      1. Call add_context_message with non-existent session_id
      2. Assert Result is Err
    Expected Result: Graceful error, not panic
    Failure Indicators: Panic or Ok result
    Evidence: .sisyphus/evidence/task-8-missing-session.txt
  ```

  **Evidence to Capture:**
  - [ ] task-8-session-lifecycle.txt
  - [ ] task-8-missing-session.txt

  **Commit**: YES
  - Message: `feat(subagents): implement session coordination layer`
  - Files: `src/core/subagents/coordination.rs`
  - Pre-commit: `cargo test -- subagent && cargo test -- coordination`

---

### Wave 3 — Tools + Storage (parallel)

- [ ] 9. taste.evaluate Tool implementation + registration

  **What to do**:
  - Create `src/core/tools/taste_evaluate.rs` implementing `Tool` trait:
    - `name()` returns `"taste_evaluate"`
    - `description()` provides concise system-prompt-safe description of taste evaluation purpose
    - `parameters_schema()` returns JSON Schema for:
      - `artifact`: `{ content: string, format: string }`
      - `context`: `{ domain: string, genre?: string, purpose?: string }`
    - `execute(args)` flow:
      1. Parse JSON args into typed request struct
      2. Construct `Artifact` + `TasteContext`
      3. Call `TasteEngine::evaluate()`
      4. Serialize `TasteReport` to JSON string
      5. Return `ToolResult` with JSON payload
  - Constructor takes `Arc<dyn TasteEngine>` and stores it in the tool struct.
  - Register in `src/core/tools/factory.rs`:
    - Add to `all_tools()` with `#[cfg(feature = "taste")]` and runtime guard `config.taste.enabled`
    - Add to `tool_descriptions()` under same gating
  - Register module in `src/core/tools/mod.rs`:
    - `#[cfg(feature = "taste")] pub mod taste_evaluate;`
  - Wire default taste engine composition in factory path:
    - `create_taste_engine` assembled with `LlmCritic` + adapters + store, then injected into `taste_evaluate` tool.

  **Must NOT do**:
  - Do NOT register tool when `taste` feature is disabled.
  - Do NOT bypass `config.taste.enabled` runtime toggle.
  - Do NOT return ad-hoc string formats; output must be valid JSON string payload.
  - Do NOT instantiate engine globally; inject via constructor.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Cross-module wiring + trait object injection + feature/runtime gating.
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T10, T11)
  - **Parallel Group**: Wave 3
  - **Blocks**: T12, T16
  - **Blocked By**: T3, T5, T6, T7

  **References**:

  **Pattern References**:
  - `src/core/tools/traits.rs` — Tool trait contract (`name/description/parameters_schema/execute`)
  - `src/core/tools/factory.rs` — `all_tools()` + `tool_descriptions()` registration pattern
  - `src/core/tools/mod.rs` — cfg-gated module export pattern
  - `src/core/taste/engine.rs` — `TasteEngine` trait + factory entrypoint

  **API/Type References**:
  - `Artifact`, `TasteContext`, `TasteReport` from `src/core/taste/types.rs`
  - `ToolResult` from tool trait module
  - `Arc<dyn TasteEngine>` injection boundary

  **WHY Each Reference Matters**:
  - Tool trait defines exact methods required for registry integration.
  - Factory patterns ensure consistent tool discovery + prompt description behavior.
  - Taste types prevent schema drift and keep evaluate pipeline type-safe.

  **Acceptance Criteria**:
  - [ ] `taste_evaluate` tool compiles and implements full `Tool` contract
  - [ ] Args schema validates artifact/context shape
  - [ ] `execute()` returns serialized `TasteReport` JSON through `ToolResult`
  - [ ] Tool appears in `all_tools()` + `tool_descriptions()` only when taste enabled
  - [ ] `cargo check --features taste` passes

  **QA Scenarios:**

  ```
  Scenario: Tool registration respects feature + config gate
    Tool: Bash
    Preconditions: taste_evaluate wiring completed
    Steps:
      1. Run cargo check --no-default-features (taste not compiled)
      2. Run cargo check --features taste (taste compiled)
      3. Verify unit test toggles config.taste.enabled true/false
    Expected Result: Tool only registered when feature ON and config enabled
    Failure Indicators: Tool appears when disabled or missing when enabled
    Evidence: .sisyphus/evidence/task-9-tool-gating.txt

  Scenario: taste_evaluate execute returns valid JSON report
    Tool: Bash (cargo test)
    Preconditions: Mock TasteEngine available in test
    Steps:
      1. Build args JSON with artifact + context
      2. Call tool execute()
      3. Parse returned content as JSON
      4. Assert keys for axis scores and suggestions exist
    Expected Result: Valid JSON payload with report structure
    Failure Indicators: Parse error, panic, missing required keys
    Evidence: .sisyphus/evidence/task-9-execute-json.txt
  ```

  **Evidence to Capture:**
  - [ ] task-9-tool-gating.txt
  - [ ] task-9-execute-json.txt

  **Commit**: YES (groups with T10)
  - Message: `feat(taste): add taste.evaluate tool and gated factory registration`
  - Files: `src/core/tools/taste_evaluate.rs`, `src/core/tools/factory.rs`, `src/core/tools/mod.rs`
  - Pre-commit: `cargo check --features taste && cargo test --features taste`

---

- [ ] 10. TasteStore SQLite persistence

  **What to do**:
  - Implement `TasteStore` trait in `src/core/taste/store.rs`.
  - Add `SqliteTasteStore` with constructor accepting `rusqlite::Connection`.
  - Create schema initialization (`CREATE TABLE IF NOT EXISTS`):
    - `taste_comparisons` (append-only): `id TEXT PRIMARY KEY, domain TEXT, left_id TEXT, right_id TEXT, winner TEXT, rationale TEXT, context_json TEXT, created_at_ms INTEGER`
    - `taste_ratings` (mutable cache): `item_id TEXT PRIMARY KEY, domain TEXT, rating REAL, n_comparisons INTEGER, updated_at TEXT`
  - Implement methods:
    - `save_comparison()` — INSERT into taste_comparisons
    - `get_comparisons_for_item()` — SELECT WHERE left_id = ? OR right_id = ?
    - `get_rating()` — SELECT from taste_ratings
    - `update_rating()` — INSERT OR REPLACE into taste_ratings
    - `get_all_ratings()` — SELECT all from taste_ratings
  - Use prepared statements and typed row mapping.

  **Must NOT do**:
  - Do NOT use non-SQLite backend.
  - Do NOT mutate/delete rows from `taste_comparisons` (append-only invariant).
  - Do NOT skip `domain` dimension in queries.
  - Do NOT introduce new external DB dependencies.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: SQL schema + trait persistence implementation + query correctness.
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T9, T11)
  - **Parallel Group**: Wave 3
  - **Blocks**: T13, T14
  - **Blocked By**: T1

  **References**:

  **Pattern References**:
  - `src/core/memory/sqlite/schema.rs` — SQL schema creation conventions (CREATE TABLE IF NOT EXISTS)
  - `src/core/taste/store.rs` — TasteStore trait boundary from T3

  **API/Type References**:
  - `PairComparison` and taste rating types from `src/core/taste/types.rs`
  - `rusqlite::{Connection, params}`

  **WHY Each Reference Matters**:
  - memory/sqlite schema shows project-consistent DDL and migration style.
  - Append-only + cache split is required for replayability and fast reads.

  **Acceptance Criteria**:
  - [ ] Both tables created with expected columns/types
  - [ ] `save_comparison()` writes append-only records
  - [ ] `update_rating()` upserts mutable rating cache
  - [ ] Item/domain queries return correct scoped records
  - [ ] `cargo check --features taste` passes

  **QA Scenarios:**

  ```
  Scenario: Comparison append-only persistence
    Tool: Bash (cargo test)
    Preconditions: SqliteTasteStore tests with TempDir DB
    Steps:
      1. Save 2 comparisons for same pair
      2. Query comparisons for one item
      3. Assert both entries exist
    Expected Result: No overwrite; both records persisted
    Failure Indicators: Missing rows or overwrite behavior
    Evidence: .sisyphus/evidence/task-10-append-only.txt

  Scenario: Rating cache upsert behavior
    Tool: Bash (cargo test)
    Preconditions: update_rating test written
    Steps:
      1. Insert rating for item_id
      2. Update same item_id with new rating + count
      3. Fetch rating and assert latest values returned
    Expected Result: Single updated cache row per item
    Failure Indicators: Duplicate rows or stale value
    Evidence: .sisyphus/evidence/task-10-rating-upsert.txt
  ```

  **Evidence to Capture:**
  - [ ] task-10-append-only.txt
  - [ ] task-10-rating-upsert.txt

  **Commit**: YES (groups with T9)
  - Message: `feat(taste): implement SQLite TasteStore for comparisons and ratings`
  - Files: `src/core/taste/store.rs`
  - Pre-commit: `cargo check --features taste && cargo test --features taste`

---

- [ ] 11. Multi-agent parallel dispatch + result aggregation

  **What to do**:
  - Implement `dispatch_parallel()` in `src/core/subagents/dispatch.rs`:
    - Resolve each role to `RoleConfig` from session
    - Use `RoleConfig.timeout_secs` with default 60
    - Spawn each task via existing `spawn()` from `src/core/subagents/mod.rs`
    - Wrap each await in `tokio::time::timeout`
    - Collect per-task `DispatchResult` (success | failure | timeout)
    - Return `AggregatedResult { total_elapsed_ms, all_succeeded, results }`
  - Partial-failure handling: completed results returned even if some roles timeout.
  - Preserve stable result ordering matching input task order.

  **Must NOT do**:
  - Do NOT fail-fast cancel all tasks on first error.
  - Do NOT hardcode timeout globally for all roles.
  - Do NOT change existing `spawn()` public API contract.
  - Do NOT panic on missing role config; return typed failure result.

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: async orchestration, timeout semantics, partial-failure aggregation.
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T9, T10, T12)
  - **Parallel Group**: Wave 3
  - **Blocks**: T15
  - **Blocked By**: T8

  **References**:

  **Pattern References**:
  - `src/core/subagents/mod.rs:110-167` — `spawn()` function behavior and handle lifecycle
  - `src/core/subagents/dispatch.rs` — role/dispatch data models from T4
  - `src/core/subagents/coordination.rs` — CoordinationSession + RoleConfig from T4/T8

  **API/Type References**:
  - `CoordinationSession`, `RoleConfig`, `AgentRole` from roles.rs/coordination.rs
  - `DispatchResult`, `AggregatedResult` from coordination.rs
  - `tokio::task`, `tokio::time::timeout`

  **WHY Each Reference Matters**:
  - `spawn()` reuse avoids divergent subagent execution paths.
  - Dispatch type models define expected aggregate output structure.
  - Timeout semantics are critical to prevent hung coordination workflows.

  **Acceptance Criteria**:
  - [ ] `dispatch_parallel()` supports per-role timeout
  - [ ] Returns mixed outcomes without dropping completed results
  - [ ] `all_succeeded` only true when every task succeeds
  - [ ] total_elapsed_ms populated
  - [ ] `cargo test -- subagent` passes

  **QA Scenarios:**

  ```
  Scenario: Partial failure aggregation
    Tool: Bash (cargo test)
    Preconditions: Mock roles where one succeeds and one fails
    Steps:
      1. Dispatch two tasks in one session
      2. Force one role to return error
      3. Assert AggregatedResult includes both outcomes
      4. Assert all_succeeded == false
    Expected Result: Completed success retained despite one failure
    Failure Indicators: Missing successful result or panic
    Evidence: .sisyphus/evidence/task-11-partial-failure.txt

  Scenario: Per-role timeout enforcement
    Tool: Bash (cargo test)
    Preconditions: Mock role with sleep > timeout_secs
    Steps:
      1. Configure role timeout to 1s
      2. Dispatch task that sleeps 3s
      3. Assert result status is timeout
    Expected Result: Timeout captured per role config
    Failure Indicators: Hangs, no timeout, wrong status
    Evidence: .sisyphus/evidence/task-11-role-timeout.txt
  ```

  **Evidence to Capture:**
  - [ ] task-11-partial-failure.txt
  - [ ] task-11-role-timeout.txt

  **Commit**: YES
  - Message: `feat(subagents): implement parallel dispatch with timeout and aggregation`
  - Files: `src/core/subagents/dispatch.rs`
  - Pre-commit: `cargo test -- subagent`

---

- [ ] 12. taste.evaluate integration tests

  **What to do**:
  - Create integration binary `tests/taste.rs` (initial scaffold).
  - Gate entire binary with `#[cfg(feature = "taste")]`.
  - Follow `tests/memory.rs` pattern using explicit `#[path = "..."]` child module declarations.
  - Add test sets:
    - Type serialization round-trip: `Artifact`, `TasteReport`, `PairComparison`
    - `TasteConfig` default value + compilation checks
    - `TextAdapter` suggestion generation for low/high score paths
    - `UiAdapter` suggestion generation for low/high score paths

  **Must NOT do**:
  - Do NOT rely on implicit test module path resolution.
  - Do NOT include subagent-only tests in this file.
  - Do NOT remove existing integration binaries.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: integration test wiring + feature-gated compilation coverage.
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T10, T11)
  - **Parallel Group**: Wave 3
  - **Blocks**: T16
  - **Blocked By**: T9

  **References**:

  **Pattern References**:
  - `tests/memory.rs` — integration test binary structure with `#[path]`
  - `src/core/taste/adapter.rs`, `src/core/taste/types.rs` — types and adapter logic to test

  **WHY Each Reference Matters**:
  - `#[path]` pattern is explicitly required by this repo for integration test child modules.
  - Round-trip and default tests prevent early schema/type regressions.

  **Acceptance Criteria**:
  - [ ] `tests/taste.rs` compiles under `--features taste`
  - [ ] Type round-trip tests pass
  - [ ] `TasteConfig` defaults asserted
  - [ ] Text/UI adapter tests pass for both low/high score conditions
  - [ ] `cargo test --features taste --test taste` passes

  **QA Scenarios:**

  ```
  Scenario: Integration binary compiles and runs
    Tool: Bash (cargo test)
    Preconditions: tests/taste.rs created with #[path]
    Steps:
      1. Run cargo test --features taste --test taste
    Expected Result: Tests compile and run successfully
    Failure Indicators: Module-not-found or unresolved path errors
    Evidence: .sisyphus/evidence/task-12-path-wiring.txt

  Scenario: Adapter suggestion behavior baseline
    Tool: Bash (cargo test)
    Preconditions: low/high score fixture tests in taste binary
    Steps:
      1. Run adapter-specific test cases
      2. Assert low scores produce suggestions, high scores do not
    Expected Result: Rule-based adapter thresholds hold
    Failure Indicators: Suggestion count mismatch
    Evidence: .sisyphus/evidence/task-12-adapter-baseline.txt
  ```

  **Evidence to Capture:**
  - [ ] task-12-path-wiring.txt
  - [ ] task-12-adapter-baseline.txt

  **Commit**: YES (groups with T16)
  - Message: `test(taste): add initial integration tests for types config and adapters`
  - Files: `tests/taste.rs`
  - Pre-commit: `cargo test --features taste --test taste`

---

### Wave 4 — Learning + Multi-agent tests

- [ ] 13. TasteLearner (Bradley-Terry online model)

  **What to do**:
  - Implement `TasteLearner` trait in `src/core/taste/learner.rs`.
  - Add `BradleyTerryLearner` with in-memory `HashMap<String, (f64, u32)>` (rating, n_comparisons).
  - ~50 lines of custom BT implementation, NO external crate.
  - LMSYS Arena parameters: `η = 4.0`, `λ = 0.5`, sigmoid clamp `[-35.0, 35.0]`, log-space.
  - Methods:
    - `update(&mut self, winner_id, loser_id, outcome: f64)`: Online BT update
      - P(A>B) = sigmoid(r_A - r_B), clamp input to [-35, 35]
      - r_A += η * (outcome - P(A>B)) - λ * r_A (L2 regularization)
      - r_B += η * ((1-outcome) - P(B>A)) - λ * r_B
    - `get_rating(&self, item_id) -> Option<(f64, u32)>`: Returns (rating, n_comparisons)
    - `get_rating_if_sufficient(&self, item_id, min_comparisons: u32) -> Option<f64>`: Only if n >= min (default 5)
    - `from_comparisons(comparisons: &[PairComparison]) -> Self`: Replay builder
  - MUST NOT surface ratings until n_comparisons >= 5 per item.

  **Must NOT do**:
  - Do NOT add `skillratings` or BT crate dependencies.
  - Do NOT implement batch MLE/L-BFGS-B.
  - Do NOT expose ratings below minimum comparison threshold.
  - Do NOT store learner state in static globals.

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: numerical update logic + threshold invariants + replay determinism.
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T15)
  - **Parallel Group**: Wave 4
  - **Blocks**: T14
  - **Blocked By**: T10

  **References**:

  **Pattern References**:
  - `src/core/taste/learner.rs` — TasteLearner trait from T3
  - `src/core/taste/types.rs` — `PairComparison` for replay

  **External References**:
  - Bradley-Terry online update equation (pairwise logistic model)
  - LMSYS Arena parameterization: η=4.0, λ=0.5, sigmoid clamp ±35

  **WHY Each Reference Matters**:
  - Replayability depends on stable `PairComparison` fold behavior.
  - Parameter constants are mandatory for expected convergence.
  - Threshold gating protects against low-sample noisy output.

  **Acceptance Criteria**:
  - [ ] Online BT update implemented with clamp + L2 regularization
  - [ ] Ratings and comparison counts tracked per item
  - [ ] `from_comparisons()` reproduces same state as incremental updates
  - [ ] Ratings hidden until min comparison threshold reached
  - [ ] `cargo check --features taste` passes

  **QA Scenarios:**

  ```
  Scenario: Threshold gating blocks early ratings
    Tool: Bash (cargo test)
    Preconditions: learner unit tests implemented
    Steps:
      1. Apply <5 comparisons involving item A
      2. Call get_rating_if_sufficient(A, 5) → assert None
      3. Add 5th comparison
      4. Call get_rating_if_sufficient(A, 5) → assert Some(rating)
    Expected Result: Rating only appears at/after threshold
    Failure Indicators: Rating shown too early
    Evidence: .sisyphus/evidence/task-13-threshold-gating.txt

  Scenario: Replay determinism
    Tool: Bash (cargo test)
    Preconditions: comparison fixture list
    Steps:
      1. Build learner via incremental update loop
      2. Build learner via from_comparisons()
      3. Compare ratings/counts for all items
    Expected Result: Equivalent states
    Failure Indicators: Divergent ratings/counts
    Evidence: .sisyphus/evidence/task-13-replay-determinism.txt
  ```

  **Evidence to Capture:**
  - [ ] task-13-threshold-gating.txt
  - [ ] task-13-replay-determinism.txt

  **Commit**: YES
  - Message: `feat(taste): implement Bradley-Terry online learner with threshold gating`
  - Files: `src/core/taste/learner.rs`
  - Pre-commit: `cargo check --features taste && cargo test --features taste -- learner`

---

- [ ] 14. taste.compare Tool implementation + registration

  **What to do**:
  - Create `src/core/tools/taste_compare.rs` implementing `Tool` trait:
    - `name()` returns `"taste_compare"`
    - `parameters_schema()` accepts: left_id, right_id, winner, domain, context, rationale
    - `execute()` flow:
      1. Parse args
      2. Build `PairComparison`
      3. Call `TasteEngine::compare()` which persists via TasteStore + updates TasteLearner
      4. Return JSON with comparison status + current ratings (only if sufficient comparisons)
  - Register in `src/core/tools/factory.rs` with same `#[cfg(feature = "taste")]` + `config.taste.enabled` gate.
  - Add `#[cfg(feature = "taste")] pub mod taste_compare;` to `src/core/tools/mod.rs`.

  **Must NOT do**:
  - Do NOT bypass threshold gating and expose low-sample ratings.
  - Do NOT duplicate persistence logic outside engine/store boundary.
  - Do NOT register tool without taste feature/runtime gate.
  - Do NOT accept invalid `winner` values outside Winner enum.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: tool wiring across engine/store/learner and strict schema handling.
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: LIMITED (scaffolding in parallel; engine integration waits for T13)
  - **Parallel Group**: Wave 4
  - **Blocks**: T16
  - **Blocked By**: T3, T10, T13

  **References**:

  **Pattern References**:
  - `src/core/tools/taste_evaluate.rs` — sibling tool pattern for schema + execute style from T9
  - `src/core/tools/factory.rs` — gated registration pattern
  - `src/core/taste/engine.rs` — compare path contract

  **WHY Each Reference Matters**:
  - Matching evaluate tool conventions keeps tooling/prompt ecosystem consistent.
  - Compare path must atomically persist + learn for correctness.

  **Acceptance Criteria**:
  - [ ] `taste_compare` tool implements full `Tool` contract
  - [ ] Schema validates required fields and winner constraints
  - [ ] Execute path persists comparison + updates learner
  - [ ] Response includes ratings only when comparison count >= 5
  - [ ] `cargo check --features taste` passes

  **QA Scenarios:**

  ```
  Scenario: Compare writes comparison and updates learner
    Tool: Bash (cargo test)
    Preconditions: mock/in-memory engine-store-learner wiring
    Steps:
      1. Call taste_compare with valid pair input
      2. Assert save_comparison invoked
      3. Assert learner rating state changed
    Expected Result: Persistence and learning both executed
    Failure Indicators: Only one side updates or panic
    Evidence: .sisyphus/evidence/task-14-compare-write-learn.txt

  Scenario: Ratings gated below threshold
    Tool: Bash (cargo test)
    Preconditions: fewer than 5 comparisons for each item
    Steps:
      1. Execute compare once
      2. Inspect returned JSON
      3. Assert ratings fields absent or null
    Expected Result: No surfaced rating below minimum
    Failure Indicators: Numeric ratings returned too early
    Evidence: .sisyphus/evidence/task-14-rating-gate.txt
  ```

  **Evidence to Capture:**
  - [ ] task-14-compare-write-learn.txt
  - [ ] task-14-rating-gate.txt

  **Commit**: YES (groups with T13)
  - Message: `feat(taste): add taste.compare tool with learner-backed rating updates`
  - Files: `src/core/tools/taste_compare.rs`, `src/core/tools/factory.rs`, `src/core/tools/mod.rs`
  - Pre-commit: `cargo check --features taste && cargo test --features taste`

---

- [ ] 15. Multi-agent integration tests

  **What to do**:
  - Add integration tests for subagent coordination and dispatch.
  - Tests:
    - `CoordinationManager` session lifecycle (create, add context, close)
    - `dispatch_parallel()` timeout handling
    - Partial-failure aggregation
    - Role-based timeout override behavior
  - Use mock provider/subagent execution harness. No real LLM calls.
  - Keep tests always-compiled (no `#[cfg(feature = "taste")]`).

  **Must NOT do**:
  - Do NOT add taste feature gates to subagent tests.
  - Do NOT call real external LLM/network providers.
  - Do NOT modify existing subagent APIs to fit tests.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: coordination and concurrent dispatch integration coverage.
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T13)
  - **Parallel Group**: Wave 4
  - **Blocks**: T18
  - **Blocked By**: T11, T8

  **References**:

  **Pattern References**:
  - `tests/memory.rs` — integration binary style
  - `src/core/subagents/coordination.rs` and `src/core/subagents/dispatch.rs`
  - Existing subagent unit tests for mock setup pattern

  **WHY Each Reference Matters**:
  - Ensures session + dispatch contracts remain stable and verifiable.
  - Mock-driven tests validate behavior without external provider variance.

  **Acceptance Criteria**:
  - [ ] Session lifecycle integration test passes
  - [ ] Timeout result path verified
  - [ ] Partial-failure aggregation verified
  - [ ] Role timeout override behavior verified
  - [ ] `cargo test -- subagent` passes with new coverage

  **QA Scenarios:**

  ```
  Scenario: Timeout + partial failure in one dispatch batch
    Tool: Bash (cargo test)
    Preconditions: mock roles (success, timeout, error)
    Steps:
      1. Dispatch 3 role tasks
      2. Collect AggregatedResult
      3. Assert statuses include success + timeout + failure
      4. Assert all_succeeded == false
    Expected Result: Mixed outcomes represented correctly
    Failure Indicators: Dropped result or incorrect aggregate flags
    Evidence: .sisyphus/evidence/task-15-mixed-outcomes.txt
  ```

  **Evidence to Capture:**
  - [ ] task-15-mixed-outcomes.txt

  **Commit**: YES (groups with T16)
  - Message: `test(subagents): add coordination and parallel dispatch integration tests`
  - Files: `tests/` (new or extended integration file)
  - Pre-commit: `cargo test -- subagent`

---

- [ ] 16. Taste integration test binary (tests/taste.rs) — full pipeline

  **What to do**:
  - Extend `tests/taste.rs` from T12 with full pipeline integration tests:
    - Evaluate pipeline: `Artifact::Text → LlmCritic → TextAdapter → TasteReport`
    - Compare pipeline: `PairComparison → TasteStore → TasteLearner`
    - BT convergence: repeated comparisons, verify ratings diverge
    - Rating threshold: verify ratings not surfaced with < 5 comparisons
    - TasteStore persistence: write + read back comparisons + ratings
  - Use mock `Provider` for LLM calls (deterministic).
  - Keep full binary under `#[cfg(feature = "taste")]`.

  **Must NOT do**:
  - Do NOT call real provider APIs.
  - Do NOT assert exact floating-point values for convergence (assert trend/bounds).
  - Do NOT remove T12 baseline tests.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: end-to-end taste verification across multiple components.
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: PARTIAL (scaffolding parallel; full run waits for T14)
  - **Parallel Group**: Wave 4
  - **Blocks**: T18
  - **Blocked By**: T9, T12, T14

  **References**:

  **Pattern References**:
  - `tests/memory.rs` — integration binary organization
  - `src/core/taste/{engine,critic,adapter,store,learner}.rs` — full pipeline modules
  - Mock provider patterns from provider/subagent tests

  **WHY Each Reference Matters**:
  - Validates system integration, not just unit behavior.
  - Mock provider required for deterministic CI.

  **Acceptance Criteria**:
  - [ ] Full evaluate pipeline test passes
  - [ ] Full compare pipeline test passes
  - [ ] BT divergence trend validated after repeated outcomes
  - [ ] Ratings hidden before 5 comparisons
  - [ ] Persistence read/write round-trip verified
  - [ ] `cargo test --features taste --test taste` passes

  **QA Scenarios:**

  ```
  Scenario: End-to-end evaluate pipeline with mock provider
    Tool: Bash (cargo test)
    Preconditions: mock provider returns deterministic critique JSON
    Steps:
      1. Build Artifact::Text and TasteContext
      2. Call engine.evaluate()
      3. Assert report includes 3 axis scores + suggestions
    Expected Result: Pipeline produces valid report
    Failure Indicators: Missing axes/suggestions or parse failure
    Evidence: .sisyphus/evidence/task-16-evaluate-e2e.txt

  Scenario: BT threshold + convergence in compare pipeline
    Tool: Bash (cargo test)
    Preconditions: store + learner integration test fixture
    Steps:
      1. Apply repeated comparisons where item A wins
      2. Assert A rating trend rises vs B
      3. Assert no rating surfaced before threshold
    Expected Result: Correct trend and gating behavior
    Failure Indicators: No divergence or early rating exposure
    Evidence: .sisyphus/evidence/task-16-compare-e2e.txt
  ```

  **Evidence to Capture:**
  - [ ] task-16-evaluate-e2e.txt
  - [ ] task-16-compare-e2e.txt

  **Commit**: YES (groups with T12, T15)
  - Message: `test(taste): add end-to-end evaluate and compare integration coverage`
  - Files: `tests/taste.rs`
  - Pre-commit: `cargo test --features taste --test taste`

---

### Wave 5 — Polish + Full QA

- [ ] 17. Feature gate verification + doc comments

  **What to do**:
  - Verify feature-gate matrix:
    - `cargo check --no-default-features`
    - `cargo check --features taste`
    - `cargo check --features "email,vector-search,tui,media,taste"`
    - `cargo check --lib && cargo check --bin asteroniris`
  - Add doc comments to all public types and traits in `src/core/taste/` modules.
  - Validate runtime gate: `[taste] enabled = false` disables engine/tool registration.
  - Fix any cfg/doc regressions found during checks.

  **Must NOT do**:
  - Do NOT alter default feature list to force taste on.
  - Do NOT leave undocumented public taste APIs.
  - Do NOT skip runtime-disable validation.

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: verification-heavy pass + documentation polish.
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on all implementation tasks)
  - **Parallel Group**: Wave 5
  - **Blocks**: T18
  - **Blocked By**: T1-T16

  **References**:

  **Pattern References**:
  - Existing documented public modules in `src/core/*` for rustdoc style
  - `Cargo.toml` feature declarations

  **Acceptance Criteria**:
  - [ ] All five cargo check commands pass
  - [ ] Every public taste type/trait has doc comments
  - [ ] Runtime `taste.enabled=false` disables taste engine/tool path
  - [ ] `cargo clippy --features taste -- -D warnings` passes

  **QA Scenarios:**

  ```
  Scenario: Feature matrix compile verification
    Tool: Bash
    Preconditions: all prior tasks merged
    Steps:
      1. Run all 5 cargo check commands
    Expected Result: All checks succeed
    Failure Indicators: cfg errors, missing imports, unresolved symbols
    Evidence: .sisyphus/evidence/task-17-feature-matrix.txt

  Scenario: Runtime taste disable behavior
    Tool: Bash (cargo test)
    Preconditions: config fixture with [taste] enabled=false
    Steps:
      1. Initialize tool factory with taste feature compiled but config disabled
      2. Assert taste tools are absent from registry
    Expected Result: Runtime gate disables taste behavior
    Failure Indicators: taste tools still active
    Evidence: .sisyphus/evidence/task-17-runtime-gate.txt
  ```

  **Evidence to Capture:**
  - [ ] task-17-feature-matrix.txt
  - [ ] task-17-runtime-gate.txt

  **Commit**: YES
  - Message: `docs(taste): add public API docs and verify feature gate matrix`
  - Files: `src/core/taste/*.rs`
  - Pre-commit: `cargo check --no-default-features && cargo check --features taste`

---

- [ ] 18. Full feature matrix regression

  **What to do**:
  - Run full verification suite:
    ```bash
    cargo fmt -- --check
    cargo clippy -- -D warnings
    cargo test
    cargo check --no-default-features
    cargo check --features "taste"
    cargo check --features "email,vector-search,tui,media,taste"
    cargo check --lib
    cargo check --bin asteroniris
    ```
  - Confirm all existing 2,074+ tests plus new tests pass.
  - Verify secret scrubbing applied in taste LLM I/O path (security regression).
  - Store command outputs in `.sisyphus/evidence/final-qa/`.

  **Must NOT do**:
  - Do NOT skip any command from the matrix.
  - Do NOT ignore clippy warnings (`-D warnings` is mandatory).
  - Do NOT report pass without evidence files.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: broad regression pass across compile/lint/test/security gates.
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: NO (final consolidation gate)
  - **Parallel Group**: Wave 5
  - **Blocks**: F1-F4
  - **Blocked By**: T17

  **References**:

  **Pattern References**:
  - Final verification section in this plan (F1-F4 contract)
  - `src/core/providers/scrub.rs` — scrubbing verification target
  - `.sisyphus/evidence/` evidence storage conventions

  **Acceptance Criteria**:
  - [ ] `cargo fmt -- --check` passes
  - [ ] `cargo clippy -- -D warnings` passes
  - [ ] `cargo test` passes (existing + new)
  - [ ] All feature matrix `cargo check` commands pass
  - [ ] Scrubbing regression check passes
  - [ ] Evidence bundle exists in `.sisyphus/evidence/final-qa/`

  **QA Scenarios:**

  ```
  Scenario: Full regression gate execution
    Tool: Bash
    Preconditions: T17 complete
    Steps:
      1. Run all 8 verification commands
      2. Capture stdout/stderr logs to final-qa evidence directory
    Expected Result: All commands pass
    Failure Indicators: Any non-zero exit code
    Evidence: .sisyphus/evidence/final-qa/command-matrix.txt

  Scenario: Security scrubbing regression check
    Tool: Bash (grep)
    Preconditions: taste LLM path implemented
    Steps:
      1. Verify scrub_secret_patterns usage in taste critic/tool paths
      2. Run relevant tests validating scrub behavior
    Expected Result: Secret scrubbing enforced on taste LLM I/O
    Failure Indicators: Missing scrub call or failing scrub test
    Evidence: .sisyphus/evidence/final-qa/security-scrub-regression.txt
  ```

  **Evidence to Capture:**
  - [ ] final-qa/command-matrix.txt
  - [ ] final-qa/security-scrub-regression.txt

  **Commit**: NO (verification-only gate; no net code changes expected)

---
## Final Verification Wave

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists. For each "Must NOT Have": search codebase for forbidden patterns. Check evidence files exist in .sisyphus/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo fmt -- --check` + `cargo clippy -- -D warnings` + `cargo test`. Review all new files for: dead code, empty catches, unwrap in production, commented-out code, unused imports. Verify feature gate correctness: `taste` OFF → no taste code compiled. Check AI slop: excessive comments, over-abstraction.
  Output: `Build [PASS/FAIL] | Lint [PASS/FAIL] | Tests [N pass/N fail] | Feature Gate [PASS/FAIL] | VERDICT`

- [ ] F3. **Full Regression QA** — `unspecified-high`
  Run complete verification suite:
  ```bash
  cargo fmt -- --check
  cargo clippy -- -D warnings
  cargo test
  cargo check --no-default-features
  cargo check --features "taste"
  cargo check --features "email,vector-search,tui,media,taste"
  cargo check --lib
  cargo check --bin asteroniris
  ```
  Run security regression gate. Save all output to `.sisyphus/evidence/final-qa/`.
  Output: `Standard [PASS/FAIL] | Features [PASS/FAIL] | Security [PASS/FAIL] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff. Verify 1:1. Check "Must NOT do" compliance: no image/video/audio perceivers, no neural deps, no all 7 axes, no batch MLE, no breaking existing APIs. Flag unaccounted changes.
  Output: `Tasks [N/N compliant] | Guardrails [CLEAN/N violations] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- **Wave 1**: `feat(taste): add feature gate, type definitions, and TasteEngine trait` + `feat(subagents): add multi-agent role and coordination types`
- **Wave 2**: `feat(taste): implement LLM-based UniversalCritic with 3-axis scoring` + `feat(taste): add text and UI domain adapters` + `feat(subagents): implement session coordination layer`
- **Wave 3**: `feat(taste): add taste.evaluate tool and SQLite persistence` + `feat(subagents): implement parallel dispatch and result aggregation`
- **Wave 4**: `feat(taste): add Bradley-Terry learner and taste.compare tool` + `test(taste,subagents): add integration tests`
- **Wave 5**: `feat(taste): feature gate verification and doc comments`

---

## Success Criteria

### Verification Commands
```bash
cargo fmt -- --check                           # Expected: no diff
cargo clippy -- -D warnings                    # Expected: 0 warnings
cargo test                                     # Expected: all pass (2,074+ existing + new)
cargo check --no-default-features              # Expected: success (taste OFF)
cargo check --features "taste"                 # Expected: success (taste ON)
cargo check --features "email,vector-search,tui,media,taste"  # Expected: success
cargo check --lib                              # Expected: success
cargo check --bin asteroniris                  # Expected: success
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All 2,074+ existing tests pass + new taste/multi-agent tests
- [ ] Feature matrix compilation successful
- [ ] Security regression gate passes
- [ ] taste.evaluate returns 3-axis scores + suggestions for text input
- [ ] taste.compare persists comparison + updates BT ratings
- [ ] Multi-agent parallel dispatch with timeout + partial-failure handling works
