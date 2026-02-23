# Wave 4 Learnings

<!-- Append only. Format: ## [TIMESTAMP] Task: {task-id}\n{content} -->

## [2026-02-23] Task: T4 - Multi-Agent Role Types & Coordination Session Types

### Completed
- Created `src/core/subagents/roles.rs` with AgentRole enum (Planner, Executor, Reviewer, Critic, Custom)
- Created `src/core/subagents/coordination.rs` with CoordinationSession, SharedContext, ContextMessage, DispatchResult, AggregatedResult
- Created `src/core/subagents/dispatch.rs` with dispatch_parallel() stub returning bail!("not yet implemented")
- Updated `src/core/subagents/mod.rs` to export new modules and added Deserialize to SubagentRunStatus

### Key Learnings
- SubagentRunStatus is defined in subagents/mod.rs — must import via `super::SubagentRunStatus` in coordination.rs
- dispatch_parallel is a stub returning bail! — real impl in T11 (Wave 3)
- AgentRole::Custom(String) serializes correctly with #[serde(rename_all = "snake_case")]
- AgentRole::Planner serializes to "planner" (snake_case) ✓
- All existing subagent tests pass (3 tests: inline_and_background_runs_complete, list_and_cancel_work, agent_role_serializes_snake_case)
- Cargo check passes for lib (no subagent-related errors)

### Test Results
- test core::subagents::roles::tests::test_agent_role_serializes_snake_case ... ok
- test core::subagents::tests::subagent_list_and_cancel_work ... ok
- test core::subagents::tests::subagent_inline_and_background_runs_complete ... ok
- Result: 3 passed; 0 failed

### Architecture Notes
- RoleConfig includes optional overrides: system_prompt_override, model_override, temperature_override, timeout_secs
- SharedContext uses HashMap<String, serde_json::Value> for artifacts and serde_json::Map for metadata
- DispatchResult tracks run_id, role, status, output, error, elapsed_ms
- AggregatedResult aggregates multiple DispatchResult with total_elapsed_ms and all_succeeded flag

## [2026-02-23] Task: T1 - Taste Feature Gate & Config Schema

### Key Learnings

1. **TasteConfig Always Compiled (No cfg gate)**
   - TasteConfig struct is NOT behind `#[cfg(feature = "taste")]`
   - Reason: TOML deserialization needs the type available at all times
   - The taste MODULE itself IS gated: `#[cfg(feature = "taste")] pub mod taste;` in src/core/mod.rs
   - This pattern allows config files to be parsed regardless of feature state

2. **Config Field Pattern**
   - All optional config sections use `#[serde(default)]` on the field
   - Config::default() impl explicitly initializes every field
   - Follow exact pattern: `#[serde(default)] pub taste: TasteConfig,` in Config struct
   - Then add `taste: TasteConfig::default(),` in Config::default() impl

3. **Module Re-export Pattern**
   - Add `mod taste;` to src/config/schema/mod.rs
   - Add `pub use taste::TasteConfig;` to src/config/schema/mod.rs
   - Add `TasteConfig` to pub use list in src/config/mod.rs (the parent module)
   - This ensures TasteConfig is accessible as `crate::config::TasteConfig`

4. **Feature Gate Placement**
   - Feature gate goes on the MODULE, not the struct
   - `#[cfg(feature = "taste")] pub mod taste;` in src/core/mod.rs
   - Allows config to deserialize even when feature is disabled
   - Runtime code (in taste module) only compiles when feature enabled

5. **Default Values Pattern**
   - Use helper functions for complex defaults: `fn default_backend() -> String { "llm".into() }`
   - Use `#[serde(default = "function_name")]` for non-trivial defaults
   - Use `#[serde(default)]` for simple types (bool, Vec, etc.)
   - Implement Default trait with explicit field initialization

6. **Testing TOML Round-trip**
   - toml crate is available in dev-deps
   - Test serialization: `toml::to_string(&cfg).expect("serialize")`
   - Test deserialization: `toml::from_str(&serialized).expect("deserialize")`
   - Verify all fields survive round-trip

