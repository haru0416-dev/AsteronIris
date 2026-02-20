# Pseudo-Human Agent Architecture Spec (AsteronIris)

## 1. Purpose

この仕様は、AsteronIrisを「エージェントっぽさ」ではなく、以下を模倣する疑似人間的システムへ段階的に拡張するための設計案を定義する。

- 自己タスク立案 (self-tasking)
- 回答ゆらぎの制御 (controlled variability)
- 記憶に基づく推論更新 (memory inference)
- 疑似人格の一貫運用 (bounded persona adaptation)

## 2. Design Principles

- 既存の traits-first 構成を維持する
- 安全境界を先に強化し、その後に自律性を段階導入する
- 「自己進化」は無制限最適化ではなく、監査可能な更新のみ許可する
- 機能は `decision layer` と `execution layer` を分離する

## 3. Current Anchors in Codebase

- Decision loop: `src/agent/loop_.rs`
- Persona state model: `src/persona/state_header.rs`
- Persona persistence/mirror: `src/persona/state_persistence.rs`
- Writeback validation: `src/security/writeback_guard.rs`
- Memory abstraction/events: `src/memory/traits.rs`
- Sqlite memory behavior: `src/memory/sqlite.rs`
- Scheduler execution path: `src/cron/mod.rs`, `src/cron/scheduler.rs`
- Heartbeat runner: `src/heartbeat/engine.rs`
- Security boundary: `src/security/policy.rs`
- Runtime/orchestration: `src/daemon/mod.rs`, `src/channels/mod.rs`

## 4. Target Architecture

4層構造で運用する。

1. **Conversation Layer**
   - 通常応答とツール使用
   - 入口: main session turn

2. **Reflection/Writeback Layer**
   - 対話後の反省・状態更新候補生成
   - 入口: persona reflect writeback

3. **Self-Task Layer**
   - 自己生成タスクのキュー化・実行
   - 入口: cron/heartbeat

4. **Governance Layer**
   - writeback guard / security policy / observability
   - すべての更新と実行に対する制約・監査

## 5. Scope and Non-Goals

### In Scope

- persona起点の自己タスク追加と上限管理
- 記憶イベントに推論更新の概念を追加
- 自律経路に対する温度帯制御
- 実行経路横断での action/cost 制限の一貫適用

### Out of Scope

- AGI 的な自己改造
- 無制限な shell 実行の自律化
- 安全ポリシーを迂回する高速化

## 6. Phased Implementation Plan

### Phase 0: Bootstrap Hardening

目的: 起動時に persona state が未初期化で空振りする問題を解消。

- `BackendCanonicalStateHeaderPersistence` の初期化経路を明示
- startup 時に canonical と mirror の reconcile を必ず実行
- canonical 不在時は最小 `StateHeaderV1` をseed

完了条件:

- 起動直後の反省処理が no-op にならない
- mirror と backend state の不整合が検出・修復される

### Phase 1: Writeback Guard Expansion

目的: 自己タスクとスタイル更新を安全に受け入れる。

- `writeback_guard` に許可フィールドを追加
  - `self_tasks` (件数/長さ/期限制限)
  - `style_profile` (安全な範囲のみ)
- 既存の immutable field 保護は厳守
- poisoning pattern 検出を継続

完了条件:

- 不変フィールド改変は100%拒否
- 追加フィールドが上限違反時に拒否される

### Phase 2: Self-Task Queue with Boundaries

目的: 自己タスクを「提案 -> 制約付き実行」に限定。

- `CronJob` にメタデータ追加
  - `job_kind`, `origin`, `expires_at`, `max_attempts`
- persona生成ジョブは `job_kind=agent` として分離
- pending上限 (例: 5) を設定し、越えた提案は拒否

完了条件:

- persona由来ジョブの無制限増殖が起きない
- 期限切れジョブが自動的に失効する

### Phase 3: Execution Path Separation

目的: persona起点実行をshell直通から隔離。

- schedulerで `job_kind=user` と `job_kind=agent` を分岐
- `agent` job は直接 shell ではなく制約付きエージェント経路へ
- `SecurityPolicy::record_action`/cost制限を全経路で適用

完了条件:

- persona起点の shell 直打ちが0件
- action/cost 制限の適用漏れがない

### Phase 4: Memory Inference and Contradiction Control

目的: 記憶を単なる保存から「検証付き更新」へ。

