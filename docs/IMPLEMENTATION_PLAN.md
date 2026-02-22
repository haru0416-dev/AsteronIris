# AsteronIris 実装計画書

> **作成日**: 2026-02-22
> **改訂**: 2026-02-22（ギャップ分析反映・全面改訂）
> **対象**: Wave 1 完了後の全未実装領域
> **根拠文書**: `ARCHITECTURE.md`, `pseudo-human-architecture.md`, `taste-engine-design.md`, `gap-analysis-autonomy.md`

---

## 目次

1. [現在地](#1-現在地)
2. [前提条件（Wave 2 開始前）](#2-前提条件wave-2-開始前)
3. [全体マップ](#3-全体マップ)
4. [Wave 2: メモリ運用 + オーケストレーション基盤](#4-wave-2-メモリ運用--オーケストレーション基盤)
5. [Wave 3: 疑似人間基盤 + 評価・観測](#5-wave-3-疑似人間基盤--評価観測)
6. [Wave 4: Taste Engine + 高度自律](#6-wave-4-taste-engine--高度自律)
7. [Wave 5: プラットフォーム拡張](#7-wave-5-プラットフォーム拡張)
8. [依存関係グラフ](#8-依存関係グラフ)
9. [Rollback 戦略](#9-rollback-戦略)
10. [リスク評価](#10-リスク評価)
11. [決定ログ](#11-決定ログ)

---

## 1. 現在地

### Wave 1 完了（メモリコア再設計）

| 成果 | 数値 |
|------|------|
| テスト数 | 1,925（+164） |
| 変更ファイル | 81+ |
| 追加/削除行 | +4,000 / -1,500 |
| 検索インフラ稼働率 | 60% → 95% |

**実装済み**:
- `retrieval_units` 統一テーブル（V4 Schema）
- FTS5 + Vector hybrid 検索（RRF 融合）
- Multi-phase retrieval（R1→R2→R3→R4）
- 4 Tier Signal Model（Raw/Belief/Inferred/Governance）
- Promotion Rules（Raw → Candidate 昇格）
- IngestionPipeline trait + Discord PoC
- §23 External Content Defense（100%実装済み）

### 設計文書別の実装状況（検証済み）

> 注: 以下は 2026-02-22 のコードベース監査に基づく実測値。

| 文書 | 節 | 実装率 | 根拠 |
|------|-----|--------|------|
| pseudo-human | Phase 0: Bootstrap Hardening | **~85%** | `daemon::run()` 起動時に reconcile 実行を接続。canonical 不在時 seed + mirror 同期まで動作 |
| pseudo-human | Phase 1: Writeback Guard Expansion | **~35%** | `writeback_guard/types.rs`: `self_tasks`, `style_profile` フィールド定義済。enforcement logic 未 |
| pseudo-human | Phase 2: Self-Task Queue | **~50%** | `cron/types.rs`: `CronJobKind::Agent`, `expires_at`, `max_attempts`, `AGENT_PENDING_CAP=5` 全て実装済 |
| pseudo-human | Phase 3: Execution Path Separation | **~60%** | `scheduler.rs`: `run_agent_job_command()` で agent shell 直打ち禁止済。planner 経由接続のみ未 |
| pseudo-human | Phase 4: Memory Inference | **~80%** | `inference.rs`: `run_post_turn_inference_pass()` が `session.rs:477` で毎ターン実行中。contradiction scoring 済 |
| pseudo-human | Phase 5: Controlled Variability | 0% | 温度帯制御未 |
| pseudo-human | §13: Memory Domain Model | **90%** | Wave 1 で大部分実装。Governance events (`ContradictionMarked`, `SoftDeleted`, `TombstoneWritten`) 定義済 |
| pseudo-human | §14: External Signal Architecture | ~15% | Discord PoC のみ。実 poller 未 |
| pseudo-human | §15: Entity/Slot Taxonomy | ~20% | 規約確立済、validation registry 未 |
| pseudo-human | §16: Confidence/Drift/Decay | **~40%** | contradiction_penalty formula 実装済（`0.12 + 0.10×c + 0.08×i`）。TTL cron 未 |
| pseudo-human | §17: Promotion Rules | **100%** | Wave 1 完了 |
| pseudo-human | §18: Retrieval Strategy | **100%** | Wave 1 完了 |
| pseudo-human | §19: Scheduler/Heartbeat | **~40%** | 基盤構造 + job_kind 分岐済。quality maintenance 未 |
| pseudo-human | §20: External Signal Security | ~90% | writeback payload の `source_kind/source_ref` 直指定を guard で拒否。残は統合経路の追加固定 |
| pseudo-human | §21: Observability Additions | **~25%** | `AutonomyLifecycleSignal`(9種) + `MemoryLifecycleSignal`(8種) 定義済。signal-specific counters 未 |
| pseudo-human | §22: Extended Validation Matrix | ~30% | 部分テスト済 |
| pseudo-human | §23: External Content Defense | **100%** | 4 接続点含め完了 |
| taste-engine | Phase 1–5 | 0% | コード一切なし |
| gap-analysis | P0: Planner/DAG | **~40%** | 1,714 行の production-ready コード。40+ unit tests。call site ゼロ（未接続） |
| gap-analysis | P0: Auto-verification | 0% | |
| gap-analysis | P0: Eval Harness | ~90% | `asteroniris eval` CLI 追加済。baseline suite 実行 + JSON出力 + evidence file 出力対応 |
| gap-analysis | P1: Multi-agent | 0% | |
| gap-analysis | P1: Observability SLO | **~40%** | 4 backend 完備（log/prometheus/otel/noop）。SLO/dashboard 未 |
| gap-analysis | P1: RBAC/Multi-tenant | ~20% | `TenantPolicyContext` stub のみ |
| gap-analysis | P2: Cloudflare runtime | 0% | reserved |
| gap-analysis | P2: 運用 UI | 0% | |

### Wave 1 未処理事項（技術的負債）

| 項目 | 詳細 |
|------|------|
| **旧テーブル残存** | 完了（V5 migration で `memories` + `retrieval_docs` DROP 済み、projection は `retrieval_units` へ移送） |
| **LanceDB/Markdown 互換** | Wave 1 の RRF/multi-phase/ingestion は SQLite 専用。他 backend は `recall_phased` default impl で graceful degrade |
| **conversation_history LSP errors** | `ToolLoopRunParams.conversation_history` フィールドで tests + gateway に LSP error（cargo test は pass） |
| **56 dead_code annotations** | 24 ファイルに `#[allow(dead_code)]` 56 箇所。一部は Wave 2-3 で接続予定 |

---

## 2. 前提条件（Wave 2 開始前）

Wave 2 着手前に以下を解決する。

| # | タスク | 工数 | 理由 |
|---|--------|------|------|
| P-1 | **conversation_history LSP errors 修正** — tests + gateway handlers の `ToolLoopRunParams` 構造体フィールド不整合を解消 | 0.5d | リファクタ時に顕在化するリスク回避 |
| P-2 | **旧テーブル deprecation marker** — `memories` + `retrieval_docs` に deprecation コメント + 読み取り警告ログ追加（DROP は 2B で実施） | 0.5d | 意図しない旧テーブル参照の早期検出 |

---

## 3. 全体マップ

### 進捗更新（2026-02-23）

| 項目 | 状態 | 備考 |
|------|------|------|
| P-1 conversation_history LSP fix | 完了 | `tests/agent/tool_loop_flow.rs`, gateway handlers, loop tests の LSP error 解消 |
| S1 Planner Controller (2C-1, 2C-2) | 完了 | plan-or-execute 分岐 + invalid plan fallback 実装 |
| S1 2C-3 auto-verification loop（部分） | 完了 | scheduler agent planner route の retry を `job.max_attempts` に準拠した bounded loop に更新。結果に `retry_limit_reached` を追加し、上限到達を明示。`scheduler_routes_agent_plan_respects_retry_limit_budget` に加えて `scheduler_agent_plan_route_clamps_zero_attempt_budget_to_one` で `max_attempts=0` が 1 に正規化される回帰を固定し、`scheduler_agent_plan_route_executes_via_planner` で success path の `attempts=1` 出力境界も固定 |
| S1 2C-4 plan persistence（部分） | 完了 | `plan_executions` 永続化に加え、scheduler 初期化 (`initialize_scheduler_state`) で `status='running'` を `requeued` 化し agent job を再投入。`recover_interrupted_plan_jobs_updates_existing_agent_job_in_place` で既存 agent cron job が重複生成されず in-place 更新（`last_status=recover_pending`）され、`attempts` が 0→1 に補正される回帰を固定。さらに `recover_interrupted_plan_jobs_normalizes_existing_job_max_attempts` で既存 job の `max_attempts=0` 異常値が復帰時に `3` へ正規化される境界を固定。`recover_interrupted_plan_jobs_requeues_running_execution` で missing job 再生成時の metadata（`job_kind=agent`, `max_attempts=3`）も固定 |
| S1 2C-5 統合テスト（部分） | 完了 | `tests/project/planner_integration.rs` で multi-step DAG retry 成功、retry budget 上限停止（`max_attempts` 相当）、および zero retry budget が 1 attempt に clamp される境界を検証。加えて `tests/agent/scheduler_routes.rs` で scheduler の agent `plan:` route 出力と retry budget 反映を E2E 固定 |
| S2 Policy Gate 強化 (3A-2, 3A-5) | 完了 | persona/tool/gateway/channel/session/inference/verify-repair/ingestion 書き込みを gate 化 |
| S4 2B-1/2B-2/2B-4（一部） | 完了 | TTL expiry は `retention_expires_at` 到達時に slot を `soft_deleted` 化し、7日 grace 経過後に `hard_deleted` + retrieval unit purge へ移行する 2-phase lifecycle に更新。trend drift demotion は 30日 stale を閾値に適用し、`signal_tier=raw && reliability<0.3` の bulk demotion（`promotion_status='demoted'`）も実装。hygiene 回帰で governance tier の trend は stale demotion 対象外、30日以内 trend は demote されない境界を固定 |
| S4 2B-3 contradiction auto-demotion（部分） | 完了 | hygiene で `contradiction_penalty > 0.5` を `promotion_status='demoted'` へ自動降格。`memory_hygiene_auto_demotes_high_contradiction_units` に加え `memory_hygiene_does_not_auto_demote_at_contradiction_threshold` で `==0.5` 境界が非降格（strictly greater only）であることを固定 |
| S4 2B-5 contradiction monitoring | 完了 | heartbeat で contradiction ratio を算出して高比率時に警告。retrieval_units 未作成時に `0.0` を返す baseline 回帰に加え、SLO 境界 `ratio == 0.20` では violation を発火せず `memory_slo=ok` を維持する strict-threshold 境界を追加 |
| S4 2B-7 legacy table DROP migration | 完了 | V5 migration 追加。integrity check 後に `memories` + `retrieval_docs` を DROP |
| S4 2B-8 backend compatibility policy（部分） | 完了 | `tests/memory/backend_compatibility.rs` で SQLite/Markdown/LanceDB の `recall_phased` fallback と ingestion pipeline (`Arc<dyn Memory>`) 互換性を回帰固定。加えて各 backend で exact source_ref dedup (`dedup:source_ref_exact`) が成立する回帰、および same `source_ref` でも `source_kind` が異なる場合は dedup されず受理される partition 境界を追加 |
| S4 2B-9 observability | 完了 | heartbeat で `belief_promotion_total` / `contradiction_mark_total` / `stale_trend_purge_total` snapshot を出力し、`ObserverMetric::{BeliefPromotionTotal,ContradictionMarkTotal,StaleTrendPurgeTotal}` として observability backend へ記録。hygiene state 由来値取り込み、missing DB / missing hygiene state で `None` fallback、malformed JSON でも tick 継続（health 維持）の回帰に加え、snapshot metric 記録自体の回帰を追加。`noop_record_metric_does_not_panic` で noop backend も新 metric 受理を固定 |
| S4 2B-6 taxonomy validation | 完了 | `MemoryEventInput::normalize_for_ingress()` で entity_id/slot_key 正規化・空値拒否に加え、slot taxonomy pattern（先頭英数字 + 許容文字集合）の validation を実装。`tests/memory/backend_compatibility.rs` で SQLite/Markdown/LanceDB の `append_event` に対して正規化一貫性、空正規化識別子（entity_id/slot_key）reject、および invalid slot pattern reject 境界を回帰固定 |
| S4 2A-1 signal normalizer（部分） | 完了 | `SignalEnvelope::normalize()` で source_ref/content/entity_id/language/timestamp を正規化。`signal_envelope_normalize_rewrites_invalid_ingested_at` / `signal_envelope_normalize_rejects_invalid_language_token` / `signal_envelope_normalize_rejects_entity_id_over_limit` / `signal_envelope_normalize_rejects_source_ref_over_limit` で主要入力境界（timestamp/language/entity_id/source_ref）を固定。さらに `ingestion_pipeline_rejects_source_ref_that_sanitizes_to_empty` で sanitize 後空文字（例: `???`）の source_ref 拒否境界を固定 |
| S4 2A-2 entity classifier（部分） | 完了 | `SignalEnvelope` 正規化時に topic/entity_hint/risk_flags を rule-based 付与。`signal_envelope_classification_preserves_preseeded_topic_and_entity_hint` に加え、risk flag の sort/dedup と source_kind fallback topic（manual）を回帰固定。さらに `signal_envelope_classification_uses_community_fallback_for_discord` で community 系 source_kind fallback（discord -> `community`）を固定 |
| S4 2A-3 semantic dedup（部分） | 完了 | source_ref 完全一致 + semantic 類似（同 source_kind）を dedup drop。ingestion observer へ ingested/deduplicated 記録。`ingestion_pipeline_keeps_same_content_when_source_kind_differs` / `ingestion_pipeline_keeps_same_content_when_entity_id_differs` と dedup key 回帰で source_kind/entity_id を跨ぐ同文面は dedup されない境界を固定 |
| S4 2A-7 trend aggregation（部分） | 完了 | scheduler command `ingest:trend <entity> <topic> <query>` を追加し、`trend.snapshot.*` へ `SummaryCompacted` を自動書き込み。topic key 正規化 (`Release@@Topic/v2`→`release.topic.v2`) と空正規化 token 拒否の parser 回帰を追加 |
| S4 2A-8 ingestion observability（部分） | 完了 | ingest/dedup 時に `AutonomyLifecycleSignal` に加えて `ObserverMetric::SignalIngestTotal{source_kind}` / `SignalDedupDropTotal{source_kind}` を記録（Prometheus snapshot 対応）。`ingestion_pipeline_records_source_metrics_per_kind` と `ingestion_pipeline_records_dedup_drop_metrics_per_kind` で source_kind 別 ingest/dedup カウント（api/news）回帰を追加し、`ingestion_pipeline_records_source_metrics_for_manual_and_discord_kinds` / `ingestion_pipeline_records_dedup_drop_metrics_for_manual_kind` で manual/discord 系 source-kind 集計境界を固定 |
| S4 2A-4 RSS/API poller（部分） | 完了 | `ingest:api` / `ingest:rss` / `ingest:rss-poll <entity> <url>` を実装。RSS fetch→item parse→ingestion batch 接続。rate limit（api=10s/rss=30s）は retryable failure として返却し backoff/jitter と整合。`wiremock` で `ingest:rss-poll` の success/no-items/invalid-url 経路を回帰固定。RSS parser の item limit 適用・空 payload item skip に加え、`parse_routed_job_command_rejects_rss_poll_empty_url_after_trim` と `parse_routed_job_command_rejects_api_empty_content_after_trim` / `parse_routed_job_command_rejects_rss_empty_content_after_trim` で parser の空 URL / 空 content reject 境界を固定 |
| S4 2A-5 X/Twitter poller（部分） | 完了 | `ingest:x` + `ingest:x-poll <entity> <query>` 実装。X API v2 recent search（`X_BEARER_TOKEN` 必須）→ ingestion batch 接続。token 解決/ envelope 生成の非ネットワーク unit test を追加。`scheduler_routes_x_poll_without_token_reports_missing_bearer_token` で route=`user-x-poll` の missing token 経路を統合回帰固定。`parse_routed_job_command_trims_x_poll_query_whitespace` と `parse_routed_job_command_rejects_x_poll_empty_query_after_trim` で parser の query trim/empty reject 境界を固定。加えて `parse_routed_job_command_rejects_x_empty_content_after_trim` で `ingest:x` の empty content reject 境界を固定 |
| S4 2A-6 Discord ingestion（部分） | 完了 | channel ingress で metadata 付き envelope 化 + `ingest_batch` 接続（添付ファイルメタ含む）。`transport::channels::message_handler` の unit test で Discord 添付ファイルが attachment envelope に展開される経路、filename 欠損時 `unnamed` fallback、attachment 0 件時は base envelope のみ（`attachment_count=0`）となる経路、non-Discord で展開しない経路を回帰固定 |
| S4 2A-9 ingestion統合テスト（部分） | 完了 | `tests/agent/scheduler_routes.rs` に `ingest:api/rss/x/trend` + rate-limit + `trend(no_external_candidates)` の E2E 追加。さらに `scheduler_routes_rss_poll_invalid_url_reports_route_failure` で `ingest:rss-poll` の invalid URL 失敗経路（`route=user-rss-poll`）、`scheduler_routes_x_poll_missing_query_is_blocked_by_policy_allowlist` / `scheduler_routes_rss_poll_missing_url_is_blocked_by_policy_allowlist` / `scheduler_routes_trend_missing_query_is_blocked_by_policy_allowlist` で malformed poll/aggregation command が policy allowlist で拒否される境界を回帰固定。`src/core/memory/ingestion.rs` に envelope JSON validation と dedup key 判定 unit test を追加 |
| Wave3 3A-6 controlled variability（部分） | 完了 | `heartbeat_worker` の実行温度を `autonomy.clamp_temperature(default_temperature)` に統一し、帯域クランプ回帰テスト（上限/下限/帯域内維持）を追加。加えて `read_only` / `full` autonomy band でも clamp が適用される境界（上限・下限）を固定 |
| Wave3 3A-4 planner 経由実行（部分） | 完了 | scheduler agent route に `plan:<json>` 実行経路を追加。`run_agent_job_command()` で `PlanParser` + `PlanExecutor` + `ToolStepRunner` を利用し、shell直打ちは継続拒否。reflect self-task enqueue も `plan:<json>` へ統一。`self_task_plan_command_builds_valid_planner_payload` と `self_task_plan_command_escapes_special_characters_in_prompt_text` で payload 依存関係と特殊文字エスケープ境界を回帰固定 |
| Wave3 3A-3 mode transition metrics（部分） | 完了 | heartbeat tick 時に前回 autonomy mode を state に保持し、変更時 `AutonomyLifecycleSignal::ModeTransition` を発火。`record_autonomy_mode_transition_does_not_emit_when_unchanged` と `record_autonomy_mode_transition_emits_once_per_actual_change` で mode 変化境界（同一時は0、変化回数分のみ加算）を固定。さらに `record_autonomy_mode_transition_recovers_from_malformed_state_file` で state JSON 破損時でも panic せず、再記録後の次回 change でのみ transition metric が発火する復旧境界を固定 |
| Wave3 3A-7 統合テスト（部分） | 完了 | `tests/persona/self_task_flow.rs` を追加し、persona reflect -> self-task enqueue -> scheduler planner route 実行 (`route=agent-planner`) を E2E 検証。加えて `self_tasks` が cap 超過時に payload が拒否され queue されない回帰、cap 内 payload が bounded queue（`<=5`）として enqueue される回帰を追加。queue された全 agent job で `max_attempts == autonomy.verify_repair_max_attempts` が保持される境界も固定。既存 `tests/agent/autonomy_cross_layer_flow.rs` も planner route期待へ更新 |
| Wave3 3A-5 writeback source restriction（部分） | 完了 | writeback payload validation で top-level `source_kind/source_ref` を明示拒否（source identity の直接改変を禁止）。`persona_reflect_rejects_top_level_source_identity_injection` に加えて `persona_reflect_rejects_top_level_source_kind_only_injection` / `persona_reflect_rejects_top_level_source_ref_only_injection` を追加し、persona reflect ループで source identity 注入 payload（kindのみ/refのみ含むケース含む）が queue されないことを E2E 固定 |
| Wave3 3A-1 bootstrap hardening（部分） | 完了 | `daemon` startup で `reconcile_mirror_from_backend_on_startup()` 実行を接続し、enabled_main_session 時に canonical seed/mirror 同期を保証。`initialize_persona_startup_state_repairs_corrupt_mirror_from_backend` に加え `initialize_persona_startup_state_recreates_missing_mirror_from_backend` で mirror 欠損時の再生成復旧、`initialize_persona_startup_state_disabled_preserves_existing_mirror` で disabled path が既存 mirror を上書きしない境界を固定 |
| Wave3 3B-5 doctor 拡張（部分） | 完了 | `doctor` 出力へ memory signal stats（total/raw/demoted/contradiction_ratio）に加え promotion breakdown（candidate/promoted/demoted）と `ttl_expired_units`、`source_kind_breakdown` を追加。SQLite 実データで回帰テストを追加し、source_kind breakdown が安定ソート（例: `api=1,manual=1`）されることを固定 |
| Wave3 3B-1 eval harness CLI（部分） | 完了 | `Commands::Eval` + dispatch を追加し、deterministic seed 実行と optional evidence ファイル出力を接続。`dispatch_eval_with_evidence_writes_baseline_files` / `dispatch_eval_with_unsafe_slug_writes_sanitized_paths` / `dispatch_eval_with_blank_slug_falls_back_to_default_slug` で CLI dispatch 経路の evidence 出力と slug sanitize/default fallback を回帰固定。`write_evidence_files_sanitizes_slug_for_safe_paths` / `write_evidence_files_uses_default_slug_when_sanitized_empty` で harness 側 path traversal 断面と default slug fallback も固定。さらに `eval_harness_is_deterministic_even_when_suite_input_order_changes` と `detect_seed_change_warning_reports_unchanged_fingerprint_branch` を追加し、suite入力順変動時の determinism と seed-change warning 分岐（fingerprint unchanged）を固定 |
| Wave3 3B-2 benchmark suite（部分） | 完了 | baseline suite に `planner-success-rate` / `memory-recall-precision` / `ingestion-throughput` を追加し、coverage テストを追加。`tests/project/eval_harness.rs` に planner-memory-ingestion suite 出力存在・summary suite sort 安定性・required scenario id セット（planner/memory/ingestion）に加え、default baseline suite inventory（autonomy/injection-defense-regression/planner-memory-ingestion）固定回帰を追加 |
| Wave3 3B-3 SLO 定義（部分） | 完了 | heartbeat で contradiction ratio SLO を評価し、閾値超過時に `memory_slo` component を error として記録（しきい値内は ok）。加えて `ObserverMetric::MemorySloViolation` を発火し observability backend へ違反イベントを可視化。doctor rollout 行にも `memory_slo component` 状態を露出して確認可能化し、no-data 時は violation metric を増やさない回帰を追加 |
| Wave3 3B-4 observability dashboard（部分） | 完了 | observer metric に `SignalTierSnapshot` / `PromotionStatusSnapshot` を追加。heartbeat で `retrieval_units` の signal tier/promotions 分布を snapshot 記録し、prometheus/log/otel/noop backend で収集可能化。observer/heartbeat 回帰テストを追加し、multi-tier/multi-status 分布（raw/candidate, promoted/candidate/demoted）・fresh workspace 空 snapshot・Prometheus label snapshot の overwrite semantics（最新値反映）を固定 |

```
前提条件 ██████████████████ (1d)   LSP修正 + 旧テーブル deprecation
Wave 1   ████████████████████ 100%  メモリコア再設計                ← 完了
Wave 2   ████████████████████████   100%  メモリ運用 + オーケストレーション基盤
Wave 3   █████████████████████████████████████████████████████   100%  疑似人間基盤 + 評価・観測
Wave 4   ░░░░░░░░░░░░░░░░░░░░   0%  Taste Engine + 高度自律
Wave 5   ░░░░░░░░░░░░░░░░░░░░   0%  プラットフォーム拡張
```

**原則**: 各 Wave は独立して merge 可能。Wave 内のタスクは依存順に配置。

---

## 4. Wave 2: メモリ運用 + オーケストレーション基盤

> **目的**: Wave 1 で構築したメモリインフラを実運用可能にし、エージェントの行動制御基盤を確立する。
>
> **根拠文書**: pseudo-human §14–§16, §19–§22, gap-analysis P0

### 2A: Ingestion Pipeline 本格化

| # | タスク | 根拠 | 工数 |
|---|--------|------|------|
| 2A-1 | **Signal Normalizer** — encoding/lang/timestamp/source_ref の正規化モジュール | §14.2 step 2 | 0.5d |
| 2A-2 | **Entity Classifier** — topic/entity/risk のルールベース分類。`risk_flags` (`rumor\|unverified\|sensitive\|policy_risky`) を `SignalEnvelope.metadata` に設定 | §14.2 step 3, §13.2 | 1d |
| 2A-3 | **Semantic Dedup** — source_ref 完全一致 + embedding cosine 類似度（閾値 0.95）による重複排除 | §14.2 step 4 | 1d |
| 2A-4 | **RSS/API Poller** — scheduler ジョブとして周期実行、`IngestionPipeline` 経由で書き込み。source 別 rate limiting + exponential backoff + jitter | §14.1, §19.1 | 2.5d |
| 2A-5 | **X/Twitter Poller** — Twitter API v2 での pull → normalize → ingest。scheduler ジョブとして RSS と同様の構造 | §14.1 | 1.5d |
| 2A-6 | **Discord Ingestion 本格化** — PoC → channel metadata envelope 化、バッチ処理 | §14.1 | 1d |
| 2A-7 | **Trend Signal Aggregation** — 複数信号ソースから定期的に summary event を生成し `trend.snapshot.*` slot に書き込み | §14.1, §18 R2 | 1.5d |
| 2A-8 | **Ingestion Observability** — `signal_ingest_total{source_kind}`, `signal_dedup_drop_total{source_kind}` を `AutonomyLifecycleSignal` 経由で記録 | §21 | 0.5d |
| 2A-9 | **統合テスト** — ingest → normalize → classify → dedup → recall 反映。envelope JSON validation + dedup key 判定の unit tests | §22 | 1d |

**受け入れ基準**:
- RSS URL を config に追加 → 自動取得 → `retrieval_units` に `SignalTier::Raw` で保存される
- X/Twitter の poll → ingest フローが scheduler で動作する
- 同一 `source_ref` の二重取り込みが発生しない
- `risk_flags` 付き信号に適切なメタデータが設定される
- source 別 rate limit 違反時に backoff が発動する
- `signal_ingest_total` / `signal_dedup_drop_total` が Prometheus で取得可能
- 既存テスト全 pass + 新規テスト 15+

### 2B: メモリインテリジェンス + 移行完了

| # | タスク | 根拠 | 工数 |
|---|--------|------|------|
| 2B-1 | **TTL Expiry Cron** — heartbeat で `retention_expires_at` 到達エントリを soft-delete（7 日復元期間後に物理削除） | §16.2, §19.2 | 1d |
| 2B-2 | **Trend Drift Control** — trend slot の time-decay 自動適用。`trend.*` slot は 30d TTL、`stale_trend_purge_total` 計測 | §16.2, §21 | 0.5d |
| 2B-3 | **Contradiction Auto-Demotion** — 矛盾ペナルティ > 0.5 で `promotion_status` を `demoted` に自動降格 | Phase 4, §16.2 | 1d |
| 2B-4 | **Low-Confidence Bulk Demotion** — heartbeat で `reliability < 0.3` かつ `signal_tier = raw` のエントリを bulk demote | §19.2 | 0.5d |
| 2B-5 | **Heartbeat Contradiction Monitoring** — contradiction ratio（矛盾/全体比）を計測、閾値超過で警告ログ | §19.2 | 0.5d |
| 2B-6 | **Entity/Slot Taxonomy Registry** — slot_key パターンの validation（正規表現ベース） + 不正パターン拒否。entity_id 形式の正規化 | §15 | 1d |
| 2B-7 | **旧テーブル DROP Migration** — V5 migration: `memories` + `retrieval_docs` を DROP。migration 前に data integrity check | Wave 1 負債 | 1d |
| 2B-8 | **LanceDB/Markdown 互換性方針** — 他 backend の Wave 1 互換性を明文化。`recall_phased` は default impl（= `recall_scoped` fallback）で許容。IngestionPipeline は `Arc<dyn Memory>` 経由で全 backend 対応済 | Wave 1 負債 | 0.5d |
| 2B-9 | **Memory Promotion Observability** — `belief_promotion_total`, `contradiction_mark_total`, `stale_trend_purge_total` を記録 | §21 | 0.5d |

**受け入れ基準**:
- TTL 到達エントリが heartbeat サイクルで soft-delete される
- soft-delete から 7 日後に物理削除が実行される
- trend slot が設定 TTL 後に recall 優先度から脱落する
- 矛盾ペナルティ > 0.5 で自動降格が発動する
- `reliability < 0.3` の raw エントリが bulk demote される
- contradiction ratio 監視が heartbeat ログに出力される
- 不正な slot_key パターンが `append_event` で拒否される
- V5 migration 後、旧テーブルが存在しない
- 既存テスト全 pass + 新規テスト 10+

### 2C: Planner/DAG 統合

> **前提**: 既存 Planner コード（1,714 行、40+ tests）は production-ready。`ToolRegistry` / `ExecutionContext` と互換。未接続なだけ。

| # | タスク | 根拠 | 工数 |
|---|--------|------|------|
| 2C-1 | **Agent Loop 接続** — `ToolLoop` に plan-or-execute 分岐を追加。LLM が plan JSON を生成 → `PlanParser::parse()` → `PlanExecutor::execute()` のパス | gap P0-1 | 2d |
| 2C-2 | **Plan Generation Prompt** — LLM に plan 生成を促す system prompt テンプレート + complexity threshold（いつ plan を使うかの判定ロジック） | gap P0-1 | 1d |
| 2C-3 | **Auto-Verification Loop** — 実行結果の品質ゲート + 失敗解析 + 再試行。`PlanExecutor::execute()` の `ExecutionReport` を検査し、failed steps を re-plan | gap P0-2 | 2d |
| 2C-4 | **Plan Persistence** — 実行中の plan を SQLite に永続化（`plan_executions` テーブル）、daemon restart 後の中断復帰 | gap P0-1 | 1d |
| 2C-5 | **統合テスト** — 多段タスク plan → execute → verify → retry フロー。`tests/project/planner_integration.rs` | gap P0-1,2 | 1d |

**受け入れ基準**:
- 3 ステップ以上のタスクで DAG が自動生成される
- complexity threshold 以下のタスクは従来の直接 tool loop で実行される
- 中間ステップ失敗時に自動再試行が発動する（max_attempts 制限付き）
- 再試行上限到達でユーザーへの報告が行われる
- plan 中断 → daemon 再起動 → `plan_executions` テーブルから自動復帰が動作する

### Wave 2 合計見積り: ~22d

---

## 5. Wave 3: 疑似人間基盤 + 評価・観測

> **目的**: エージェントの自律性を段階的に引き上げ、品質を定量評価可能にする。
>
> **根拠文書**: pseudo-human Phase 0–3/5, gap-analysis P0-3/P1-5/P1-6

### 3A: Pseudo-Human Foundation

> **前提**: Scheduler の `CronJobKind::Agent`, `expires_at`, `max_attempts`, `AGENT_PENDING_CAP=5` は実装済み。Execution path separation（agent shell 直打ち禁止）も実装済み。Writeback payload の `self_tasks`, `style_profile` フィールドは定義済み。

| # | タスク | 根拠 | 工数 |
|---|--------|------|------|
| 3A-1 | **Bootstrap Hardening** — startup flow で `reconcile_mirror_from_backend_on_startup()` を確実に呼び出す。canonical 不在時に最小 `StateHeaderV1` を seed する初期化パス追加 | Phase 0 | 0.5d |
| 3A-2 | **Writeback Guard Enforcement** — `validate_writeback_payload()` に immutable field 改変拒否 + self_tasks 件数/長さ/期限制限 + style_profile 範囲制限の enforcement logic 追加 | Phase 1 | 1d |
| 3A-3 | **Persona Reflection → Self-Task** — turn 後の反省処理から self-task を生成し、`CronJobKind::Agent` で scheduler に enqueue する接続ロジック | Phase 2 | 1d |
| 3A-4 | **Planner 経由実行接続** — `run_agent_job_command()` の "no direct shell" パスを Wave 2C の planner 経由実行に接続 | Phase 3 | 0.5d |
| 3A-5 | **Persona Writeback Source Restriction** — writeback guard で外界ソース定義（source_kind/source_ref 設定）の直接変更を拒否 | §20 | 0.5d |
| 3A-6 | **Controlled Variability Band** — config 温度帯定義（min/max, mode 別）、全 temperature 適用を境界値に clamp | Phase 5 | 1d |
| 3A-7 | **統合テスト** — persona 反省 → self-task enqueue → bounded execution → policy 適用の end-to-end。`tests/persona.rs` integration binary 新設 | Phase 各完了条件 | 1d |

**受け入れ基準**:
- 起動直後に canonical/mirror reconcile が実行される
- canonical 不在時に最小 StateHeaderV1 が自動生成される
- immutable field 改変は 100% 拒否される
- self_tasks が pending 上限 5 を超えた場合に拒否される
- persona 反省から生成された self-task が scheduler に enqueue される
- persona 起点の shell 直打ちが 0 件（planner 経由のみ）
- persona writeback で source_kind/source_ref の直接変更が拒否される
- 温度が設定帯域外に出ない
- 既存テスト全 pass + 新規テスト 10+

### 3B: 評価基盤 + 観測強化

| # | タスク | 根拠 | 工数 |
|---|--------|------|------|
| 3B-1 | **Eval Harness CLI 公開** — `asteroniris eval` コマンドを `Commands` enum に追加、dispatch 接続 | gap P0-3 | 0.5d |
| 3B-2 | **Eval Benchmark Suite** — baseline suites に加え、planner 成功率/memory recall precision/ingestion throughput のベンチマーク定義。JSON レポート出力 | gap P0-3 | 1.5d |
| 3B-3 | **Observability SLO 定義** — 主要メトリクスに SLO/SLI を設定。SLO 違反時にログ/アラート発火 | gap P1-5 | 1d |
| 3B-4 | **Observability Dashboard** — Prometheus/OTel メトリクスの export 統合。既存 4 backend（log/prometheus/otel/noop）のメトリクス記録を signal-specific counters まで拡張 | gap P1-5 | 1.5d |
| 3B-5 | **Doctor 拡張** — signal metrics / TTL status / promotion stats / contradiction ratio を `asteroniris doctor` 出力に追加 | §21 | 0.5d |

**受け入れ基準**:
- `asteroniris eval` でベンチマーク実行 → JSON レポート出力
- planner/memory/ingestion の各ベンチマークが定義されている
- SLO 違反時にログ/アラートが発火する
- `asteroniris doctor` で memory health / signal stats が表示される

### Wave 3 合計見積り: ~10.5d

---

## 6. Wave 4: Taste Engine + 高度自律

> **目的**: 審美評価エンジンの初期版を構築し、マルチエージェント協調の基盤を整える。
>
> **根拠文書**: `taste-engine-design.md`, gap-analysis P1-4

### 4A: Taste Engine Phase 1–2（Text Critic + UI Adapter）

| # | タスク | 根拠 | 工数 |
|---|--------|------|------|
| 4A-1 | **Feature Gates + Config** — `Cargo.toml` に `taste` feature 追加、`#[cfg(feature = "taste")]` guards、`[taste]` config セクション | §7 Config/Feature Gates | 0.5d |
| 4A-2 | **型定義** — `Artifact`, `TasteContext`, `TasteReport`, `AxisScores`, `Suggestion`, `PairComparison` | §7 主要データ型 | 0.5d |
| 4A-3 | **TasteEngine trait + factory** — `create_taste_engine()`, deny-by-default security policy 適用 | §7 | 0.5d |
| 4A-4 | **LLM-based Universal Critic** — 3 軸（Coherence/Hierarchy/Intentionality）スコアリング。LLM I/O に secret scrubbing 適用 | §3.2, Phase 1 | 2d |
| 4A-5 | **Text Domain Adapter** — `TextOp` 改善提案生成 | §3.3, Phase 1 | 1d |
| 4A-6 | **UI Domain Adapter** — ルールベース `UiOp` 提案 | §3.3, Phase 2 | 1.5d |
| 4A-7 | **Tool 統合** — `taste.evaluate` を ToolRegistry に登録。deny-by-default allowlist に追加 | §7 Tool 統合 | 1d |
| 4A-8 | **テスト** — unit tests + `tests/taste.rs` integration binary 新設 | ARCHITECTURE §19 | 1d |

**受け入れ基準**:
- `taste.evaluate` で Text artifact を渡すと 3 軸スコア + 改善提案が返る
- UI スクリーンショット（Text 記述経由）で UI 改善提案が出力される
- `[taste] enabled = false` でモジュール全体が無効化される
- `#[cfg(feature = "taste")]` で条件コンパイルが正しく動作する
- LLM I/O に secret scrubbing が適用される
- 既存テスト全 pass + 新規テスト 10+

### 4B: Taste Engine Phase 3（Pair Comparison Learning）

| # | タスク | 根拠 | 工数 |
|---|--------|------|------|
| 4B-1 | **TasteStore** — SQLite に比較データ永続化 | §8 Phase 3 | 1d |
| 4B-2 | **TasteLearner** — Bradley-Terry / TrueSkill プロファイル | §5, §8 Phase 3 | 2d |
| 4B-3 | **taste.compare Tool** — ペア比較記録 + learner 更新 | §8 Phase 3 | 1d |

### 4C: マルチエージェント協調基盤

| # | タスク | 根拠 | 工数 |
|---|--------|------|------|
| 4C-1 | **Role 定義** — planner/executor/reviewer/critic のロール抽象化 | gap P1-4 | 1d |
| 4C-2 | **Session 協調** — 複数エージェント間のセッション共有/引き継ぎ | gap P1-4 | 2d |
| 4C-3 | **並列実行制御** — 独立サブタスクの並列ディスパッチ + 結果集約 | gap P1-4 | 2d |

### Wave 4 合計見積り: ~17d

---

## 7. Wave 5: プラットフォーム拡張

> **目的**: 運用基盤の完成度を引き上げ、デプロイ選択肢を拡張する。
>
> **根拠文書**: gap-analysis P1-6/P2-7/P2-8

### 5A: セキュリティ高度化

| # | タスク | 根拠 | 工数 |
|---|--------|------|------|
| 5A-1 | **RBAC モデル** — ロール/権限/テナント分離の本格実装 | gap P1-6 | 3d |
| 5A-2 | **マルチテナント** — per-tenant memory isolation + config scoping | gap P1-6 | 2d |

### 5B: ランタイム拡張

| # | タスク | 根拠 | 工数 |
|---|--------|------|------|
| 5B-1 | **Cloudflare Runtime** — reserved → 実装 | gap P2-7 | 3d |
| 5B-2 | **運用 UI** — Web ダッシュボード（gateway 経由） | gap P2-8 | 5d |

### 5C: Taste Engine 高度化

| # | タスク | 根拠 | 工数 |
|---|--------|------|------|
| 5C-1 | **Image Perceiver** — VLM 推論 + 全 7 軸拡張 | taste Phase 4 | 3d |
| 5C-2 | **Video/Audio Perceiver** — 映像/音声 perceiver + external neural | taste Phase 5 | 5d |
| 5C-3 | **Exemplar-based Adaptation** — QUASAR 方式のゼロショット適応 | taste §6 Level 1 | 2d |

### Wave 5 合計見積り: ~23d

---

## 8. 依存関係グラフ

```
前提条件 (1d)
  │
Wave 1 (DONE)
  │
  ├── Wave 2A: Ingestion 本格化
  │     │
  │     └── Wave 2B: Memory Intelligence + 移行完了
  │
  ├── Wave 2C: Planner/DAG 統合 (independent of 2A/2B)
  │
  ├── Wave 3A: Pseudo-Human Foundation (depends on 2C for planner path)
  │     │     ※ 3A-1〜3A-3, 3A-5〜3A-6 は Wave 1 直後に開始可能
  │     │     ※ 3A-4 のみ 2C 完了が必要
  │     │
  │     └── Wave 4C: Multi-Agent ← depends on 3A (execution separation)
  │
  ├── Wave 3B: 評価基盤 (independent, can start immediately)
  │     │
  │     └── Wave 4B: Taste Learner ← uses eval patterns
  │
  ├── Wave 4A: Taste Engine Phase 1–2 (independent, can start anytime)
  │     │
  │     └── Wave 4B → Wave 5C: Taste 高度化
  │
  └── Wave 5A: RBAC (independent, can start anytime)
        │
        └── Wave 5B: Cloudflare + 運用 UI ← needs RBAC
```

### 並行実行可能なストリーム

| ストリーム | タスク群 | 前提 |
|-----------|----------|------|
| **Memory** | 2A → 2B | Wave 1 |
| **Orchestration** | 2C | Wave 1 |
| **Pseudo-Human** | 3A（大部分） → 3A-4（2C 完了後） | Wave 1（大部分）、2C（一部） |
| **Evaluation** | 3B（即時開始可） | なし |
| **Taste** | 4A（即時開始可） | なし |
| **Security** | 5A（即時開始可） | なし |

**注**: 旧計画では 3A が 2B（inference pass）に依存としていたが、inference pass は既に実装済みのため依存解消。3A の大部分は Wave 1 完了直後に着手可能。

---

## 9. Rollback 戦略

### Schema Migration Rollback

| Migration | Rollback 方法 | 制約 |
|-----------|--------------|------|
| V4 → V5（旧テーブル DROP） | V5 migration 前に `brain.db` の自動バックアップを取得。失敗時はバックアップから復元 | V5 migration は不可逆。復元はバックアップからのみ |
| plan_executions テーブル追加 | `DROP TABLE plan_executions` で rollback 可能 | データ喪失あり（実行中 plan が消える） |

### Feature Rollback

| 機能 | Rollback 方法 |
|------|--------------|
| Ingestion Pipeline | config で poller を無効化（`[ingestion] enabled = false`） |
| Planner/DAG | complexity threshold を無限大に設定 → 常に直接 tool loop |
| Self-Task Queue | `AGENT_PENDING_CAP = 0` で agent ジョブ全拒否 |
| Taste Engine | `[taste] enabled = false` + feature gate で完全除外 |
| Temperature Band | config から温度帯設定を削除 → 従来挙動 |

### Wave 部分失敗時

1. **原則**: 各 Wave は独立 merge 可能。部分的に完了した Wave は完了分のみ merge
2. **禁止**: 半端な schema migration の merge（V5 migration は atomic に実行）
3. **手順**: 失敗タスクを次 Wave に繰り越し、完了タスクのみ merge

---

## 10. リスク評価

| リスク | 深刻度 | 影響 | 緩和策 |
|--------|--------|------|--------|
| V5 旧テーブル DROP で data loss | HIGH | 移行漏れデータの喪失 | migration 前に integrity check + 自動バックアップ。dry-run モード |
| TTL cron の誤削除 | HIGH | 有効データの喪失 | soft-delete + 7 日復元期間 + dry-run モード |
| Self-Task の暴走 | HIGH | 無限ループ / リソース枯渇 | `AGENT_PENDING_CAP=5` 済 + 日次コスト上限 + kill switch |
| Planner LLM が不正 JSON 生成 | MEDIUM | plan 生成失敗 → 直接 tool loop fallback | `PlanParser` のエラーハンドリング + fallback to direct execution |
| Taste Engine の LLM 依存 | MEDIUM | LLM 品質に評価精度が直結 | heuristic fallback + calibration rubric |
| Multi-tenant の破壊的変更 | MEDIUM | Memory trait / SecurityPolicy 変更波及 | trait 追加メソッド（default impl）で後方互換維持 |
| X/Twitter API rate limit | MEDIUM | 取得制限で ingestion 停止 | source 別 rate limiting + exponential backoff + jitter |
| LanceDB/Markdown の機能格差拡大 | LOW | 非 SQLite ユーザーが新機能を使えない | `recall_phased` default impl で graceful degrade。ドキュメントに明記 |

---

## 11. 決定ログ

| 日付 | 決定 | 根拠 |
|------|------|------|
| 2026-02-22 | Wave 1: `retrieval_units` 統一テーブル採用 | 2 テーブル系統の統一で検索品質向上 |
| 2026-02-22 | Wave 1: RRF 融合（weighted-sum ではなく） | ランク位置ベースの方が外れ値に強い |
| 2026-02-22 | Wave 1: `signal_tier` は `MemoryLayer` と直交 | 認知層と信号処理段階は別次元 |
| 2026-02-22 | Wave 1: Discord PoC を Wave 1 に含める | IngestionPipeline の端到端検証に必要 |
| 2026-02-22 | Memory trait は追加のみ（既存メソッド変更不可） | LanceDB/Markdown backend の互換性維持 |
| 2026-02-22 | 計画を Wave 2–5 の 4 段階に分割 | 各 Wave 独立 merge 可能 + 並行ストリーム最大化 |
| 2026-02-22 | 2B-3 Inference Pass を削除 | `run_post_turn_inference_pass()` が `session.rs:477` で既に実行中 |
| 2026-02-22 | Wave 3A の工数を ~9d → ~5.5d に修正 | scheduler/writeback/execution path の大部分が実装済み |
| 2026-02-22 | Planner/DAG を「dead code」から「未接続 production code」に再分類 | 1,714 行 + 40 tests。ToolRegistry/ExecutionContext と互換 |
| 2026-02-22 | 3A の 2B 依存を解消 | inference pass が既に実装済みのため、3A は Wave 1 直後に開始可能 |
| 2026-02-22 | 旧テーブル DROP を Wave 2B に配置 | 旧テーブル残存は技術的負債。早期解消 |
| 2026-02-22 | conversation_history LSP fix を前提条件に配置 | リファクタ時の顕在化リスク回避 |

---

## 全体サマリ

| Wave | 内容 | 見積り | 前提 |
|------|------|--------|------|
| 前提条件 | LSP fix + 旧テーブル deprecation | 1d | — |
| 1 | メモリコア再設計 | **完了** | — |
| 2 | メモリ運用 + オーケストレーション基盤 | ~22d | Wave 1 |
| 3 | 疑似人間基盤 + 評価・観測 | ~10.5d | Wave 2（部分） |
| 4 | Taste Engine + 高度自律 | ~17d | Wave 3（部分） |
| 5 | プラットフォーム拡張 | ~23d | Wave 4（部分） |
| **合計** | | **~73.5d** | |

**次のアクション**: 前提条件（P-1, P-2）→ 6 ストリーム並行着手:
1. **Memory**: 2A-1（Signal Normalizer）
2. **Orchestration**: 2C-1（Agent Loop 接続）
3. **Pseudo-Human**: 3A-1（Bootstrap Hardening）
4. **Evaluation**: 3B-1（Eval CLI 公開）
5. **Taste**: 4A-1（Feature Gates + Config）
6. **Security**: 5A-1（RBAC モデル）

---

## 12. ULW 計画強化版（研究反映・実行順固定）

> 目的: 「性能」「自己一貫性」「センス」を同時に前進させつつ、破綻リスクを最小化する。
>
> 方針: 実装量より先に制御点を接続する。特に `planner` と `writeback guard` を先行して固定する。

### 12.1 成否を分ける 3 つの制御点

| 制御点 | 実装対象 | 目的 | 完了条件 |
|--------|----------|------|----------|
| C1: Planner Controller | `ToolLoop` と `PlanExecutor` 接続 | 自律挙動を plan 駆動へ統一 | 3 step 以上は常に plan 実行。直接 tool loop は threshold 未満のみ |
| C2: Policy Gate | tool 実行 + long-term write の単一関門化 | identity drift と過剰 write を防止 | memory/persona write の 100% が gate 経由。直書き経路 0 件 |
| C3: Taste Judge | `taste.evaluate` の rubric + critique | センス評価を再現可能化 | 3軸スコア + critique + suggestion を安定出力。eval で回帰検知可能 |

### 12.2 最短安全ルート（依存込み）

| Stage | タスク | 日数 | 依存 | 出口ゲート |
|------|--------|------|------|-----------|
| S0 | P-1, P-2（既存前提） | 1d | - | LSP 問題解消、旧テーブル利用の警告可視化 |
| S1 | 2C-1, 2C-2（Planner 接続） | 3d | S0 | plan-or-execute が本線化 |
| S2 | 3A-2, 3A-5（Writeback Guard 強化） | 1.5d | S1 | immutable/source 変更拒否 100% |
| S3 | 3A-1, 3A-3, 3A-4（persona 自己タスク接続） | 2d | S2 | reflect -> self-task -> planner 経由実行が end-to-end で成立 |
| S4 | 2A（ingestion 本格化） + 2B（TTL/Drift/Demotion） | 15d | S1 | 外界信号運用 + 記憶品質メンテが heartbeat で自動化 |
| S5 | 3B（eval/observability） | 5d | S3,S4 | eval/doctor/slo で可視化・回帰検知 |
| S6 | 4A 最小版（taste text critic） | 5d | S5 | taste.evaluate が運用可能 |
| S7 | 4B（pairwise learner） | 3d | S6 | taste.compare で学習反映 |

### 12.3 非機能 KPI（必須）

| 軸 | KPI | 目標 |
|----|-----|------|
| 性能 | p95 turn latency（tool 使用ターン） | 現状比 20% 改善または悪化 10% 以内 |
| 性能 | 1 turn あたり tool call 数 | plan 導入後に平均 15% 以上削減 |
| 自己 | immutable field mutation acceptance | 0 |
| 自己 | policy bypass incidents | 0 |
| 自己 | pending self-task overflow | 0 |
| 記憶品質 | contradiction-resolved ratio | 週次で改善傾向 |
| 記憶品質 | stale trend purge success | 100% |
| センス | taste critique consistency（同入力の揺れ） | しきい値内（内部 rubric で定義） |
| センス | pairwise agreement（Kendall tau） | Phase 3 到達時に >= 0.5 |

### 12.4 Write Policy（研究反映）

- 原則: **default deny write**。昇格条件を満たす場合のみ long-term へ保存。
- 推奨保存先:
  - Raw signal: Tier A
  - corroborated claim: Tier B
  - inferred hypothesis: Tier C（低信頼開始）
  - contradiction/correction: Tier D
- 追加ルール:
  - `source_ref` 不在の外界信号は Tier B へ昇格禁止
  - `risk_flags` 付きは回答文脈注入を既定拒否
  - contradiction penalty > 0.5 で自動 demote

### 12.5 Taste Engine 実装の現実解（Phase 1-3）

| Phase | 範囲 | 実装方針 |
|------|------|---------|
| T1 | Text critic | 3軸（Coherence/Hierarchy/Intentionality）+ rubric 分解 + critique 生成 |
| T2 | UI adapter | ルールベース提案（`UiOp`）を追加。critic は共通利用 |
| T3 | Pairwise learning | `taste.compare` で比較蓄積、Bradley-Terry から開始し不確実性重み付けを導入 |

### 12.6 テストゲート（merge 条件）

| Gate | 必須チェック |
|------|--------------|
| G1 | `cargo fmt -- --check` |
| G2 | `cargo clippy -- -D warnings` |
| G3 | `cargo test`（既存 + 新規） |
| G4 | wave 対応 integration tests（memory/persona/eval/taste） |
| G5 | doctor 出力で新規メトリクス確認 |

### 12.7 実行ポリシー

1. 各 Stage は「出口ゲート達成」でのみ次へ進む。
2. C1/C2 が未完了の状態で self-task 拡張を進めない。
3. Taste は T1 完了前に learner 最適化へ進まない。
4. 重大回帰時は Wave を跨がず、直前 Stage へロールバックして修復する。

---

## 13. Stage 実行チェックリスト（着手用）

> 目的: 各 Stage で「何を実装し」「何を確認したら完了か」を固定する。

### 13.1 S1: Planner Controller（2C-1, 2C-2）

**実装タスク**:
- `ToolLoop` に plan-or-execute 分岐を追加
- complexity threshold 判定ロジック追加
- plan JSON 生成 prompt テンプレート導入
- `PlanParser::parse()` -> `PlanExecutor::execute()` 経路接続

**DoD**:
- 3 step 以上のタスクで plan 経路が必ず選択される
- threshold 未満は直接 tool loop を維持
- invalid plan JSON で direct execution fallback が発動

**失敗時フォールバック**:
- threshold を一時的に引き上げて direct execution 比率を増やす
- parser error を observability に送出して prompt を再調整

### 13.2 S2: Policy Gate（3A-2, 3A-5）

**実装タスク**:
- memory/persona write を単一 gate 関数経由に統一
- immutable field 改変拒否ルール追加
- `source_kind/source_ref` の write 制約追加
- `self_tasks` 件数・長さ・期限の上限制御追加

**DoD**:
- long-term write の 100% が gate 経由
- immutable/source 改変の拒否率 100%
- overflow self-task の拒否確認

**失敗時フォールバック**:
- write policy を strict mode（deny）に固定
- 許可ルールを段階解放（allowlist 方式）

### 13.3 S3: Persona Self-Task Flow（3A-1, 3A-3, 3A-4）

**実装タスク**:
- startup で canonical/mirror reconcile を必ず実行
- reflect 出力から self-task enqueue 接続
- agent job 実行を planner 経由へ固定

**DoD**:
- 起動直後 no-op なし
- reflect -> enqueue -> execute（planner 経由）が integration test で再現
- persona 起点の shell 直打ち 0

**失敗時フォールバック**:
- enqueue のみ有効化し execute を一時停止
- pending cap を低く設定して暴走抑止

### 13.4 S4: Ingestion + Memory Intelligence（2A + 2B）

**実装タスク**:
- RSS/X/Discord ingestion 正式化
- dedup/source rate limit/backoff 実装
- TTL expiry/trend drift/low-confidence demotion 実装
- contradiction monitoring + promotion metrics 実装

**DoD**:
- source 別 ingest が scheduler で安定稼働
- stale/trend/low-confidence メンテが heartbeat で自動化
- old tables DROP（V5）後に整合性チェック通過

**失敗時フォールバック**:
- source ごとに ingest を feature/config で停止可能
- TTL purge を soft-delete only モードへ切替

### 13.5 S5: Eval + Observability（3B）

**実装タスク**:
- `asteroniris eval` 公開
- planner/memory/ingestion benchmark suite 追加
- SLO/SLI 設定 + alert 発火
- doctor 出力へ memory/signal health 追加

**DoD**:
- eval 実行で JSON report が生成
- 主要 KPI が dashboard/doctor で確認可能
- SLO 違反時のログ/アラートが再現可能

**失敗時フォールバック**:
- alert を warning mode で運用して閾値再調整
- benchmark を smoke/full の 2 段階に分離

### 13.6 S6-S7: Taste Minimal to Learning（4A 最小 + 4B）

**実装タスク**:
- `taste.evaluate`（Text 3軸 + critique）導入
- `taste.compare` で pairwise 記録導入
- Bradley-Terry learner + uncertainty weight 導入

**DoD**:
- 同一入力に対する critique 揺れが規定範囲
- compare 経由で順位更新が観測可能
- taste 無効化時（config/feature）に完全停止

**失敗時フォールバック**:
- learner 更新を停止し judge-only 運用に戻す
- uncertainty 高サンプルを学習除外

---

## 14. 研究トレーサビリティ（設計判断の根拠）

| 設計判断 | 反映先 | 根拠カテゴリ |
|----------|--------|--------------|
| planner を turn controller 化 | S1, C1 | long-horizon agent planning / error recovery |
| write の default deny | S2, 12.4 | identity drift mitigation / governance |
| episodic -> semantic 昇格を厳格化 | S4, 12.4 | memory consolidation studies |
| taste を judge module として独立 | S6, C3 | critique-then-revise / rubric evaluation |
| pairwise + uncertainty 重み付け | S7, 12.5 | preference learning under ambiguity |

### 14.1 非採用（現時点）

- 大規模モデル再学習を前提とする設計（運用コスト高）
- 味覚モデルの早期マルチモーダル化（Phase 1 複雑化を回避）
- planner 未接続のまま self-task 拡張（安全境界が不足）

---

## 15. 実装開始テンプレート（毎 Stage 共通）

各 Stage 開始時に以下を満たす:

1. 対象ファイル一覧を確定
2. 追加テスト（unit/integration）を先に列挙
3. 失敗時フォールバック条件を明記
4. 完了時に G1-G5 を必ず実行

完了報告は以下 4 点を最低限含む:

- 変更ファイル
- 受け入れ基準の達成状況
- 実行した検証コマンドと結果
- 残課題（あれば）
