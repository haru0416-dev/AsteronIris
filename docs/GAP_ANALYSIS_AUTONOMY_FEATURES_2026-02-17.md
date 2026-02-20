# AsteronIris 機能ギャップ調査（自走系エージェント比較）

作成日: 2026-02-17

## 調査目的

本リポジトリ（AsteronIris）に対して、OpenClaw および主要な自走系エージェント実装を参照しながら、
「横断的に不足していそうな機能」を抽出し、優先度付きで整理する。

## 比較対象（外部）

- OpenClaw: https://github.com/openclaw/openclaw
- OpenHands: https://github.com/All-Hands-AI/OpenHands
- SWE-agent: https://github.com/SWE-agent/SWE-agent
- AutoGPT: https://github.com/Significant-Gravitas/AutoGPT
- AutoGen: https://github.com/microsoft/autogen

## リポジトリ内で確認した主要実装領域

- エージェント実行ループ: `src/agent/loop_.rs`
- ゲートウェイ/デーモン/実行制御: `src/gateway/mod.rs`, `src/daemon/mod.rs`, `src/runtime/mod.rs`
- メモリ層: `src/memory/mod.rs` と各 backend 実装
- ツール層: `src/tools/mod.rs` と各ツール (`shell`, `file_read`, `file_write`, `browser` など)
- Provider 層: `src/providers/mod.rs` と各 provider
- セキュリティポリシー: `src/security/policy.rs`
- スケジューラ/heartbeat: `src/cron/scheduler.rs`, `src/heartbeat/engine.rs`
- 観測性: `src/observability/mod.rs`（log/otel/prometheus backend）
- 統合カタログ: `src/integrations/registry.rs`

---

## 結論サマリ

AsteronIris は「ローカル実行型エージェント」の土台（ループ、ツール、メモリ、provider、チャネル、基本安全制約）は強い。
一方で、OpenClaw 系の 24/7 自走運用や SWE-agent 系の検証主導フローと比較した場合、
不足は主に **上位オーケストレーション層（計画・評価・運用）** に集中している。

---

## 不足機能リスト（優先度付き）

### P0（最優先）

1. **専用 Planner / DAG 実行層**
   - 現状はターンベースの実行ループが中心で、計画の明示的データ構造（DAG/依存グラフ）と実行分離が弱い。
   - 影響: 長時間タスクや複数依存タスクで失敗率が上がりやすい。
   - 根拠: `src/agent/loop_.rs`, `src/daemon/mod.rs`, `src/cron/scheduler.rs`

2. **自動検証ループ（実装→テスト→失敗解析→再試行）**
   - ツール実行はあるが、継続的な品質ゲート（回帰検証フロー）の標準化が弱い。
   - 影響: 自走実装の品質ばらつき、失敗時の自己修復限界。
   - 根拠: `src/agent/loop_.rs`, `src/tools/shell.rs`

3. **評価基盤（Eval Harness）**
   - 成功率・コスト・遅延・再試行率を継続計測し比較できる eval 面が不足。
   - 影響: 改善の定量判断が困難。
   - 根拠: `src/observability/mod.rs`（backend はあるが、評価運用一式が未統合）

### P1（高優先）

4. **マルチエージェント協調オーケストレーション**
   - session/daemon はあるが、役割分担型の本格協調（planner/executor/reviewer など）の標準化が不足。
   - 影響: 複雑課題の並列処理能力に上限。
   - 根拠: `src/daemon/mod.rs`, `src/channels/mod.rs`, `src/runtime/mod.rs`

5. **運用観測（ダッシュボード/SLO/アラート）**
   - OTel/Prometheus backend 自体はあるが、運用導線（SLO、異常検知、継続可視化）を含む完成度が不足。
   - 影響: 障害予兆検知と原因分析の速度が落ちる。
   - 根拠: `src/observability/mod.rs`, `src/observability/otel.rs`, `src/observability/prometheus.rs`

6. **RBAC / マルチテナント境界の高度化**
   - workspace 制約中心で、細粒度の権限モデルやユーザ/テナント分離が弱い。
   - 影響: 複数ユーザ・共有運用時の安全性/運用性低下。
   - 根拠: `src/security/policy.rs`

### P2（中優先）

7. **Cloudflare runtime 実装（現状は予約/未対応）**
   - runtime kind として予約されているが未実装のまま。
   - 影響: 実行環境拡張性の欠落。
   - 根拠: `src/config/schema.rs`（reserved not implemented）、`src/runtime/mod.rs`（unsupported path）、`tests/runtime_adapter_contract_cloudflare.rs`

8. **運用 UI / リモート制御体験の強化**
   - Gateway と CLI はあるが、OpenClaw 系と比較して運用 UI の統合体験が弱い。
   - 影響: 非開発者運用・24/7 運用のハードル上昇。
   - 根拠: `src/main.rs`, `src/gateway/mod.rs`

---

## 内部ロードマップ上の「未充足サイン」

以下は実装コードだけでなく、リポジトリ内計画文書からも確認できるギャップ兆候。

- platform hardening 計画で Cloudflare/runtime/observability/CI hardening が未充足テーマとして明示
  - 根拠: `.sisyphus/plans/platform-hardening.md`
- Cloudflare runtime は契約テストを伴う将来対応として扱われている
  - 根拠: `tests/runtime_adapter_contract_cloudflare.rs`, `src/runtime/mod.rs`

---

## 比較観点（OpenClaw 等との相対差分）

相対的に差が大きいのは次の領域。

1. **24/7 運用導線**（cron/doctor/運用面 UX の統合度）
2. **自動回復と検証主導ループ**（失敗解析と再試行の標準化）
3. **評価運用**（継続 benchmark / regression 評価）
4. **多主体協調**（複数エージェント構成の標準化）

---

## 推奨実装順（最短で効果を出す順）

1. Planner/DAG + 自動検証ループ（P0）
2. Eval Harness（P0）
3. 協調オーケストレーションと RBAC 強化（P1）
4. Observability 運用完成度の底上げ（P1）
5. Cloudflare runtime と運用 UI 強化（P2）

---

## 付記

本ドキュメントは「不足機能の探索と整理」に限定しており、実装変更は含まない。
次段として、各不足機能を issue 化（受け入れ条件・工数・依存関係付き）すると、実行計画に直結しやすい。