- turn後に inference pass を追加
- `InferredClaim` と `ContradictionMarked` をイベント化
- sqlite backend で矛盾ペナルティを検索順位に反映

完了条件:

- 推論イベントが保存・検索に反映される
- 矛盾イベントが優先度制御に作用する

### Phase 5: Controlled Variability Band

目的: 応答ゆらぎをモード別に制御。

- configで温度帯を定義 (min/max, mode)
- 会話経路は適応帯、自律経路は低分散帯
- すべてのtemperature適用を境界値にclamp

完了条件:

- 温度が設定帯域外に出ない
- 自律実行で過度なゆらぎが発生しない

## 7. Safety Invariants (Must Hold)

- immutable persona fields は更新不可
- workspace外アクセス禁止
- allowlist 外コマンド実行禁止
- 日次コスト上限・時間あたりアクション上限を厳守
- writeback payload は検証通過時のみ反映

## 8. Success Metrics

- Safety
  - immutable field mutation acceptance: `0`
  - policy bypass incidents: `0`
- Autonomy
  - pending self-task queue overflow: `0`
  - expired tasks auto-cleanup rate: `100%`
- Memory quality
  - contradiction-resolved ratio: 継続改善
  - stale inference persistence: 継続低下
- Variability control
  - out-of-band temperature events: `0`

## 9. Risks and Mitigations

- Risk: 自己タスクが実質 shell 自動実行になる
  - Mitigation: `job_kind=agent` 分離と専用実行経路
- Risk: 記憶推論の誤差蓄積
  - Mitigation: contradictionイベントと重み減衰
- Risk: スタイル更新が人格逸脱を誘発
  - Mitigation: style_profileの許容範囲を固定し急変を拒否

## 10. Minimal Validation Matrix

- Unit tests
  - writeback guard: 上限違反/毒性/immutable改変
  - scheduler: job_kind分岐, expiry, retry上限
  - memory: inferred/contradictionの保存と重み反映
- Integration tests
  - persona反省 -> self-task enqueue -> bounded execution
  - policy制限 (cost/action/path/command) の横断適用

## 11. Rollout Strategy

- デフォルトは保守設定 (自律拡張OFF)
- feature flag / config gate で段階展開
- doctor/status に可観測項目を追加後に本番展開

## 12. Decision Log (Initial)

- Rewriteではなく拡張を採用
- persona writeback を意思決定の中核として採用
- self-task は scheduler経路へ分離し、監査下で実行
- memory inference は sqlite-first で導入

## 13. Memory Domain Model (Detailed)

記憶は「保存先」ではなく「信号処理パイプライン」として扱う。

### 13.1 Memory Tiers

- **Tier A: Signal Ledger (Raw, append-only)**
  - 目的: 外界信号を改変なしで保存
  - 形: `MemoryEventType::FactAdded` を中心に保存
  - 例: Discord生発話、X投稿本文、ニュース見出し
- **Tier B: Working Beliefs (Resolvable slots)**
  - 目的: 現在有効な信念スロットを解決
  - 形: `resolve_slot(entity_id, slot_key)`
  - 例: `user.preference.language`, `market.trend.ai_agents`
- **Tier C: Inferred Memory (Hypothesis)**
  - 目的: 推論に基づく仮説を明示管理
  - 形: `InferredClaim` + `confidence` + provenance
- **Tier D: Governance Trail (Correction/forget)**
  - 目的: 矛盾・削除・トゥームストーンを監査可能に保持
  - 形: `ContradictionMarked`, `SoftDeleted`, `TombstoneWritten`

### 13.2 Event Envelope Rules

既存 `MemoryEventInput` を活かし、追加メタは `value` をJSON文字列で包んで持つ。

- 必須キー
  - `text`: 元情報テキスト
  - `source_kind`: `discord|x|news|trend|webhook|manual`
  - `source_ref`: 投稿URL/メッセージID/記事URLなど
  - `ingest_ts`: 収集時刻(RFC3339)
  - `lang`: 推定言語
- 推奨キー
  - `author_id`, `author_handle`
  - `topic_tags`: 正規化済みタグ配列
  - `risk_flags`: `rumor|unverified|sensitive|policy_risky`
  - `quality`: `primary|secondary|unknown`

## 14. External Signal Architecture (Discord, X, News, Trends)

### 14.1 Ingestion Sources

- **Discord**
  - 既存経路: `src/channels/mod.rs` の message bus で受信・自動保存済み
  - 拡張: channel metadata を value envelope 化
