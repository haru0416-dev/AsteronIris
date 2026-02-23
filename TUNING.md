# Tuning Tracker

src/ のチューニング進捗を記録する。

## ランタイム性能

| ID | 箇所 | 内容 | 工数 | 状態 |
|----|------|------|------|------|
| R1 | `memory/markdown.rs` `encode_tag_value` | 全文字ごとのVec割り当て → push loop | 低 | done |
| R2 | `providers/streaming.rs` `scrub_delta` | `self.carry.clone()` → `mem::take` | 低 | done |
| R3 | `agent/loop_/context.rs` | 非external entry の value clone 除去 | 低 | done |
| R4 | `agent/tool_execution.rs` | `to_lowercase()` → ASCII case-insensitive比較 | 低 | done |
| R5 | `memory/sqlite/repository.rs` | signal_tier/source_kind の無意味なラウンドトリップ除去 | 低 | done |
| R6 | `security/policy/trackers.rs` | ツール呼び出し毎のロック5回→1回に統合 | 中 | done |
| R7 | 複数ファイル | `.collect::<Vec<_>>().join()` → 中間Vec除去 (6箇所) | 低 | done |
| R8 | `agent/tool_loop.rs` | 会話履歴毎ターンclone → `Cow`/`Arc`スライス化 | 高 | — |

## コンパイル時間

| ID | 箇所 | 内容 | 工数 | 状態 |
|----|------|------|------|------|
| C1 | 63ファイル | `async-trait` → ネイティブ async fn in trait 移行 | 高 | — |
| C2 | `transport/channels/` | telegram/slack/matrix/irc/whatsapp/imessage に feature gate | 中 | done |
| C3 | `onboard/` | `reqwest` blocking feature 除去 (async化) | 中 | done |
| C4 | `Cargo.toml` | `chacha20poly1305` default-features = false | 低 | done |
| C5 | 4ファイル | `strum` derive → 手動 impl で proc-macro 除去 | 低 | skip (コスパ悪) |
| C6 | `plugins/skillforge/scout.rs` | 751行に3実装混在 → 3ファイル分割 | 低 | done |

## コード品質・保守性

| ID | 箇所 | 内容 | 工数 | 状態 |
|----|------|------|------|------|
| Q1 | `agent/loop_/session.rs` + `run.rs` | tool registry 初期化の重複排除 | 中 | done |
| Q2 | `providers/compatible/mod.rs` | `"NOT_FOUND_FALLBACK::"` センチネル文字列 → typed error | 中 | done |
| Q3 | `memory/sqlite/repository.rs` `append_event` | 266行巨大関数 → フェーズ別分割 | 中 | done |
| Q4 | `transport/channels/message_handler.rs` | `handle_channel_message` 278行 → 4ヘルパー分解 | 中 | done |
| Q5 | `core/agent/loop_/mod.rs` | thin facade 規約違反 → ロジックを `run.rs` へ | 低 | done |
| Q6 | `discord/mod.rs`, `whatsapp/mod.rs` | mod.rs に実装コード → `channel.rs` へ移動 | 低 | done |
| Q7 | `security/writeback_guard/policy.rs` | 本番 `.expect()` → `?` に置換 | 低 | done |
