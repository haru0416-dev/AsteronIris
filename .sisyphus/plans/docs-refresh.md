# docs/ 設計文書リフレッシュ — Wave 1–3 完了反映

## TL;DR

> **Quick Summary**: Wave 1–3 完了後のコードベースと乖離した設計文書 5 ファイルを正確に更新する。4 ファイル更新 + 1 ファイル削除。コード変更なし。
>
> **Deliverables**:
> - `docs/ARCHITECTURE.md` — モジュールツリー・trait・型・スキーマテーブルの大規模更新
> - `docs/IMPLEMENTATION_PLAN.md` — 統計数値更新 + ステータステーブル修正 + gap-analysis 参照クリーンアップ
> - `docs/pseudo-human-architecture.md` — ~20 ファイルパス修正 + 存在しない scope.rs 参照削除
> - `docs/taste-engine-design.md` — モジュールパス修正 (`src/taste/` → `src/core/taste/`)
> - `docs/gap-analysis-autonomy.md` — **削除**（役割完了、内容は IMPLEMENTATION_PLAN.md に統合済み）
>
> **Estimated Effort**: Quick — ドキュメント編集のみ
> **Parallel Execution**: YES — 3 waves
> **Critical Path**: Task 4 (gap-analysis 参照クリーンアップ) → Task 5 (gap-analysis 削除)

---

## Context

### Original Request
ユーザーから「docs にあるものすべて実装したい」→ 監査の結果「設計書自体が古い」→「修正と更新と、不要な doc は削除」

### Interview Summary
**Key Discussions**:
- 5 ファイル全量を現コードベース（2,074 tests、Wave 1–3 完了）と照合済み
- ARCHITECTURE.md が最大乖離（新モジュール群・DROPPED テーブル・新型定義が反映されていない）
- gap-analysis-autonomy.md は P0 項目が全解消済みで削除対象
- taste-engine-design.md のモジュールパスがコードベース規約と不一致

**Research Findings**:
- テスト数: 1,925 → **2,074** (+149)
- dead_code: 56 箇所 / 24 files → **9 箇所 / 6 files** (84% 削減)
- TenantPolicyContext: "~20% stub" → **完全実装済**（enforcement logic + 9 unit tests）
- SQLite スキーマ: `memories` / `memories_fts` / `retrieval_docs` は V5 migration で DROP 済み
- 新テーブル: `retrieval_units` / `retrieval_fts` が現行
- 新モジュール 10+ が ARCHITECTURE.md に未記載
- pseudo-human-architecture.md の §3 パス ~20 箇所が旧構造
- `src/memory/scope.rs` は存在しない（pseudo-human §15 が参照）
- gap-analysis-autonomy.md は IMPLEMENTATION_PLAN.md から 13 回参照されている

### Metis Review
**Identified Gaps** (addressed):
- ARCHITECTURE.md スキーマテーブルは想定より悪い（5 エラー: 3 削除済み + 2 欠落）
- IMPLEMENTATION_PLAN.md に矛盾するステータスセクション（46–77 行の旧%値 vs 103–148 行の完了記録）
- gap-analysis 削除時に 13 箇所の参照クリーンアップが必須
- pseudo-human の `src/memory/scope.rs` 参照は存在しないファイル — 削除 or 実在ファイルへ置換が必要
- ARCHITECTURE.md のモジュールツリーは Wave 4 実装でまた陳腐化するため、最終更新日のスタンプを明記

---

## Work Objectives

### Core Objective
Wave 1–3 完了後のコードベース実態と設計文書の乖離をゼロにする。

### Concrete Deliverables
- `docs/ARCHITECTURE.md` — 更新済みファイル
- `docs/IMPLEMENTATION_PLAN.md` — 更新済みファイル
- `docs/pseudo-human-architecture.md` — 更新済みファイル
- `docs/taste-engine-design.md` — 更新済みファイル
- `docs/gap-analysis-autonomy.md` — 削除済み