- **X (Twitter)**
  - 推奨経路: schedulerジョブで pull -> memory append
  - 代替: webhook gateway経由
- **News**
  - 推奨経路: RSS/API poller を scheduler で周期実行
- **Trend signals**
  - 推奨経路: 複数信号から集約した summary event を定期生成

### 14.2 Normalize -> Store Pipeline

1. Acquire (API/webhook/channel)
2. Normalize (encoding/lang/timestamp/source_ref)
3. Classify (topic/entity/risk)
4. De-duplicate (source_ref + semantic similarity)
5. Append to Tier A
6. Optional inference to Tier C
7. Contradiction check and belief update (Tier B)

## 15. Entity and Slot Taxonomy

`src/memory/scope.rs` の命名規約を拡張し、外界信号を一貫IDで保持する。

- Entity IDs
  - `channel:discord:sender:<id>`
  - `feed:x:author:<id>`
  - `feed:news:publisher:<id>`
  - `trend:topic:<slug>`
- Slot keys
  - `signal.discord.message`
  - `signal.x.post`
  - `signal.news.article`
  - `trend.snapshot.hourly`
  - `belief.topic.sentiment`
  - `belief.topic.momentum`

## 16. Confidence, Importance, and Drift Control

### 16.1 Base Scoring Heuristic

- confidence 初期値目安
  - discord direct quote: `0.95`
  - verified official news: `0.85`
  - unverified repost/rumor: `0.35-0.60`
  - inferred claim: `<=0.70` で開始
- importance 初期値目安
  - user-specific preference: `0.80+`
  - market/general chatter: `0.40-0.65`

### 16.2 Drift and Decay

- topic系slotは time-decay を適用
- trend slot はTTL短め (例: 6h/24h)
- 矛盾イベントが入ったslotは confidence を減衰
- `ContradictionMarked` が閾値超えで auto-demote

## 17. Promotion Rules (Raw -> Belief)

Raw信号を即座に長期信念へ昇格しない。

- 昇格条件
  - 同一主張が独立ソースで複数回観測
  - source quality が `primary` または検証済み
  - policy/risk flag が許容範囲
- 非昇格条件
  - 単発のバズ/煽り
  - 高リスク語を含む未検証情報
  - 外部リンク先が不明/取得失敗

## 18. Retrieval Strategy for Reasoning

回答時は全記憶を投げず、問い合わせ意図で層別抽出する。

- Phase R1: entity-scoped recall (`RecallQuery`)
- Phase R2: recent trend snapshot (TTL内のみ)
- Phase R3: contradiction trail 参照
- Phase R4: final context synthesis

実装アンカー:

- `src/agent/loop_.rs` の `build_context` を拡張し、trend/beliefの優先読込を追加

## 19. Scheduler and Heartbeat Responsibilities

### 19.1 Scheduler (`src/cron/scheduler.rs`)

- 外界信号 pull ジョブを担当
- source別レート制限
- 失敗時 backoff + jitter
- 同一 `source_ref` 再取り込み防止

### 19.2 Heartbeat (`src/heartbeat/engine.rs`)

- 「記憶品質メンテ」担当
  - stale trend purge
  - contradiction ratio 監視
  - low-confidence bulk demotion

## 20. Security and Trust Boundaries for External Signals

- `source_ref` がない外界信号は昇格禁止
- `risk_flags` 付き信号は既定で回答文脈に直接注入しない
- `PrivacyLevel::Secret` は channel応答文脈へ流さない
- persona writeback から外界ソース定義を直接変更不可

## 21. Observability Additions

`src/observability/*` と `doctor` に以下を追加して運用監視する。

- `signal_ingest_total{source_kind}`
- `signal_dedup_drop_total{source_kind}`
- `belief_promotion_total{topic}`
- `contradiction_mark_total{topic}`
- `stale_trend_purge_total`

## 22. Extended Validation Matrix

- Unit
  - envelope JSON validation (required keys/malformed)
  - dedup key (`source_ref`) 判定
  - promotion gate 判定
- Integration
  - Discord ingest -> trend update -> recall反映
  - X/news ingest -> contradiction -> belief demotion
  - high-risk signal が回答文脈へ混入しないこと

---

この仕様は「最終像」ではなく、既存安全境界を崩さずに疑似人間性を高めるための実装ロードマップである。

## 23. External-Content Prompt Injection Defense