7. **Config Initializer Updates**
   - When adding new Config fields, must update ALL Config initializers:
     - src/config/schema/core/types.rs - Config::default() impl
     - src/onboard/flow.rs - two Config initializers (lines ~77, ~293)
     - src/onboard/tui/mod.rs - one Config initializer (line ~101)
   - Use sed for bulk updates to avoid JSON escaping issues in edit tool

8. **Verification Steps**
   - `cargo check` - default features (taste NOT compiled)
   - `cargo check --features taste` - taste module compiled
   - `cargo check --no-default-features` - minimal build
   - `cargo test --lib config::schema::taste` - unit tests pass
   - All checks must pass before considering task complete

### Files Modified
- Cargo.toml: Added `taste = []` to [features]
- src/config/schema/taste.rs: NEW - TasteConfig struct + tests
- src/config/schema/mod.rs: Added mod taste + pub use TasteConfig
- src/config/schema/core/types.rs: Added taste field + import
- src/config/mod.rs: Added TasteConfig to pub use list
- src/core/mod.rs: Added #[cfg(feature = "taste")] pub mod taste;
- src/core/taste/mod.rs: NEW - empty module with placeholder comment
- src/onboard/flow.rs: Added taste field to Config initializers (2 places)
- src/onboard/tui/mod.rs: Added taste field to Config initializer

### Evidence
- .sisyphus/evidence/task-1-feature-gate-no-taste.txt: cargo check (default)
- .sisyphus/evidence/task-1-feature-gate-with-taste.txt: cargo check --features taste
- .sisyphus/evidence/task-1-feature-gate-no-defaults.txt: cargo check --no-default-features
- .sisyphus/evidence/task-1-taste-unit-tests.txt: cargo test --lib config::schema::taste

All tests PASS ✓

## [2026-02-23] Task: T2 - Taste Engine Core Type Definitions

### Completed
- Created `src/core/taste/types.rs` with all 14 core types
- Updated `src/core/taste/mod.rs` to export types module
- All 8 unit tests pass (round-trip serialization, BTreeMap key, axis count, tagged enums)
- `cargo check --features taste` passes
- `cargo test --features taste` passes (8 tests in taste module)

### Key Learnings
- **Artifact uses tagged enum**: `#[serde(tag = "kind", rename_all = "snake_case")]` — same pattern as Suggestion
- **Axis needs Ord+PartialOrd+Eq+PartialEq+Hash**: Required for BTreeMap key usage in AxisScores
- **Domain has Default derive**: `#[default] General` — used in TasteContext default
- **TasteContext has Default derive**: All fields optional/defaulted for easy construction
- **PairComparison.ctx is owned TasteContext**: Not a reference — enables owned serialization
- **AxisScores is type alias**: `pub type AxisScores = BTreeMap<Axis, f64>;` — not a struct wrapper
- **Exactly 3 Axis variants**: Coherence, Hierarchy, Intentionality (test guards against 4th)
- **Only Text + Ui Artifacts**: No Image, Video, Audio, or Interaction variants
- **TextFormat is simple enum**: Plain, Markdown, Html (no serde tag needed)
- **Priority and Winner enums**: Both use `#[serde(rename_all = "snake_case")]` for consistency

### Type Hierarchy
```
Artifact (tagged enum)
  ├─ Text { content: String, format: Option<TextFormat> }
  └─ Ui { description: String, metadata: Option<serde_json::Value> }

Suggestion (tagged enum)
  ├─ General { title, rationale, priority }
  ├─ Text { op: TextOp, rationale, priority }
  └─ Ui { op: UiOp, rationale, priority }

TasteReport
  ├─ axis: AxisScores (BTreeMap<Axis, f64>)
  ├─ domain: Domain
  ├─ suggestions: Vec<Suggestion>
  └─ raw_critique: Option<String>

PairComparison
  ├─ domain: Domain
  ├─ ctx: TasteContext (owned)
  ├─ left_id, right_id: String
  ├─ winner: Winner
  ├─ rationale: Option<String>
  └─ created_at_ms: u64
```