### Definition of Done
- [ ] 更新後の全 docs 内の `src/` パスがファイルシステム上に実在する
- [ ] ARCHITECTURE.md のスキーマテーブルに DROPPED テーブルが含まれない
- [ ] ARCHITECTURE.md のスキーマテーブルに `retrieval_units` / `retrieval_fts` が含まれる
- [ ] IMPLEMENTATION_PLAN.md にテスト数 2,074 が記載されている
- [ ] IMPLEMENTATION_PLAN.md に dead_code 9 箇所が記載されている
- [ ] gap-analysis-autonomy.md がファイルシステム上に存在しない
- [ ] docs/ 配下に "gap-analysis" への参照が残っていない
- [ ] `cargo test` が pass する（ドキュメント変更のみなので影響なしの確認）

### Must Have
- 全ファイルパス参照の正確性
- DROPPED テーブルの除去と現行テーブルの追加
- 統計数値の正確な更新
- gap-analysis 削除 + 参照クリーンアップ

### Must NOT Have (Guardrails)
- 日本語コンテンツの英語翻訳や文体変更
- ドキュメントセクションの再構成やドキュメント統合
- IMPLEMENTATION_PLAN.md のタスク記述を未来形→過去形に書き換え（タスクテーブルは「何をするか」を記述、進捗セクションが「何をしたか」を記述）
- pseudo-human-architecture.md や taste-engine-design.md への「実装完了」アノテーション追加（これらは設計仕様であり、ステータストラッカーではない）
- AGENTS.md や docs/ 外ファイルの編集
- ソースコードファイルの変更
- ARCHITECTURE.md に新モジュールの詳細説明セクション追加（既存ツリーリストへの追加のみ）
- ULW Enhanced Plan セクション (§12–15) の統計修正以外の変更
- gap-analysis の内容を「改善」してから削除する行為

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES
- **Automated tests**: None needed (docs-only changes)
- **Framework**: N/A

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Docs edits**: Use Bash (grep) — Search for expected/forbidden patterns in updated files
- **File deletion**: Use Bash (test) — Verify file absence
- **Path validation**: Use Bash (test -e) — Verify every `src/` path in docs exists on disk

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately — independent edits):
├── Task 1: ARCHITECTURE.md 大規模更新 [unspecified-high]
├── Task 2: pseudo-human-architecture.md パス修正 [quick]
└── Task 3: taste-engine-design.md パス修正 [quick]

Wave 2 (After Wave 1 or parallel — depends on nothing):
└── Task 4: IMPLEMENTATION_PLAN.md 統計更新 + gap-analysis 参照クリーンアップ [unspecified-high]

Wave 3 (After Task 4 — sequential dependency):
└── Task 5: gap-analysis-autonomy.md 削除 + 最終検証 [quick]

Wave FINAL (After ALL tasks — verification):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Path validation sweep (unspecified-high)
└── Task F3: Scope fidelity check (deep)