本節は、外部コンテンツ経由 Prompt Injection への防衛を設計として固定する。

### 23.1 Threat Model (対象と境界)

- 対象攻撃
  - 外部テキスト内の命令注入 (`ignore previous instructions` 等)
  - role/system/developer 偽装
  - 境界マーカー偽装（同形異字含む）
  - transcript/memory への再注入
- 信頼区分
  - `trusted`: system prompt とユーザー直接入力
  - `untrusted_external`: channel/webhook/tool 由来の外部本文
  - `derived_summary`: untrusted を要約・抽出した再利用可能情報
- 原則
  - 未分類入力は必ず `untrusted_external` として扱う

### 23.2 Security Objectives

- 外部本文は命令として解釈させない
- 生の外部本文が再びモデル入力へ戻る経路を閉じる
- 正常文を過剰ブロックせず、可用性を維持する

### 23.3 Core Defense Pipeline

1. Classify
   - 入力を `trusted` / `untrusted_external` / `derived_summary` に分類
2. Wrap
   - `untrusted_external` は統一境界でラップして警告文を前置
3. Sanitize
   - 入力内の境界マーカー/同形異字偽装を無害化
4. Detect
   - 注入シグナル（命令上書き、role偽装、エンコード回避）を検知
5. Decide
   - `allow | sanitize | block | audit` を決定
6. Persist
   - 保存時は raw payload を抑制し summary + source + digest を保存

### 23.4 Module Design

- 新規モジュール: `src/security/external_content.rs`
- 公開API:
  - `wrap_external_content(source: &str, text: &str) -> String`
  - `sanitize_marker_collision(text: &str) -> String`
  - `detect_injection_signals(text: &str) -> InjectionSignals`
  - `decide_external_action(signals: &InjectionSignals) -> ExternalAction`
  - `summarize_for_persistence(source: &str, wrapped: &str) -> PersistedExternalSummary`
- 型:
  - `enum ExternalAction { Allow, Sanitize, Block, Audit }`
  - `struct InjectionSignals { score: u8, flags: Vec<String> }`
  - `struct PersistedExternalSummary { source: String, summary: String, digest: String }`

### 23.5 Integration Points

- Channels ingress
  - `src/channels/mod.rs`
  - `chat_with_system` 呼び出し前に `classify -> wrap/sanitize -> decide` を適用
- Gateway ingress
  - `src/gateway/mod.rs`
  - `provider.chat(...)` 呼び出し前に同一パイプラインを適用
- Main agent loop
  - `src/agent/loop_.rs`
  - `build_context` 結合前に外部由来断片へ同一処理を適用
- Memory/writeback
  - `src/agent/loop_.rs` の `append_event` 前で raw 抑制
  - `src/security/writeback_guard.rs` と整合する形式で保存

### 23.6 Action Policy (default behavior)

- `Allow`
  - 注入シグナル低・偽装なし
- `Sanitize` (default)
  - 注入シグナル中程度。本文は保持しつつ命令語/偽装境界を無効化
- `Block`
  - 高信頼で危険シグナル一致。モデルへ本文を渡さず拒否応答
- `Audit`
  - 本文は流すが強制ログ化（検証フェーズ向け）

### 23.7 Persistence and Replay Rules

- 保存可
  - `source`, `summary`, `digest`, `flags`, `timestamp`
- 保存不可
  - 生の `untrusted_external` 本文
- 再投入可
  - `derived_summary` のみ
- 再投入不可
  - raw external payload

### 23.8 Test Matrix (External Injection)

- Unit
  - marker collision 無害化（ASCII/同形異字）
  - 注入シグナル検知（override/role spoof/encoding evasions）
  - action 判定 (`allow/sanitize/block/audit`) の閾値整合
- Integration
  - channel受信 -> モデル入力で raw 注入文が不在
  - gateway受信 -> block/sanitize 分岐が期待通り
  - memory再利用 -> raw payload が再投入されない

### 23.9 Acceptance Criteria

- 外部コンテンツ経由の攻撃サンプルで、生payloadがモデル入力に残らない
- 危険シグナル高の入力は `block` または `sanitize` へ必ず遷移
- 正常入力の誤遮断率を運用許容値内に維持

### 23.10 Rollout

- Step 1: `Audit` で観測のみ
- Step 2: `Sanitize` を既定化
- Step 3: 高信頼シグナルのみ `Block` を有効化