### Test Results
```
test core::taste::types::tests::test_artifact_text_roundtrip ... ok
test core::taste::types::tests::test_axis_btreemap_key ... ok
test core::taste::types::tests::test_axis_has_exactly_3_variants ... ok
test core::taste::types::tests::test_suggestion_text_tagged_enum ... ok
test core::taste::types::tests::test_pair_comparison_roundtrip ... ok
test core::taste::types::tests::test_domain_default_is_general ... ok
Result: 8 passed; 0 failed
```

### Evidence
- .sisyphus/evidence/task-2-types-compile.txt: cargo check --features taste
- .sisyphus/evidence/task-2-serde-roundtrip.txt: cargo test --features taste

All tests PASS ✓

## [2026-02-23] Task: T3 - TasteEngine Trait & Internal Trait Declarations

### Completed
- Created `src/core/taste/engine.rs` with TasteEngine trait + DefaultTasteEngine struct + create_taste_engine factory
- Created `src/core/taste/critic.rs` with UniversalCritic pub(crate) trait + CritiqueResult struct
- Created `src/core/taste/adapter.rs` with DomainAdapter pub(crate) trait
- Created `src/core/taste/store.rs` with TasteStore pub(crate) trait + ItemRating struct
- Created `src/core/taste/learner.rs` with TasteLearner pub(crate) trait
- Updated `src/core/taste/mod.rs` to be full facade with all module re-exports
- `cargo check --features taste` passes (7 expected dead_code warnings for stub code)
- `cargo test --features taste` passes (1879 tests, all passing)

### Key Learnings
- **async_trait is available**: Already in Cargo.toml (version 0.1, default-features = false)
- **Internal traits are pub(crate)**: UniversalCritic, DomainAdapter, TasteStore, TasteLearner all pub(crate)
- **DefaultTasteEngine fields must match trait visibility**: Fields holding pub(crate) traits must be pub(crate) to avoid private_interfaces warnings
- **Stub methods use anyhow::bail!**: evaluate/compare return bail!("not yet wired (T9/T14)"), create_taste_engine returns bail!("full wiring in T9")
- **TasteEngine trait is public**: Only the engine trait and factory are pub; internal traits stay crate-private
- **CritiqueResult is public struct**: Used by DomainAdapter::suggest, so must be pub (not pub(crate))
- **ItemRating is public struct**: Used by TasteStore trait methods, so must be pub
- **TasteLearner::from_comparisons has Self: Sized bound**: Intentional — not object-safe, used for concrete implementations only
- **DefaultTasteEngine fields are pub(crate)**: critic, adapters, store, learner all pub(crate) to match trait visibility

### Trait Signatures
```rust
// TasteEngine (public)
pub trait TasteEngine: Send + Sync {
    async fn evaluate(&self, artifact: &Artifact, ctx: &TasteContext) -> anyhow::Result<TasteReport>;
    async fn compare(&self, comparison: &PairComparison) -> anyhow::Result<()>;
    fn enabled(&self) -> bool;
}

// UniversalCritic (pub(crate))
pub(crate) trait UniversalCritic: Send + Sync {
    async fn critique(&self, artifact: &Artifact, ctx: &TasteContext) -> anyhow::Result<CritiqueResult>;
}

// DomainAdapter (pub(crate))
pub(crate) trait DomainAdapter: Send + Sync {
    fn domain(&self) -> Domain;
    fn suggest(&self, critique: &CritiqueResult, ctx: &TasteContext) -> Vec<Suggestion>;
}

// TasteStore (pub(crate))
pub(crate) trait TasteStore: Send + Sync {
    async fn save_comparison(&self, comparison: &PairComparison) -> anyhow::Result<()>;
    async fn get_comparisons_for_item(&self, item_id: &str, domain: &Domain) -> anyhow::Result<Vec<PairComparison>>;
    async fn get_rating(&self, item_id: &str, domain: &Domain) -> anyhow::Result<Option<ItemRating>>;
    async fn update_rating(&self, rating: ItemRating) -> anyhow::Result<()>;
    async fn get_all_ratings(&self, domain: &Domain) -> anyhow::Result<Vec<ItemRating>>;
}

// TasteLearner (pub(crate))
pub(crate) trait TasteLearner: Send + Sync {
    fn update(&mut self, winner_id: &str, loser_id: &str, outcome: f64);
    fn get_rating(&self, item_id: &str) -> Option<(f64, u32)>;
    fn get_rating_if_sufficient(&self, item_id: &str, min_comparisons: u32) -> Option<f64>;
    fn from_comparisons(comparisons: &[PairComparison]) -> Self where Self: Sized;
}
```