Critical Path: Task 4 → Task 5 → F1-F3
Parallel Speedup: Wave 1 の 3 タスクが並列
Max Concurrent: 3 (Wave 1)
```

### Dependency Matrix

| Task | Depends On | Blocks |
|------|-----------|--------|
| 1 | — | F1, F2 |
| 2 | — | F1, F2 |
| 3 | — | F1, F2 |
| 4 | — | 5 |
| 5 | 4 | F1, F2, F3 |

### Agent Dispatch Summary

- **Wave 1**: **3** — T1 → `unspecified-high`, T2 → `quick`, T3 → `quick`
- **Wave 2**: **1** — T4 → `unspecified-high`
- **Wave 3**: **1** — T5 → `quick`
- **FINAL**: **3** — F1 → `oracle`, F2 → `unspecified-high`, F3 → `deep`

---

## TODOs


- [ ] 1. ARCHITECTURE.md 大規模更新

  **What to do**:
  - §3 モジュールツリー（lines 163–480）に以下を追加:
    - `src/core/subagents/mod.rs` — Subagent オーケストレーション
    - `src/core/memory/ingestion.rs` — IngestionPipeline trait + SqliteIngestionPipeline
    - `src/core/memory/hygiene/` — メモリ衛生（mod.rs, prune.rs, filesystem.rs, state.rs）
    - `src/runtime/evolution/mod.rs` — 自己進化サイクル
    - `src/platform/daemon/state.rs` — Daemon state persistence
    - `src/platform/cron/expression.rs` — Cron expression parser
    - `src/platform/cron/types.rs` — CronJob/CronJobKind/CronJobOrigin 型
    - `src/platform/cron/tests.rs` — Cron テスト
    - `src/security/writeback_guard/policy.rs` — Write policy enforcement
    - `src/security/writeback_guard/tests.rs` — Writeback guard テスト
    - `src/runtime/diagnostics/health.rs` — Health snapshot
  - §4 トレイト一覧に `IngestionPipeline` trait を追加（`core/memory/ingestion.rs`）
  - §4.2 Memory trait に `recall_phased()` メソッドを追加
  - §4.2 MemorySource enum に `ExternalPrimary` / `ExternalSecondary` variants を追加
  - §4.5 その他のトレイト表に `IngestionPipeline` 行を追加
  - §7.1 MemoryEventType enum に `SummaryCompacted` 以降の新 variants を確認・追記
  - §7.1 新型定義を追加: `SignalTier` enum, `SourceKind` enum, `SignalEnvelope` struct, `IngestionResult` struct
  - §7.2 SQLite スキーマテーブル（lines 900–913）を修正:
    - **削除**: `memories`（DROPPED V5）, `memories_fts`（DROPPED V5）, `retrieval_docs`（DROPPED V5）
    - **追加**: `retrieval_units`（統一検索テーブル、id/entity_id/slot_key/content/signal_tier/source_kind/embedding等）
    - **追加**: `retrieval_fts`（FTS5 仮想テーブル、retrieval_units 上）
    - **追加**: `plan_executions`（プラン実行永続化）
    - 既存の `memory_events`, `belief_slots`, `deletion_ledger`, `embedding_cache` は据え置き
  - §17 ランタイムシステムの Observability セクションに追記:
    - `AutonomyLifecycleSignal` enum（10 variants: Ingested, Deduplicated, Promoted, etc.）
    - `MemoryLifecycleSignal` enum（8 variants: ConsolidationStarted, ConflictDetected, etc.）
    - `ObserverMetric` の新 variants（SignalIngestTotal, BeliefPromotionTotal, SignalTierSnapshot, etc.）
  - §11 セキュリティ セクションに writeback_guard/policy.rs の 7 つの enforcement 関数を追記
  - 最終更新日を `2026-02-23` に更新

  **Must NOT do**:
  - 新モジュールの詳細説明セクションを新設しない — 既存パターン（ツリーリスト + テーブル行）に従う
  - 既存セクションの構造・順序を変更しない
  - 英語翻訳しない

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 大規模なマークダウン編集で精度が必要。多数の具体的な行参照と正確なテーブル構文
  - **Skills**: []
  - **Skills Evaluated but Omitted**:
    - `frontend-ui-ux`: ドメイン不一致（UI ではなくドキュメント編集）

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3)
  - **Blocks**: F1, F2
  - **Blocked By**: None (can start immediately)

  **References** (CRITICAL):

  **Pattern References**:
  - `docs/ARCHITECTURE.md:163-480` — 現在のモジュールツリー（ここに新エントリを追加）
  - `docs/ARCHITECTURE.md:484-614` — 現在のトレイト一覧（§4、ここに IngestionPipeline 追加）
  - `docs/ARCHITECTURE.md:900-913` — 現在の SQLite スキーマテーブル（ここを修正）

  **API/Type References**:
  - `src/core/memory/ingestion.rs` — IngestionPipeline trait 定義、SignalEnvelope struct、IngestionResult struct
  - `src/core/memory/memory_types.rs` — SignalTier enum, SourceKind enum, MemorySource variants
  - `src/core/memory/traits.rs` — Memory trait の recall_phased() メソッド
  - `src/core/subagents/mod.rs` — SubagentRuntimeConfig, SubagentRunSnapshot, SubagentRunStatus
  - `src/runtime/evolution/mod.rs` — EvolutionReport, EvolutionRecommendation
  - `src/runtime/observability/traits.rs` — AutonomyLifecycleSignal, MemoryLifecycleSignal, ObserverMetric
  - `src/security/writeback_guard/policy.rs` — 7 つの enforce_*_write_policy() 関数
  - `src/core/memory/sqlite/schema.rs` — 実際の CREATE TABLE 文（テーブル名と構造の正確な参照元）
  - `src/platform/cron/types.rs` — CronJob, CronJobKind, CronJobOrigin
  - `src/platform/daemon/state.rs` — DaemonStatus, state_file_path(), spawn_state_writer()

  **WHY Each Reference Matters**:
  - `ingestion.rs` → IngestionPipeline trait のメソッドシグネチャを正確に記載するため
  - `memory_types.rs` → SignalTier / SourceKind の variant 名を正確にコピーするため
  - `sqlite/schema.rs` → テーブル名・カラム名・目的を正確に記載するため（推測ではなく実コード参照）
  - `observability/traits.rs` → AutonomyLifecycleSignal の 10 variants を正確に列挙するため

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: DROPPED テーブルがスキーマテーブルから削除されている
    Tool: Bash (grep)
    Preconditions: ARCHITECTURE.md が更新済み
    Steps:
      1. grep -cP '^\| `memories`\s' docs/ARCHITECTURE.md
      2. grep -cP '^\| `memories_fts`\s' docs/ARCHITECTURE.md
      3. grep -cP '^\| `retrieval_docs`\s' docs/ARCHITECTURE.md
    Expected Result: 全て 0（テーブル行が存在しない）
    Failure Indicators: 1 以上の値が返る
    Evidence: .sisyphus/evidence/task-1-dropped-tables.txt

  Scenario: 現行テーブルがスキーマテーブルに存在する
    Tool: Bash (grep)
    Preconditions: ARCHITECTURE.md が更新済み
    Steps:
      1. grep 'retrieval_units' docs/ARCHITECTURE.md
      2. grep 'retrieval_fts' docs/ARCHITECTURE.md
      3. grep 'plan_executions' docs/ARCHITECTURE.md
    Expected Result: 全てマッチあり
    Failure Indicators: いずれかでマッチなし
    Evidence: .sisyphus/evidence/task-1-current-tables.txt

  Scenario: 新モジュールがツリーに追加されている
    Tool: Bash (grep)
    Preconditions: ARCHITECTURE.md が更新済み
    Steps:
      1. grep 'subagents' docs/ARCHITECTURE.md
      2. grep 'ingestion.rs' docs/ARCHITECTURE.md
      3. grep 'evolution' docs/ARCHITECTURE.md
      4. grep 'writeback_guard/policy.rs' docs/ARCHITECTURE.md
    Expected Result: 全てマッチあり
    Failure Indicators: いずれかでマッチなし
    Evidence: .sisyphus/evidence/task-1-new-modules.txt

  Scenario: IngestionPipeline trait がトレイト一覧に存在する
    Tool: Bash (grep)
    Preconditions: ARCHITECTURE.md が更新済み
    Steps:
      1. grep 'IngestionPipeline' docs/ARCHITECTURE.md
    Expected Result: マッチあり
    Failure Indicators: マッチなし
    Evidence: .sisyphus/evidence/task-1-ingestion-trait.txt
  ```

  **Commit**: YES (group 1)
  - Message: `docs: refresh ARCHITECTURE.md for Wave 1-3 changes`
  - Files: `docs/ARCHITECTURE.md`

