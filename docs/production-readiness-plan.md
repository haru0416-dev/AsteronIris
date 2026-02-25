# AsteronIris Production Readiness Plan

最終更新日: 2026-02-24

## 1. 目的

本計画書は、AsteronIris を「実運用に耐える状態」へ移行するための修正、検証、リリース判定を定義する。
対象は以下の 3 点である。

1. セキュリティ境界の欠落修正
2. テスト/CI 品質ゲートの復旧
3. CLI・設定・運用挙動の整合

## 2. 現状サマリー

2026-02-24 時点の確認結果:

1. `cargo clippy -- -D warnings` は通過
2. `cargo test --lib` は通過
3. `cargo test-dev` は統合テスト群のコンパイルエラーで不通過
4. README でも本番利用は未推奨と明記されている

## 3. ブロッカー一覧（重大度順）

| ID | 重大度 | 問題 | 主な根拠 |
|---|---|---|---|
| B-01 | Critical | `shell` 実行時に `SecurityPolicy::is_command_allowed` が適用されていない | `src/tools/shell.rs` |
| B-02 | Critical | `file_read`/`file_write` が解決後パスの workspace 内判定をしていない | `src/tools/file_read.rs`, `src/tools/file_write.rs`, `src/security/policy/path.rs` |
| B-03 | Critical | `/v1/chat/completions` がデフォルトで実質無認証 | `src/transport/gateway/openai_compat_handler.rs`, `src/transport/gateway/server.rs` |
| B-04 | Critical | `/ws` が認証境界なしで公開されている | `src/transport/gateway/server.rs`, `src/transport/gateway/websocket.rs` |
| B-05 | High | 統合テスト群が現行公開 API と乖離（`asteroniris::core::*` 参照等） | `src/lib.rs`, `tests/runtime/memory_write_paths.rs` ほか |
| B-06 | ~~High~~ Resolved | ~~`rusqlite` 依存不足により統合テストが不通過~~ rusqlite → sqlx 移行完了 | `tests/memory/delete_contract.rs`, `Cargo.toml` |
| B-07 | High | `eval` コマンドが `todo!()` で panic | `src/app/dispatch.rs`, `src/cli/commands/mod.rs` |
| B-08 | High | API キー解決経路が CLI / Gateway / Channels で不一致 | `src/app/dispatch.rs`, `src/transport/gateway/server.rs`, `src/transport/channels/startup/runtime.rs` |
| B-09 | Medium | pairing token TTL が設定のみで未実装 | `src/config/schema/gateway.rs`, `src/transport/gateway/pairing.rs` |
| B-10 | Medium | CI の sqlite migration/rollback ジョブが存在しないテスト名を実行 | `.github/workflows/ci.yml`, `tests/memory/sqlite_schema.rs` |
| B-11 | Medium | CLI デフォルト値と config デフォルト値に不整合（port/host/temperature） | `src/cli/commands/mod.rs`, `src/config/schema/gateway.rs`, `src/config/schema/core/types.rs` |

## 4. 実行フェーズ

| Phase | 目安期間 | 目的 | 完了条件 |
|---|---|---|---|
| P0 | 0.5 日 | 再現固定、計測、証跡保存 | 失敗コマンド・ログ・対象箇所が issue 化済み |
| P1 | 2 日 | セキュリティ境界修復（B-01〜B-04） | 無認証アクセス・任意コマンド・workspace 外アクセスが再現不能 |
| P2 | 2 日 | テスト/CI 復旧（B-05, B-06, B-10） | `cargo test-dev` と CI が通過 |
| P3 | 1 日 | CLI/設定整合（B-07, B-11） | panic 経路ゼロ、設定値反映を確認 |
| P4 | 1 日 | キー解決/TTL 実装（B-08, B-09） | 経路統一・期限切れ token 拒否を確認 |
| P5 | 1 日 | リリース前検証と運用ドキュメント反映 | Go/No-Go 判定を通過 |

## 5. タスク詳細