### DefaultTasteEngine Structure
```rust
pub struct DefaultTasteEngine {
    pub config: TasteConfig,                                    // public config
    pub(crate) critic: Arc<dyn UniversalCritic>,               // internal
    pub(crate) adapters: HashMap<Domain, Arc<dyn DomainAdapter>>, // internal
    pub(crate) store: Option<Arc<dyn TasteStore>>,             // internal
    pub(crate) learner: Option<Arc<dyn TasteLearner>>,         // internal
}
```

### Module Facade (mod.rs)
```rust
pub mod types;
pub mod engine;
pub(crate) mod adapter;
pub(crate) mod critic;
pub(crate) mod learner;
pub(crate) mod store;

pub use types::*;
pub use engine::{create_taste_engine, TasteEngine};
```

### Test Results
- cargo check --features taste: PASS (7 expected dead_code warnings)
- cargo test --features taste: PASS (1879 tests, all passing)

### Evidence
- .sisyphus/evidence/task-3-trait-compile.txt: cargo check --features taste output

All checks PASS ✓

## [2026-02-23] Task: T8
- CoordinationManager is NOT global static — instance-based (pass as parameter in callers)
- Session ID format: coord_{uuid} using uuid::Uuid::new_v4()
- chrono is available in Cargo.toml (used by subagents/mod.rs)
- Missing session → anyhow::anyhow!("session not found: {id}") — not a panic
- sessions HashMap<String, CoordinationSession> — in-memory only


## [2026-02-23] Task: T9 - taste_evaluate Tool Implementation

### Completed
 Created `src/core/tools/taste_evaluate.rs` — full Tool trait implementation
 Registered in `src/core/tools/mod.rs` with `#[cfg(feature = "taste")]`
 Registered in `src/core/tools/factory.rs` with feature-gated `create_taste_engine` call
 7 unit tests with MockTasteEngine (text, ui, context, missing artifact, missing kind, unsupported kind, name+schema)
 `cargo clippy --features taste -- -D warnings` passes
 `cargo test --features taste` passes (1906 tests)

### Key Learnings
 **clippy manual_let_else**: Match expressions extracting from Option should use `let Some(x) = ... else { return ... };` pattern
 **Factory registration pattern**: `#[cfg(feature = "taste")] { if let Ok(engine) = create_taste_engine(&TasteConfig::default()) { tools.push(...); } }` — tries to create engine, skips silently if it fails (stub currently bails)
 **Artifact "content" → Ui "description" mapping**: Tool schema uses `content` for both kinds; for `kind=ui`, `content` maps to `Artifact::Ui { description: content, ... }`
 **TextFormat parsing via serde**: `serde_json::from_value::<TextFormat>(json!(format_str))` works because TextFormat has `#[serde(rename_all = "snake_case")]`
 **TasteContext is directly deserializable**: Has Default derive + all fields have `#[serde(default)]`, so `serde_json::from_value(context_json)` works for partial objects
 **MockTasteEngine in tests**: Must implement all 3 trait methods (evaluate, compare, enabled) even though tests only call evaluate