- [ ] 2. pseudo-human-architecture.md パス修正

  **What to do**:
  - §3 Current Anchors（lines 21–30）の全パスを修正:
    - `src/agent/loop_.rs` → `src/core/agent/loop_/mod.rs`
    - `src/persona/state_header.rs` → `src/core/persona/state_header.rs`
    - `src/persona/state_persistence.rs` → `src/core/persona/state_persistence.rs`
    - `src/security/writeback_guard.rs` → `src/security/writeback_guard/mod.rs`
    - `src/memory/traits.rs` → `src/core/memory/traits.rs`
    - `src/memory/sqlite.rs` → `src/core/memory/sqlite/mod.rs`
    - `src/cron/mod.rs` → `src/platform/cron/mod.rs`
    - `src/cron/scheduler.rs` → `src/platform/cron/scheduler.rs`
    - `src/heartbeat/engine.rs` → `src/runtime/diagnostics/heartbeat/engine.rs`
    - `src/security/policy.rs` → `src/security/policy/mod.rs`
    - `src/daemon/mod.rs` → `src/platform/daemon/mod.rs`
    - `src/channels/mod.rs` → `src/transport/channels/mod.rs`
  - §14 の `src/channels/mod.rs` → `src/transport/channels/mod.rs`
  - §15 の `src/memory/scope.rs` 参照を削除（ファイルが存在しない）。代わりに `src/core/memory/memory_types.rs` と `src/core/memory/ingestion.rs` を参照先として記載
  - §18 の `src/agent/loop_.rs` → `src/core/agent/loop_/mod.rs`
  - §19 の `src/cron/scheduler.rs` → `src/platform/cron/scheduler.rs`、`src/heartbeat/engine.rs` → `src/runtime/diagnostics/heartbeat/engine.rs`
  - §21 の `src/observability/*` → `src/runtime/observability/`
  - §23 の `src/channels/mod.rs` → `src/transport/channels/mod.rs`、`src/gateway/mod.rs` → `src/transport/gateway/mod.rs`、`src/agent/loop_.rs` → `src/core/agent/loop_/mod.rs`、`src/security/writeback_guard.rs` → `src/security/writeback_guard/mod.rs`

  **Must NOT do**:
  - Phase 説明の内容やステータスを変更しない（設計仕様のまま維持）
  - 「実装完了」マーカーを追加しない
  - セクション構成を変更しない

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: パターンマッチ的な置換作業。判断不要の機械的修正が大半
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3)
  - **Blocks**: F1, F2
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `docs/pseudo-human-architecture.md:21-30` — §3 Current Anchors セクション（主要修正箇所）

  **API/Type References**:
  - ファイルシステム上の実パス（各パスを `test -e` で検証してから書き込み）

  **WHY Each Reference Matters**:
  - 修正前に対象パスの実在を確認することで、誤ったパスへの置換を防止

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 旧パスが全て置換されている
    Tool: Bash (grep)
    Preconditions: pseudo-human-architecture.md が更新済み
    Steps:
      1. grep -c 'src/agent/loop_\.rs' docs/pseudo-human-architecture.md
      2. grep -c 'src/memory/traits\.rs' docs/pseudo-human-architecture.md (先頭に core/ がないもの)
      3. grep -c 'src/cron/mod\.rs' docs/pseudo-human-architecture.md (先頭に platform/ がないもの)
      4. grep -c 'src/memory/scope\.rs' docs/pseudo-human-architecture.md
    Expected Result: 全て 0
    Failure Indicators: 1 以上の値
    Evidence: .sisyphus/evidence/task-2-old-paths.txt

  Scenario: 新パスが存在する
    Tool: Bash (grep)
    Preconditions: pseudo-human-architecture.md が更新済み
    Steps:
      1. grep 'src/core/agent/loop_/' docs/pseudo-human-architecture.md
      2. grep 'src/core/memory/traits.rs' docs/pseudo-human-architecture.md
      3. grep 'src/platform/cron/' docs/pseudo-human-architecture.md
    Expected Result: 全てマッチあり
    Failure Indicators: いずれかでマッチなし
    Evidence: .sisyphus/evidence/task-2-new-paths.txt

  Scenario: scope.rs 参照が削除されている
    Tool: Bash (grep)
    Preconditions: pseudo-human-architecture.md が更新済み
    Steps:
      1. grep -c 'scope\.rs' docs/pseudo-human-architecture.md
    Expected Result: 0
    Failure Indicators: 1 以上
    Evidence: .sisyphus/evidence/task-2-scope-removed.txt
  ```

  **Commit**: YES (group 1)
  - Message: `docs: fix file paths in pseudo-human-architecture.md`
  - Files: `docs/pseudo-human-architecture.md`

- [ ] 3. taste-engine-design.md モジュールパス修正

  **What to do**:
  - §7 のモジュール構成（line 239）の `src/taste/` を `src/core/taste/` に変更
    - コードベース規約: core 機能は `src/core/` 配下、plugin は `src/plugins/` 配下
    - Taste Engine は Provider/Memory/Tool と深く連携する core 機能なので `src/core/taste/`
  - §7 内の全ての `src/taste/` 参照を `src/core/taste/` に置換（ツリーリスト、テキスト説明内、Factory Function セクション）

  **Must NOT do**:
  - Taste Engine の設計内容（型定義、アーキテクチャ、学習パイプライン）を変更しない
  - パス以外の内容を編集しない

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 単純な文字列置換。1 ファイル内の同一パターン置換
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2)
  - **Blocks**: F1, F2
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `docs/taste-engine-design.md:236-248` — §7 モジュール構成セクション

  **WHY Each Reference Matters**:
  - このセクション内の全 `src/taste/` を漏れなく置換するため

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: src/taste/ が src/core/taste/ に置換されている
    Tool: Bash (grep)
    Preconditions: taste-engine-design.md が更新済み
    Steps:
      1. grep -c 'src/taste/' docs/taste-engine-design.md (結果を確認)
      2. grep -c 'src/core/taste/' docs/taste-engine-design.md
    Expected Result: Step 1 で 0（古いパスなし）、Step 2 で 5+（新パスあり）
    Failure Indicators: Step 1 が 1 以上、または Step 2 が 0
    Evidence: .sisyphus/evidence/task-3-taste-paths.txt
  ```

  **Commit**: YES (group 1)
  - Message: `docs: fix taste module path to src/core/taste/`
  - Files: `docs/taste-engine-design.md`