| Task | 対象 | 実装内容 | 受け入れ基準 |
|---|---|---|---|
| T-01 | ShellTool | `execute` 冒頭で `is_command_allowed` を必須化 | 禁止コマンドが常に拒否される |
| T-02 | FileReadTool | `is_path_allowed` + `is_resolved_path_allowed` を適用 | `../`/symlink escape で読み出し不可 |
| T-03 | FileWriteTool | T-02 と同様の二段判定を適用 | workspace 外書き込み不可 |
| T-04 | Gateway Auth | `/v1/chat/completions` の無認証許可を廃止 | API key なしは 401 |
| T-05 | WebSocket Auth | `/ws` で bearer/pairing 検証を追加 | 無認証接続が拒否される |
| T-06 | Tool 実行統一 | CLI/Gateway/Channel/Daemon 全経路で同一ポリシー適用 | 実行経路差異がなくなる |
| T-07 | Prompt Hook | `hooks: &[]` の常態化を廃止し最低限 hook を標準化 | leak/security hook が全経路で有効 |
| T-08 | Integration Tests | `asteroniris::core::*` 参照を現行公開 API に移行 | 統合テストのコンパイルエラー解消 |
| T-09 | ~~Test Dependency~~ Resolved | ~~`rusqlite` を `dev-dependencies` へ追加またはテスト改修~~ 全テストを sqlx に移行済 | DB 系統合テストが通過 |
| T-10 | CI Workflow | 実在テスト名へ修正、migration/rollback テストを実体化 | 0 件実行の疑似成功が消える |
| T-11 | Eval Command | `todo!()` 排除（実装 or 明示的 `bail!`） | `asteroniris eval` 非 panic |
| T-12 | CLI Config Merge | `host/port/temperature` を未指定時に config 反映 | config の値が実行時有効 |
| T-13 | API Key Resolver | Gateway/Channels も `config.api_key` を含む同一 resolver へ統一 | CLI/Gateway/Daemon 挙動一致 |
| T-14 | Pairing TTL | token 発行時刻保存、期限判定、期限切れ清掃処理を導入 | 期限超過 token が拒否される |
| T-15 | Regression Tests | B-01〜B-14 の再発防止テスト追加 | 同系不具合の再発を検知可能 |

## 6. 検証計画

### 6.1 必須コマンド

1. `cargo fmt -- --check`
2. `cargo clippy -- -D warnings`
3. `cargo test --lib`
4. `cargo test-dev`

### 6.2 セキュリティ検証

1. 無認証で `/v1/chat/completions` が 401/403 になること
2. 無認証で `/ws` 接続できないこと
3. `shell` で禁止コマンドが拒否されること
4. `file_read`/`file_write` で `../` や symlink escape が拒否されること

### 6.3 運用検証

1. `asteroniris eval` が panic しないこと
2. `host/port/temperature` が CLI 未指定時に config 値を使うこと
3. pairing token が TTL 超過で無効化されること

## 7. Go/No-Go 判定基準

Go 判定は次を全て満たした場合のみ許可する。

1. B-01〜B-08 がクローズ済み
2. `cargo test-dev` が通過
3. CI で 0 件実行の疑似成功ジョブが存在しない
4. ユーザー起点で panic するコマンドが残っていない
5. 認証必須経路が文書化され、手動検証済み

## 8. スケジュール案

| 日付 | 作業 |
|---|---|
| 2026-02-25 | P0, P1 開始 |
| 2026-02-26 | P1 完了 |
| 2026-02-27 | P2 前半 |
| 2026-02-28 | P2 後半完了 |
| 2026-03-01 | P3 完了 |
| 2026-03-02 | P4 完了 |
| 2026-03-03 | P5 完了、Go/No-Go 判定 |

## 9. 成果物

1. セキュリティ修正 PR 群
2. 統合テスト・CI 修正 PR 群
3. 設定/CLI 整合修正 PR 群
4. 失敗再現ログと修正後証跡
5. 運用 Runbook（認証、token ローテーション、緊急遮断手順）

## 10. 即時着手順（推奨）

1. T-01〜T-05 を最優先で実施して攻撃面を閉鎖
2. T-08〜T-10 で品質ゲートを復旧
3. T-11〜T-14 を実施し運用整合を完了
4. T-15 と最終検証後に Go/No-Go 判定