## T10: SqliteTasteStore
 `Mutex<Connection>` works for sync rusqlite behind async trait — no need for `spawn_blocking`
 `domain.to_string()` (strum Display) gives clean strings like "text" for SQLite storage
 Winner lacks strum Display — use `serde_json::to_value(&winner)?.as_str()` for clean string extraction
 `serde_json::from_value(Value::String(s))` cleanly deserializes Domain/Winner from stored strings
 `anyhow::Context as _` avoids name collision while keeping `.context()` method available
 For i64↔u64 casts on timestamps: `#[allow(clippy::cast_possible_wrap)]` / `cast_sign_loss` on the let binding
 For i64→u32 (n_comparisons): prefer `u32::try_from(n_comp)?` over cast + allow for clippy pedantic
 `context_json TEXT` (nullable) read as `Option<String>` with `.unwrap_or_default()` for robustness
 Test imports: `super::*` only gets items defined in parent module, not `use`-imported items — need explicit `use super::super::types::{...}`

## [2026-02-23] Task: T11 - dispatch_parallel implementation

### Key Learnings
- `dispatch_parallel` can safely run all role tasks concurrently by spawning all `tokio::spawn` handles first, then awaiting them in insertion order to preserve result ordering.
- `CoordinationSession.roles` lookup should use `find(|assignment| assignment.role == role)`; missing role config should produce a per-task `Failed` `DispatchResult` instead of failing the whole dispatch.
- Per-role timeout wiring is straightforward with `tokio::time::timeout(Duration::from_secs(timeout_secs), run_inline(task, model_override))` and defaulting `timeout_secs` to 60.
- With clippy pedantic enabled, `as_millis() as u64` requires local `#[allow(clippy::cast_possible_truncation)]` on elapsed-time conversions.
- Subagent runtime state is global (`OnceLock` + lock); tests touching `configure_runtime` need serialization to avoid cross-test interference.


## T12: tests/taste.rs integration test binary
 `TextAdapter`, `UiAdapter`, `CritiqueResult` are `pub(crate)` — cannot test from integration tests
 Adapter tests already have full coverage in `src/core/taste/adapter.rs` unit tests
 Integration test binary focuses on public types: Artifact, TasteReport, PairComparison, TasteConfig, Domain, Axis
 Feature gate: `#![cfg(feature = "taste")]` at file top gates entire binary
 All 23 integration tests pass; full suite with `--features taste` also passes (0 failures)


## T14: taste_compare tool
 Followed exact sibling pattern from `taste_evaluate.rs`
 Winner/Domain parsing: `serde_json::from_value(Value::String(...))` works because both derive `Deserialize` with `rename_all = "snake_case"`
 `clippy::cast_possible_truncation` lint fires on `as_millis() as u64` — fix with `#[allow(clippy::cast_possible_truncation)]` annotation on the let binding
 Factory registration: when sharing engine across multiple tools, use `Arc::clone(&engine)` for all but the last tool, then move `engine` into the last one
 All 6 unit tests pass: valid args, optional fields, missing winner, missing left_id, invalid winner, name_and_schema


## T15: Multi-agent coordination and parallel dispatch integration tests
 Tests added to existing `#[cfg(test)] mod tests` blocks in dispatch.rs and coordination.rs
 `TEST_RUNTIME_LOCK` mutex is essential — all dispatch tests must hold it since they share global runtime state via `OnceLock`
 `DispatchTestProvider` handles 3 task types: normal (returns `subagent:{msg}`), fail (msg contains "fail"), sleep (msg starts with "sleep:NNN")
 `dispatch_parallel` preserves insertion order in results — tasks[0] → results[0], etc.
 Timeout tests use `timeout_secs=1` with `sleep:1500` for reliable timeout detection


## T16: Pipeline Integration Tests
 `pub(crate)` types (LlmCritic, SqliteTasteStore, BradleyTerryLearner) are NOT accessible from integration tests under `tests/`
 Integration tests can only use public API: types re-exported from `asteroniris::core::taste::*` and `asteroniris::config::TasteConfig`
 BTreeMap<Axis, f64> (AxisScores) key ordering survives JSON serde roundtrip — serde_json serializes BTreeMap in key order
 `TasteContext.extra` is `serde_json::Map<String, Value>` — preserves all JSON types through roundtrip
 Existing tests in `tests/taste.rs` follow pattern: `mod <category> { use super::*; ... }` with `#[test]` functions
 The `all_axes_scores()` helper is a file-level function shared across all test modules via `use super::*`