- [ ] 4. IMPLEMENTATION_PLAN.md 統計更新 + gap-analysis 参照クリーンアップ

  **What to do**:
  - §1 現在地の統計数値を更新:
    - テスト数: `1,925（+164）` → `2,074（+313）`（Wave 1 開始前からの合計増分）
  - §1 Wave 1 未処理事項（技術的負債）テーブルを更新:
    - `56 dead_code annotations` → `9 dead_code annotations（6 files、全て Wave 4–5 向けの意図的保持）`
    - `旧テーブル残存` 行を「完了（V5 migration で DROP 済み）」に更新
    - `conversation_history LSP errors` 行を「完了）」に更新（§13.1 S1 で解消済み）
  - §1 設計文書別の実装状況テーブル（lines 46–77）を Wave 2–3 完了後の実測値に更新:
    - gap-analysis P0: Planner/DAG `~40%` → `~100%`（plan-or-execute 分岐、persistence、recovery 全実装）
    - gap-analysis P0: Auto-verification `0%` → `~90%`（bounded retry、max_attempts、retry_limit_reached）
    - gap-analysis P0: Eval Harness `~90%` → `~100%`（CLI、benchmark suites、evidence 出力）
    - gap-analysis P1: Observability SLO `~40%` → `~80%`（4 backend + SLO 違反検知 + signal snapshots）
    - gap-analysis P1: RBAC/Multi-tenant `~20%` → `~50%`（TenantPolicyContext 完全実装、Role model 未）
    - pseudo-human Phase 1: Writeback Guard `~35%` → `~100%`（enforcement logic 完了）
    - pseudo-human Phase 2: Self-Task `~50%` → `~100%`
    - pseudo-human Phase 3: Execution Path `~60%` → `~100%`（planner 経由実行接続済）
    - pseudo-human Phase 5: Controlled Variability `0%` → `~100%`（温度帯 clamp 実装済）
    - pseudo-human §14: External Signal `~15%` → `~80%`（RSS/X/Discord 実装済）
    - pseudo-human §15: Entity/Slot Taxonomy `~20%` → `~90%`（taxonomy validation 実装済）
    - pseudo-human §16: Confidence/Drift/Decay `~40%` → `~90%`（TTL cron + trend drift 実装済）
    - pseudo-human §19: Scheduler/Heartbeat `~40%` → `~90%`（quality maintenance 実装済）
    - pseudo-human §21: Observability `~25%` → `~80%`（signal-specific counters + snapshots）
    - pseudo-human §22: Extended Validation `~30%` → `~80%`
  - 根拠文書一覧（line 6）から `gap-analysis-autonomy.md` を削除し、「※ 2026-02-23 に役割完了として削除」の注記を追加
  - 文書内の `gap-analysis-autonomy.md` への全 13 参照をクリーンアップ:
    - Wave ヘッダーの「根拠文書」から削除
    - ステータステーブルの `gap-analysis` 行はそのまま維持（実装率更新のみ）
  - 最終更新日を `2026-02-23` に更新

  **Must NOT do**:
  - タスクテーブル（Wave 2–5）の記述を過去形に書き換えない
  - ULW Enhanced Plan セクション（§12–15）の統計修正以外を変更しない
  - Wave 4–5 のタスク記述を変更しない（未実装のまま）

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 統計値の正確な更新が必要。矛盾するセクションの整合が必要
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (independent of Wave 1 tasks)
  - **Parallel Group**: Wave 2 (can run parallel with Wave 1)
  - **Blocks**: Task 5
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `docs/IMPLEMENTATION_PLAN.md:26-36` — Wave 1 完了統計テーブル
  - `docs/IMPLEMENTATION_PLAN.md:46-77` — 設計文書別実装状況テーブル（ここの%値を更新）
  - `docs/IMPLEMENTATION_PLAN.md:79-87` — Wave 1 未処理事項テーブル
  - `docs/IMPLEMENTATION_PLAN.md:103-148` — 進捗更新セクション（完了記録の正確な参照元）

  **WHY Each Reference Matters**:
  - lines 46–77 の旧%値と lines 103–148 の完了記録が矛盾しているため、完了記録を根拠に更新

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: テスト数が更新されている
    Tool: Bash (grep)
    Preconditions: IMPLEMENTATION_PLAN.md が更新済み
    Steps:
      1. grep '2,074' docs/IMPLEMENTATION_PLAN.md
    Expected Result: マッチあり
    Failure Indicators: マッチなし
    Evidence: .sisyphus/evidence/task-4-test-count.txt

  Scenario: 旧 dead_code 数が削除されている
    Tool: Bash (grep)
    Preconditions: IMPLEMENTATION_PLAN.md が更新済み
    Steps:
      1. grep -c '56 dead_code\|56 箇所' docs/IMPLEMENTATION_PLAN.md
    Expected Result: 0
    Failure Indicators: 1 以上
    Evidence: .sisyphus/evidence/task-4-dead-code.txt

  Scenario: gap-analysis 参照がクリーンアップされている
    Tool: Bash (grep)
    Preconditions: IMPLEMENTATION_PLAN.md が更新済み
    Steps:
      1. grep -c 'gap-analysis-autonomy.md' docs/IMPLEMENTATION_PLAN.md
    Expected Result: 0（ファイル名への直接参照がない）
    Failure Indicators: 1 以上
    Evidence: .sisyphus/evidence/task-4-gap-refs.txt

  Scenario: TenantPolicyContext が stub ではなく完全実装と記載
    Tool: Bash (grep)
    Preconditions: IMPLEMENTATION_PLAN.md が更新済み
    Steps:
      1. grep -c 'stub のみ' docs/IMPLEMENTATION_PLAN.md
    Expected Result: 0
    Failure Indicators: 1 以上
    Evidence: .sisyphus/evidence/task-4-tenant-stub.txt
  ```

  **Commit**: YES (group 1)
  - Message: `docs: update IMPLEMENTATION_PLAN.md statistics and clean gap-analysis refs`
  - Files: `docs/IMPLEMENTATION_PLAN.md`

- [ ] 5. gap-analysis-autonomy.md 削除 + 最終検証

  **What to do**:
  - `docs/gap-analysis-autonomy.md` を削除（`rm` または `git rm`）
  - 削除後に docs/ 配下全体で `gap-analysis` への参照が残っていないことを検証

  **Must NOT do**:
  - 削除前に内容を「改善」しない
  - 他のファイルを削除しない

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: ファイル削除 + grep 検証のみ
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 3 (after Task 4)
  - **Blocks**: F1, F2, F3
  - **Blocked By**: Task 4（参照クリーンアップが先）

  **References**:

  **Pattern References**:
  - Task 4 の QA シナリオで gap-analysis 参照がクリーンなことを確認済みであること

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: ファイルが削除されている
    Tool: Bash (test)
    Preconditions: Task 4 完了済み
    Steps:
      1. test ! -f docs/gap-analysis-autonomy.md && echo 'PASS' || echo 'FAIL'
    Expected Result: PASS
    Failure Indicators: FAIL
    Evidence: .sisyphus/evidence/task-5-file-deleted.txt

  Scenario: docs/ 全体で gap-analysis 参照がない
    Tool: Bash (grep)
    Preconditions: ファイル削除済み
    Steps:
      1. grep -r 'gap-analysis' docs/ && echo 'FAIL' || echo 'PASS'
    Expected Result: PASS（マッチなし）
    Failure Indicators: FAIL（参照が残っている）
    Evidence: .sisyphus/evidence/task-5-no-refs.txt
  ```

  **Commit**: YES (group 2)
  - Message: `docs: remove obsolete gap-analysis-autonomy.md`
  - Files: `docs/gap-analysis-autonomy.md` (deletion)
---

## Final Verification Wave

> 3 review agents run in PARALLEL. ALL must APPROVE. Rejection → fix → re-run.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (grep doc, check file). For each "Must NOT Have": search docs for forbidden patterns — reject with file:line if found. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Path Validation Sweep** — `unspecified-high`
  Extract every `src/` path from all 4 remaining docs. Verify each path exists on disk with `test -e`. Report any MISSING paths as failure. Save results to `.sisyphus/evidence/final-path-validation.txt`.
  Output: `Paths [N/N valid] | VERDICT: APPROVE/REJECT`

- [ ] F3. **Scope Fidelity Check** — `deep`
  For each task: read actual diff (git diff). Verify nothing beyond spec was changed. Check "Must NOT do" compliance. Flag unaccounted changes.
  Output: `Tasks [N/N compliant] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- **1**: `docs: refresh design docs to reflect Wave 1-3 completion` — docs/ARCHITECTURE.md, docs/IMPLEMENTATION_PLAN.md, docs/pseudo-human-architecture.md, docs/taste-engine-design.md
- **2**: `docs: remove obsolete gap-analysis-autonomy.md` — docs/gap-analysis-autonomy.md (deletion)

---

## Success Criteria

### Verification Commands
```bash
# All src/ paths in docs exist on disk
grep -ohrP 'src/[a-zA-Z_/]+(?:\.rs|/)' docs/ | sort -u | while read p; do test -e "$p" || echo "MISSING: $p"; done
# Expected: no output (all paths valid)

# gap-analysis file deleted
test ! -f docs/gap-analysis-autonomy.md && echo "PASS" || echo "FAIL"
# Expected: PASS

# No stale gap-analysis references
grep -r "gap-analysis" docs/ && echo "FAIL" || echo "PASS"
# Expected: PASS

# Updated test count present
grep "2,074" docs/IMPLEMENTATION_PLAN.md && echo "PASS" || echo "FAIL"
# Expected: PASS

# Old dead_code count removed
grep -c "56 dead_code\|56 箇所" docs/IMPLEMENTATION_PLAN.md | grep "^0$" && echo "PASS" || echo "FAIL"
# Expected: PASS

# Dropped tables removed from schema
grep -cP '^\| `memories`\s' docs/ARCHITECTURE.md | grep "^0$" && echo "PASS" || echo "FAIL"
# Expected: PASS

# Current tables present in schema
grep "retrieval_units" docs/ARCHITECTURE.md && grep "retrieval_fts" docs/ARCHITECTURE.md && echo "PASS" || echo "FAIL"
# Expected: PASS

# Build still passes (sanity check — docs-only change)
cargo test --no-run 2>&1 | tail -1
# Expected: Finished
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] gap-analysis-autonomy.md deleted
- [ ] All 13 gap-analysis references cleaned from IMPLEMENTATION_PLAN.md
- [ ] All file paths in docs verified against filesystem
- [ ] 最終更新日 updated on all modified docs
