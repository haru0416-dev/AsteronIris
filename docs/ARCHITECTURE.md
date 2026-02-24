# AsteronIris アーキテクチャドキュメント

> **目的**: 今後の拡張・最適化・オンボーディングのための包括的な技術リファレンス。  
> **最終更新**: 2026-02-22  
> **対象バージョン**: 0.1.0 (Rust 2024 edition)

---

## 目次

1. [プロジェクト概要](#1-プロジェクト概要)
2. [アーキテクチャ設計原則](#2-アーキテクチャ設計原則)
3. [モジュール構造](#3-モジュール構造)
4. [コアトレイト一覧](#4-コアトレイト一覧)
5. [ファクトリ関数一覧](#5-ファクトリ関数一覧)
6. [エージェント会話ループ](#6-エージェント会話ループ)
7. [メモリシステム](#7-メモリシステム)
8. [LLMプロバイダシステム](#8-llmプロバイダシステム)
9. [ツールシステム](#9-ツールシステム)
10. [トランスポート層](#10-トランスポート層)
11. [セキュリティシステム](#11-セキュリティシステム)
12. [プランナーシステム](#12-プランナーシステム)
13. [セッション管理](#13-セッション管理)
14. [ペルソナシステム](#14-ペルソナシステム)
15. [評価システム](#15-評価システム)
16. [プラグインシステム](#16-プラグインシステム)
17. [ランタイムシステム](#17-ランタイムシステム)
18. [設定システム](#18-設定システム)
19. [テスト構造](#19-テスト構造)
20. [ビルドとデプロイ](#20-ビルドとデプロイ)
21. [拡張ガイド](#21-拡張ガイド)
22. [システム不変条件と運用指針](#22-システム不変条件と運用指針)

---

## 1. プロジェクト概要

AsteronIris は **Rust 製のセキュア・拡張可能な AI アシスタント**。CLI・デーモン・マルチチャネル I/O・プラガブルメモリ・強化ゲートウェイを一つのバイナリに統合する。

### 技術スタック

| カテゴリ           | 技術                                                    |
| ------------------ | ------------------------------------------------------- |
| 言語               | Rust 2024 edition, stable toolchain                     |
| 非同期ランタイム   | Tokio (`rt-multi-thread`)                               |
| HTTP サーバ        | Axum (HTTP/1 + WebSocket)                               |
| HTTP クライアント  | reqwest (rustls)                                        |
| 永続化             | rusqlite (SQLite), LanceDB (Arrow-native vector DB)     |
| シリアライズ       | serde + serde_json, TOML                                |
| 暗号化             | ChaCha20-Poly1305 (AEAD), HMAC-SHA256                   |
| Lint               | clippy pedantic (`-D warnings`)                         |
| エラーハンドリング | anyhow (アプリケーション層), thiserror (ライブラリ境界) |
| i18n               | rust-i18n (コンパイル時多言語)                          |

### エントリポイント

```
src/main.rs  →  Cli::parse()  →  Config::load_or_init()  →  app::dispatch::dispatch(cli, config)
src/lib.rs   →  ライブラリとして全 pub モジュールを re-export
```

**main.rs の起動シーケンス**:

1. Rustls暗号プロバイダのインストール (`ring::default_provider`)
2. tracing ログの初期化 (`Level::INFO`)
3. Clap による CLI パース
4. `Config::load_or_init()` で TOML 設定読み込み（存在しなければ初期化）
5. `app::dispatch::dispatch()` で CLI コマンドをルーティング

### 主要依存クレート

```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", ...] }
axum = { version = "0.8", features = ["http1", "json", "tokio", "query", "ws"] }
reqwest = { version = "0.13", features = ["json", "rustls", "multipart", "stream"] }
rusqlite = { version = "0.38" }
serde / serde_json = "1"
chacha20poly1305 = "0.10"
clap = { version = "4.5", features = ["derive"] }
async-trait = "0.1"
lancedb = { version = "0.26.2", optional = true }
```

---

## 2. アーキテクチャ設計原則

### 2.1 Trait + Factory Dispatch パターン

全サブシステムは以下の構造に従う:

```
1. trait を定義         → 例: pub trait Memory: Send + Sync { ... }
2. factory 関数を実装    → 例: pub fn create_memory(config, ...) -> Box<dyn Memory>
3. 複数の実装を提供      → 例: SqliteMemory, LanceDbMemory, MarkdownMemory
4. 呼び出し側は trait object で操作
```

**Box → Arc の境界ルール**:

- Factory は `Box<dyn Trait>` を返す
- 共有が必要な箇所で `Arc<dyn Trait>` にラップ
- 単一所有の場合は `Box` のまま使用

### 2.2 Thin Facade パターン

各 `mod.rs` は **ファサード専用**:

- `pub mod` 宣言のみ
- `pub use` による re-export
- **ロジックは書かない** — 集約されたサブモジュールに分離

```rust
// src/core/providers/mod.rs — 典型例
pub mod anthropic;
pub mod factory;
pub mod traits;
// ...
pub use factory::{create_provider, create_resilient_provider};
pub use traits::Provider;
```

### 2.3 Deny-by-Default セキュリティ

全てのシェル実行・ファイルアクセス・ネットワークバインドは**明示的許可が必要**:

- コマンド allowlist（デフォルト: git, npm, cargo, ls, cat 等）
- パス allowlist（ワークスペース内のみ）
- ゲートウェイは localhost のみバインド（公開バインドはブロック）
- LLM I/O の全てでシークレットスクラビング

### 2.4 Feature Gate システム

```toml
[features]
default = ["discord", "email", "vector-search", "tui", "bundled-sqlite", "media", "link-extraction"]
discord = []
email = ["dep:lettre", "dep:mail-parser"]
vector-search = ["dep:lancedb", "dep:arrow-array", "dep:arrow-schema"]
tui = ["dep:ratatui", "dep:crossterm"]
bundled-sqlite = ["rusqlite/bundled"]
media = ["dep:infer", "dep:mime"]
mcp = ["dep:rmcp"]
link-extraction = ["dep:scraper"]
```

条件コンパイル例:

- `#[cfg(feature = "vector-search")]` → LanceDB インポートをゲート
- `#[cfg(feature = "discord")]` → Discord チャネル・承認ブローカーをゲート
- `#[cfg(feature = "mcp")]` → Model Context Protocol サポートをゲート

### 2.5 エラーハンドリング規約

- `anyhow::Result<T>` — 全 fallible public 関数
- `anyhow::bail!()` — 早期リターン
- `thiserror::Error` — ライブラリ境界の構造化エラー
- **本番コードで `unwrap()` / `expect()` 禁止**（テストコードでは許可）
- 空の `catch` ブロック禁止

---

## 3. モジュール構造

```
src/
├── main.rs                    # バイナリエントリポイント
├── lib.rs                     # ライブラリルート (pub mod + pub use)
│
├── app/                       # アプリケーション起動・ディスパッチ
│   ├── dispatch.rs            # CLI コマンドルーティング
│   └── status.rs              # ステータス表示
│
├── cli/                       # CLI 定義
│   ├── commands.rs            # Clap CLI 構造体
│   └── mod.rs
│
├── commands/                  # コマンドハンドラ
│   ├── cli.rs                 # CLI コマンド処理
│   ├── handlers.rs            # 各コマンドのハンドラ
│   ├── parser.rs              # コマンドパーサ
│   └── types.rs               # コマンド型定義
│
├── config/                    # 設定システム
│   ├── mod.rs                 # Config re-export
│   └── schema/                # TOML スキーマ定義
│       ├── mod.rs
│       ├── core/              # コア設定型 + ローダー + 暗号化 + 環境変数オーバーライド
│       ├── autonomy.rs        # 自律性ポリシー設定
│       ├── channels.rs        # チャネル設定
│       ├── gateway.rs         # ゲートウェイ設定
│       ├── memory.rs          # メモリバックエンド設定
│       ├── mcp.rs             # MCP 設定
│       ├── observability.rs   # 可観測性設定
│       ├── tools.rs           # ツール設定
│       └── tunnel.rs          # トンネル設定
│
├── core/                      # AI コアシステム (8 サブシステム)
│   ├── mod.rs                 # ファサード
│   │
│   ├── agent/                 # 会話ループ + ツール実行
│   │   ├── mod.rs             # run() re-export
│   │   ├── loop_/             # メイン会話ループ
│   │   │   ├── mod.rs         # run() エントリポイント
│   │   │   ├── session.rs     # セッションターン実行
│   │   │   ├── context.rs     # メモリコンテキスト構築
│   │   │   ├── inference.rs   # ポストターン推論
│   │   │   ├── reflect.rs     # ペルソナリフレクション
│   │   │   ├── verify_repair.rs # 検証/修復エスカレーション
│   │   │   └── types.rs       # ターンパラメータ型
│   │   ├── tool_loop.rs       # ToolLoop::run() — ツール反復実行
│   │   ├── tool_execution.rs  # ツール結果フォーマット・信頼境界
│   │   └── tool_types.rs      # ツールループ型定義
│   │
│   ├── eval/                  # 評価ハーネス
│   │   ├── mod.rs             # EvalHarness re-export
│   │   ├── harness.rs         # 決定論的評価ランナー
│   │   ├── types.rs           # EvalReport, EvalSuiteSpec
│   │   └── rng.rs             # 再現可能乱数生成器
│   │
│   ├── memory/                # プラガブルメモリバックエンド
│   │   ├── mod.rs             # Memory trait + factory re-export
│   │   ├── traits.rs          # Memory trait 定義
│   │   ├── factory.rs         # create_memory() ファクトリ
│   │   ├── memory_types.rs    # データ構造体定義
│   │   ├── capability.rs      # バックエンド能力マトリクス
│   │   ├── chunker.rs         # ドキュメントチャンカー
│   │   ├── consolidation.rs   # メモリ統合パイプライン
│   │   ├── embeddings.rs      # EmbeddingProvider trait + factory
│   │   ├── vector.rs          # ベクトル演算 (cosine similarity, hybrid merge)
│   │   ├── sqlite/            # SQLite バックエンド
│   │   │   ├── mod.rs         # SqliteMemory
│   │   │   ├── schema.rs      # スキーマ (memories, FTS5, embedding_cache, belief_slots)
│   │   │   ├── repository.rs  # CRUD + 競合解決
│   │   │   ├── search.rs      # ベクトル + キーワードハイブリッド検索
│   │   │   ├── events.rs      # イベント処理
│   │   │   ├── projection.rs  # 検索結果フォーマット
│   │   │   └── codec.rs       # エンコード/デコード
│   │   ├── lancedb/           # LanceDB バックエンド (feature-gated)
│   │   │   ├── mod.rs         # LanceDbMemory
│   │   │   ├── interface.rs   # LanceDB インターフェース
│   │   │   ├── query.rs       # クエリ実行
│   │   │   ├── batch.rs       # バッチ操作
│   │   │   ├── backfill.rs    # 非同期エンベディングバックフィル
│   │   │   └── conversions.rs # Arrow 型変換
│   │   ├── markdown.rs        # Markdown バックエンド (追記専用)
│   │   └── hygiene/           # メモリ衛生
│   │       ├── mod.rs         # 衛生オーケストレーション
│   │       ├── prune.rs       # リテンションポリシー適用
│   │       ├── filesystem.rs  # 孤立ファイル削除
│   │       └── state.rs       # 衛生状態管理
│   │
│   ├── persona/               # ペルソナ状態管理
│   │   ├── mod.rs
│   │   ├── state_header.rs    # StateHeader JSON スキーマ
│   │   └── state_persistence.rs # ファイルベース永続化
│   │
│   ├── planner/               # DAG ベースプラン実行
│   │   ├── mod.rs             # Plan, PlanExecutor re-export
│   │   ├── types.rs           # Plan, PlanStep, StepAction, StepStatus
│   │   ├── dag_contract.rs    # DagContract, DagNode, DagEdge
│   │   ├── executor.rs        # PlanExecutor, StepRunner trait
│   │   └── parser.rs          # PlanParser
│   │
│   ├── providers/             # LLM プロバイダ抽象化
│   │   ├── mod.rs             # Provider + factory re-export
│   │   ├── traits.rs          # Provider trait 定義
│   │   ├── factory.rs         # create_provider() 等ファクトリ
│   │   ├── response.rs        # ProviderResponse, ContentBlock, StopReason
│   │   ├── streaming.rs       # ProviderStream, StreamSink trait
│   │   ├── scrub.rs           # シークレットスクラビング
│   │   ├── reliable.rs        # ReliableProvider (リトライ + フォールバック)
│   │   ├── oauth_recovery.rs  # OAuthRecoveryProvider
│   │   ├── http_client.rs     # HTTP クライアントビルダー
│   │   ├── tool_convert.rs    # ツール変換ユーティリティ
│   │   ├── fallback_tools.rs  # フォールバックツール処理
│   │   ├── sse.rs             # Server-Sent Events パーサー
│   │   ├── compatible.rs      # OpenAI 互換プロバイダ
│   │   ├── anthropic.rs       # Anthropic 実装
│   │   ├── openai.rs          # OpenAI 実装
│   │   ├── gemini.rs          # Google Gemini 実装
│   │   ├── ollama.rs          # Ollama 実装
│   │   └── openrouter.rs      # OpenRouter 実装
│   │
│   ├── sessions/              # セッション管理
│   │   ├── mod.rs             # SessionManager, SqliteSessionStore re-export
│   │   ├── types.rs           # Session, ChatMessage, SessionState
│   │   ├── store.rs           # SessionStore trait + SqliteSessionStore
│   │   ├── manager.rs         # SessionManager
│   │   └── compaction.rs      # メッセージコンパクション
│   │
│   └── tools/                 # ツール trait + 実装群
│       ├── mod.rs             # Tool, ToolRegistry re-export
│       ├── traits.rs          # Tool trait, ActionOperator trait
│       ├── factory.rs         # default_tools(), all_tools()
│       ├── registry.rs        # ToolRegistry (HashMap + middleware chain)
│       ├── middleware.rs       # ToolMiddleware trait + 実装
│       ├── shell.rs           # ShellTool
│       ├── file_read.rs       # FileReadTool
│       ├── file_write.rs      # FileWriteTool
│       ├── memory_store.rs    # MemoryStoreTool
│       ├── memory_recall.rs   # MemoryRecallTool
│       ├── memory_forget.rs   # MemoryForgetTool
│       ├── memory_governance.rs # MemoryGovernanceTool
│       ├── browser_open.rs    # BrowserOpenTool
│       ├── browser/           # BrowserTool (エージェントブラウザ)
│       └── composio.rs        # ComposioTool (1000+ アプリ統合)
│
├── transport/                 # 外部 I/O
│   ├── mod.rs
│   ├── channels/              # 9 メッセージングプラットフォーム
│   │   ├── mod.rs             # Channel trait + factory re-export
│   │   ├── traits.rs          # Channel trait 定義
│   │   ├── factory.rs         # build_channels()
│   │   ├── cli.rs             # CLI チャネル
│   │   ├── telegram/          # Telegram (Bot API long-poll)
│   │   ├── discord/           # Discord (WebSocket gateway + HTTP API)
│   │   ├── slack.rs           # Slack (RTM API)
│   │   ├── matrix.rs          # Matrix (homeserver federation)
│   │   ├── whatsapp/          # WhatsApp (Cloud API webhooks)
│   │   ├── email_channel.rs   # Email (IMAP/SMTP)
│   │   ├── irc/               # IRC (RFC 1459)
│   │   ├── imessage/          # iMessage (macOS)
│   │   ├── message_handler.rs # メッセージ処理パイプライン
│   │   ├── runtime.rs         # 監視付きリスナー起動
│   │   ├── startup.rs         # チャネル起動オーケストレーション
│   │   ├── policy.rs          # チャネル別ポリシー
│   │   ├── ingress_policy.rs  # 外部コンテンツ安全ポリシー
│   │   ├── prompt_builder.rs  # システムプロンプト生成
│   │   ├── chunker.rs         # メッセージ分割
│   │   ├── attachments.rs     # メディア添付処理
│   │   └── health.rs          # ヘルスチェック分類
│   │
│   └── gateway/               # Axum HTTP ゲートウェイ
│       ├── mod.rs             # AppState 定義
│       ├── server.rs          # run_gateway(), ルート構築
│       ├── handlers.rs        # HTTP エンドポイントハンドラ
│       ├── websocket.rs       # WebSocket ハンドラ
│       ├── events.rs          # WebSocket メッセージ型
│       ├── openai_compat_handler.rs  # OpenAI 互換 API
│       ├── openai_compat_auth.rs     # API キー認証
│       ├── openai_compat_types.rs    # ChatCompletion 型
│       ├── openai_compat_streaming.rs # SSE ストリーミング
│       ├── defense.rs         # 外部 ingress ポリシー適用
│       ├── signature.rs       # WhatsApp 署名検証
│       ├── replay_guard.rs    # リプレイ攻撃検知
│       └── autosave.rs        # メモリ自動保存
│
├── security/                  # セキュリティシステム
│   ├── mod.rs                 # ファサード + re-export
│   ├── policy/                # SecurityPolicy (deny-by-default)
│   │   ├── mod.rs             # SecurityPolicy 構造体
│   │   ├── types.rs           # AutonomyLevel, ActionPolicyVerdict
│   │   ├── command.rs         # コマンド allowlist
│   │   ├── path.rs            # パス allowlist
│   │   ├── trackers.rs        # ActionTracker, CostTracker
│   │   └── tenant.rs          # テナントスコープ
│   ├── approval.rs            # ApprovalBroker trait + リスク分類
│   ├── approval_cli.rs        # CLI 承認ブローカー
│   ├── approval_channel.rs    # チャネル承認ブローカー
│   ├── approval_telegram.rs   # Telegram 承認ブローカー
│   ├── approval_discord.rs    # Discord 承認ブローカー (feature-gated)
│   ├── pairing.rs             # PairingGuard (ペアリング認証)
│   ├── secrets.rs             # SecretStore (ChaCha20-Poly1305)
│   ├── permissions.rs         # PermissionStore (グラント管理)
│   ├── url_validation.rs      # SSRF 防止
│   ├── external_content.rs    # 外部コンテンツ検証
│   ├── auth/                  # 認証サブシステム
│   │   ├── mod.rs
│   │   ├── broker.rs          # AuthBroker
│   │   ├── store.rs           # AuthProfileStore
│   │   └── oauth.rs           # OAuth フロー
│   └── writeback_guard/       # 書き戻しガード
│       ├── mod.rs
│       ├── validation.rs      # validate_writeback_payload()
│       ├── types.rs           # WritebackPayload
│       ├── constants.rs       # サイズ制限、ポイズンパターン
│       ├── field_validators.rs    # フィールドレベル検証
│       └── profile_validators.rs  # プロファイル検証
│
├── plugins/                   # プラグインシステム
│   ├── mod.rs
│   ├── skillforge/            # スキル自動発見・評価エンジン
│   │   ├── mod.rs             # SkillForge re-export
│   │   ├── forge.rs           # SkillForge オーケストレーター
│   │   ├── scout.rs           # Scout trait (発見)
│   │   ├── gate.rs            # Gate (4層セキュリティゲート)
│   │   ├── evaluate.rs        # 評価パイプライン
│   │   ├── integrate.rs       # 統合 (マニフェスト生成)
│   │   ├── tiers.rs           # SkillTier 分類
│   │   ├── patterns.rs        # ReasonCode (拒否理由)
│   │   ├── capabilities.rs    # ケイパビリティ定義
│   │   ├── config.rs          # SkillForgeConfig
│   │   ├── overrides.rs       # 手動オーバーライド
│   │   └── provenance.rs      # 出所追跡
│   ├── skills/                # スキルローダー
│   │   ├── mod.rs
│   │   └── loader.rs          # スキルの読み込み・管理
│   ├── mcp/                   # Model Context Protocol (feature-gated)
│   │   ├── mod.rs
│   │   ├── client_manager.rs  # create_mcp_tools()
│   │   ├── client_connection.rs
│   │   ├── client_proxy_tool.rs # McpToolProxy
│   │   ├── bridge.rs          # MCP ブリッジ
│   │   ├── content.rs         # コンテンツ変換
│   │   └── server/            # MCP サーバ実装
│   └── integrations/          # 統合レジストリ
│       ├── mod.rs
│       ├── registry.rs        # 統合レジストリ
│       └── inventory.rs       # スコープロックインベントリ
│
├── runtime/                   # ランタイムシステム
│   ├── mod.rs
│   ├── environment/           # 環境アダプタ
│   │   ├── mod.rs
│   │   ├── traits.rs          # RuntimeAdapter trait
│   │   ├── native.rs          # ネイティブ実装
│   │   └── docker.rs          # Docker 実装
│   ├── tunnel/                # トンネルシステム
│   │   ├── mod.rs
│   │   ├── traits.rs          # Tunnel trait
│   │   ├── factory.rs         # create_tunnel()
│   │   ├── cloudflare.rs      # Cloudflare Tunnel
│   │   ├── tailscale.rs       # Tailscale
│   │   ├── ngrok.rs           # Ngrok
│   │   ├── custom.rs          # カスタムトンネル
│   │   └── none.rs            # No-op
│   ├── observability/         # 可観測性
│   │   ├── mod.rs
│   │   ├── traits.rs          # Observer trait
│   │   ├── log.rs             # Log-based
│   │   ├── otel.rs            # OpenTelemetry
│   │   ├── prometheus.rs      # Prometheus
│   │   └── noop.rs            # No-op
│   ├── usage/                 # 使用量追跡
│   │   ├── mod.rs
│   │   └── tracker.rs         # UsageTracker trait + SQLite 実装
│   └── diagnostics/           # 診断
│       ├── mod.rs
│       ├── doctor/            # システム診断
│       └── heartbeat/         # ハートビート監視
│
├── platform/                  # プラットフォーム管理
│   ├── mod.rs
│   ├── daemon/                # デーモンスーパーバイザ
│   │   ├── mod.rs
│   │   ├── supervisor.rs      # コンポーネント監視 + 再起動
│   │   └── heartbeat_worker.rs
│   ├── cron/                  # クロンスケジューラ
│   │   ├── mod.rs
│   │   ├── scheduler.rs       # ジョブスケジューリング
│   │   └── repository.rs      # ジョブ永続化
│   └── service/               # OS サービス管理
│       └── mod.rs
│
├── onboard/                   # オンボーディングウィザード
│   ├── mod.rs
│   ├── wizard.rs              # ウィザードオーケストレーター
│   ├── flow.rs                # オンボーディングフロー
│   ├── tui/                   # Terminal UI
│   │   ├── mod.rs
│   │   ├── app.rs             # TUI アプリ状態
│   │   └── steps/             # オンボーディングステップ
│   └── prompts/               # インタラクティブプロンプト
│   ├─ templates/             # テンプレートファイル
│
├── media/                     # メディア処理
│   ├── mod.rs
│   ├── detection.rs           # MIME 型検出
│   ├── processing.rs          # メディア処理パイプライン
│   └── storage.rs             # メディアストレージ
│
├── ui/                        # Terminal UI スタイル
│   └── mod.rs
│
└── utils/                     # 共有ユーティリティ
    ├── mod.rs
    └── links/                 # URL 検出・抽出
        └── mod.rs
```

---

## 4. コアトレイト一覧

### 4.1 Provider trait

**ファイル**: `src/core/providers/traits.rs`

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    /// テキストメッセージを送信し、テキスト応答を取得
    async fn chat(&self, message: &str, model: &str, temperature: f64) -> Result<String>;

    /// システムプロンプト付きテキストチャット
    async fn chat_with_system(
        &self, system_prompt: Option<&str>, message: &str,
        model: &str, temperature: f64,
    ) -> Result<String>;

    /// HTTP 接続プール暖機 (TLS ハンドシェイク、DNS、HTTP/2)
    async fn warmup(&self) -> Result<()>;

    /// フルレスポンス（メタデータ含む）を返すチャット
    async fn chat_with_system_full(
        &self, system_prompt: Option<&str>, message: &str,
        model: &str, temperature: f64,
    ) -> Result<ProviderResponse>;

    /// ツール付き構造化チャット
    async fn chat_with_tools(
        &self, system_prompt: Option<&str>, messages: &[ProviderMessage],
        tools: &[ToolSpec], model: &str, temperature: f64,
    ) -> Result<ProviderResponse>;

    /// ネイティブ構造化ツール呼び出しサポート可否
    fn supports_tool_calling(&self) -> bool;

    /// ストリーミングレスポンスサポート可否
    fn supports_streaming(&self) -> bool;

    /// ビジョン（画像入力）サポート可否
    fn supports_vision(&self) -> bool;

    /// ツール付きストリーミングチャット
    async fn chat_with_tools_stream(
        &self,
        system_prompt: Option<&str>,
        messages: &[ProviderMessage],
        tools: &[ToolSpec],
        model: &str,
        temperature: f64,
    ) -> Result<ProviderStream>;
}
```

**実装一覧**: AnthropicProvider, OpenAiProvider, GeminiProvider, OllamaProvider, OpenRouterProvider, OpenAiCompatibleProvider, ReliableProvider, OAuthRecoveryProvider

### 4.2 Memory trait

**ファイル**: `src/core/memory/traits.rs`

```rust
#[async_trait]
pub trait Memory: Send + Sync {
    fn name(&self) -> &str;
    async fn health_check(&self) -> bool;
    async fn append_event(&self, input: MemoryEventInput) -> Result<MemoryEvent>;
    async fn append_inference_event(&self, event: MemoryInferenceEvent) -> Result<MemoryEvent>;
    async fn append_inference_events(&self, events: Vec<MemoryInferenceEvent>) -> Result<Vec<MemoryEvent>>;
    async fn recall_scoped(&self, query: RecallQuery) -> Result<Vec<MemoryRecallItem>>;
    async fn resolve_slot(&self, entity_id: &str, slot_key: &str) -> Result<Option<BeliefSlot>>;
    async fn forget_slot(
        &self, entity_id: &str, slot_key: &str,
        mode: ForgetMode, reason: &str,
    ) -> Result<ForgetOutcome>;
    async fn count_events(&self, entity_id: Option<&str>) -> Result<usize>;
}
```

**実装一覧**: SqliteMemory, LanceDbMemory (feature-gated), MarkdownMemory

### 4.3 Tool trait

**ファイル**: `src/core/tools/traits.rs`

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    async fn execute(&self, args: Value, ctx: &ExecutionContext) -> Result<ToolResult>;
    fn spec(&self) -> ToolSpec;  // デフォルト実装あり
}
```

**実装一覧**: ShellTool, FileReadTool, FileWriteTool, MemoryStoreTool, MemoryRecallTool, MemoryForgetTool, MemoryGovernanceTool, BrowserOpenTool, BrowserTool, ComposioTool, McpToolProxy

### 4.4 Channel trait

**ファイル**: `src/transport/channels/traits.rs`

```rust
#[async_trait]
pub trait Channel: Send + Sync {
    fn name(&self) -> &str;
    async fn send(&self, message: &str, recipient: &str) -> Result<()>;
    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> Result<()>;
    async fn health_check(&self) -> bool;             // デフォルト: true
    fn max_message_length(&self) -> usize;             // デフォルト: usize::MAX
    async fn send_typing(&self, recipient: &str) -> Result<()>;   // デフォルト: no-op
    async fn send_media(&self, attachment: &MediaAttachment, recipient: &str) -> Result<()>;
    async fn edit_message(&self, channel_id: &str, message_id: &str, content: &str) -> Result<()>;
    async fn delete_message(&self, channel_id: &str, message_id: &str) -> Result<()>;
    async fn send_chunked(&self, message: &str, recipient: &str) -> Result<()>;
}
```

**実装一覧**: CliChannel, TelegramChannel, DiscordChannel, SlackChannel, IMessageChannel, MatrixChannel, WhatsAppChannel, EmailChannel, IrcChannel

### 4.5 その他のトレイト

| トレイト            | ファイル                          | 目的                           |
| ------------------- | --------------------------------- | ------------------------------ |
| `ToolMiddleware`    | `core/tools/middleware.rs`        | ツール実行前後のインターセプト |
| `ActionOperator`    | `core/tools/traits.rs`            | 外部アクション実行             |
| `ApprovalBroker`    | `security/approval.rs`            | ツール実行承認リクエスト処理   |
| `RuntimeAdapter`    | `runtime/environment/traits.rs`   | プラットフォーム抽象化         |
| `Tunnel`            | `runtime/tunnel/traits.rs`        | トンネルプロバイダ抽象化       |
| `Observer`          | `runtime/observability/traits.rs` | 可観測性バックエンド           |
| `UsageTracker`      | `runtime/usage/tracker.rs`        | 使用量追跡                     |
| `SessionStore`      | `core/sessions/store.rs`          | セッション永続化               |
| `EmbeddingProvider` | `core/memory/embeddings.rs`       | エンベディングモデル抽象化     |
| `StreamSink`        | `core/providers/streaming.rs`     | ストリーミングレスポンスシンク |
| `StepRunner`        | `core/planner/executor.rs`        | プランステップ実行             |
| `Scout`             | `plugins/skillforge/scout.rs`     | スキル発見                     |

---

## 5. ファクトリ関数一覧

### プロバイダ系

| 関数                                                                                 | ファイル                    | 説明                                            |
| ------------------------------------------------------------------------------------ | --------------------------- | ----------------------------------------------- |
| `create_provider(name, api_key)`                                                     | `core/providers/factory.rs` | プロバイダ名から `Box<dyn Provider>` を生成     |
| `create_resilient_provider(name, api_key, reliability)`                              | 同上                        | リトライ + フォールバックチェーン付きプロバイダ |
| `create_resilient_provider_with_resolver(name, reliability, resolver)`               | 同上                        | API キーリゾルバ付きの耐障害プロバイダ          |
| `create_provider_with_oauth_recovery(config, name, api_key)`                         | 同上                        | OAuth トークンリフレッシュ付きプロバイダ        |
| `create_resilient_provider_with_oauth_recovery(config, name, reliability, resolver)` | 同上                        | OAuth + リトライ + フォールバック               |

**プロバイダ名マッピング**:

- プライマリ: `anthropic`, `openai`, `ollama`, `gemini`/`google`, `openrouter`
- OpenAI互換: `venice`, `groq`, `mistral`, `xai`/`grok`, `deepseek`, `together`/`together-ai`, `fireworks`/`fireworks-ai`, `perplexity`, `cohere`, `moonshot`/`kimi`, `glm`/`zhipu`, `minimax`, `qianfan`/`baidu`, `vercel`, `cloudflare`, `copilot`/`github-copilot`, `opencode`/`opencode-zen`, `zai`/`z.ai`, `synthetic`
- カスタム: `custom:https://...` (OpenAI互換), `anthropic-custom:https://...` (Anthropic互換)

**API キー解決順序**: 明示引数 → プロバイダ固有環境変数 → `ASTERONIRIS_API_KEY` → `API_KEY`

### メモリ系

| 関数                                                        | ファイル                    | 説明                         |
| ----------------------------------------------------------- | --------------------------- | ---------------------------- |
| `create_memory(config, workspace_dir, api_key)`             | `core/memory/factory.rs`    | メモリバックエンド生成       |
| `create_embedding_provider(provider, api_key, model, dims)` | `core/memory/embeddings.rs` | エンベディングプロバイダ生成 |
| `persist_inference_events(memory, events)`                  | `core/memory/factory.rs`    | 推論イベント永続化           |

### ツール系

| 関数                                                                                  | ファイル                   | 説明                                                  |
| ------------------------------------------------------------------------------------- | -------------------------- | ----------------------------------------------------- |
| `default_tools(security)`                                                             | `core/tools/factory.rs`    | デフォルトツールセット (shell, file_read, file_write) |
| `all_tools(security, memory, composio_key, browser_config, tools_config, mcp_config)` | 同上                       | 全ツール (設定に基づく条件付き)                       |
| `default_action_operator(security)`                                                   | 同上                       | NoopOperator                                          |
| `tool_descriptions(browser_enabled, composio_enabled, mcp_config)`                    | 同上                       | システムプロンプト用ツール説明                        |
| `default_middleware_chain(...)`                                                       | `core/tools/middleware.rs` | デフォルトミドルウェアスタック                        |

### トランスポート系

| 関数                              | ファイル                               | 説明                             |
| --------------------------------- | -------------------------------------- | -------------------------------- |
| `build_channels(channels_config)` | `transport/channels/factory.rs`        | `Vec<ChannelEntry>` を生成（チャネル + ポリシーのラッパー） |
| `run_gateway(...)`                | `transport/gateway/server.rs`          | Axum サーバ起動                  |
| `run_gateway_with_listener(...)`  | 同上                                   | カスタムリスナー付きゲートウェイ |
| `build_system_prompt(...)`        | `transport/channels/prompt_builder.rs` | システムプロンプト構築           |

### ランタイム系

| 関数                      | ファイル                       | 説明              |
| ------------------------- | ------------------------------ | ----------------- |
| `create_tunnel(config)`   | `runtime/tunnel/factory.rs`    | トンネル実装生成  |
| `create_observer(config)` | `runtime/observability/mod.rs` | Observer 実装生成 |

---

## 6. エージェント会話ループ

### 6.1 全体フロー図

```
┌─────────────────────────────────────────────────────────────┐
│                    ユーザー入力                                │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│  [1] メモリコンテキスト強化 (context.rs)                       │
│      ├─ RecallQuery で意味検索                                │
│      ├─ エントリの撤回マーカー検証                               │
│      └─ コンテキストをメッセージに前置                            │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│  [2] ツールループ実行 (tool_loop.rs)                           │
│      ┌──────────────────────────────────────────────┐        │
│      │  Provider.chat_with_tools()                   │        │
│      │  → ProviderResponse (text + tool_calls)       │        │
│      │  → ToolRegistry.execute() per tool_call       │        │
│      │  → ToolResult → ProviderMessage::tool_result  │        │
│      │  → ループ (stop_reason != ToolUse まで)        │        │
│      └──────────────────────────────────────────────┘        │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│  [3] ポストターン推論 (inference.rs)                           │
│      ├─ INFERRED_CLAIM マーカーのパース                        │
│      ├─ CONTRADICTION_EVENT マーカーのパース                    │
│      └─ メモリイベントとして永続化                               │
│      └─ ⨉ セキュリティ注意: LLM 出力からのマーカーパースは      │
│         プロンプトインジェクション経由で偽マーカーが    │
│         注入可能。厳密な JSON スキーマ検証を推奨。       │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│  [4] ペルソナリフレクション (reflect.rs)                        │
│      ├─ 現在のペルソナ状態読み込み                               │
│      ├─ リフレクションプロバイダ呼び出し                          │
│      ├─ 書き戻しガードで検証                                    │
│      └─ 更新済み状態を永続化                                    │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────┐
│                   レスポンス出力                                │
└─────────────────────────────────────────────────────────────┘
```

### 6.2 エントリポイント

**ファイル**: `src/core/agent/loop_/mod.rs`

`run()` 関数が会話ループのエントリポイント:

1. メモリバックエンドの初期化 (`create_memory()`)
2. ツールレジストリの構築 (`all_tools()` → `ToolRegistry`)
3. LLM プロバイダの解決 (回答用 + リフレクション用)
4. システムプロンプトの構築 (ツール説明 + スキル)
5. インタラクティブ or 単発メッセージモードへ
6. 各ユーザーメッセージに対し `execute_main_session_turn_with_metrics()` を呼び出す

### 6.3 ツールループ詳細

**ファイル**: `src/core/agent/tool_loop.rs`

`ToolLoop::run()` の反復フロー:

```
iterations = 0
loop {
    iterations += 1
    if iterations > max_iterations (hard cap = 25):
        return MaxIterations

    response = chat_once(provider, messages, tool_specs)
    // ストリーミング対応: supports_streaming() → chat_with_tools_stream()
    // 非ストリーミング: chat_with_tools()

    messages.push(response.to_assistant_message())

    if response.has_tool_use():
        for tool_call in response.tool_use_blocks():
            result = registry.execute(tool_call.name, tool_call.input, ctx)
            // ミドルウェアチェーン実行
            messages.push(ProviderMessage::tool_result(id, content, is_error))
            tool_calls.push(ToolCallRecord { ... })

        if rate_limited: return RateLimited
        continue  // 次のイテレーションへ

    return Completed  // ツール呼び出しなし = 完了
}
```

**LoopStopReason の種類**:

- `Completed` — プロバイダがツール呼び出しなしで応答完了
- `MaxIterations` — ハードキャップ (25回) に到達
- `RateLimited` — エンティティアクション制限超過
- `Error(String)` — プロバイダエラー
- `ApprovalDenied` — 承認ブローカーが拒否

**メッセージ履歴の進化**:

```
初期:   [User(enriched_message)]
ターン1: [User(...), Assistant(response + tool_calls)]
ツール1: [User(...), Assistant(...), ToolResult(result)]
ターン2: [User(...), Assistant(...), ToolResult(...), Assistant(more_tools)]
...
最終:   [User(...), ..., Assistant(final_response)]
```

### 6.4 ツール実行フロー

**ファイル**: `src/core/tools/registry.rs`

`ToolRegistry::execute(name, args, ctx)`:

```
1. ツール検索: HashMap から name で検索
   └─ 見つからない場合: ToolResult { success: false, error: "Tool not found" }

2. ミドルウェアチェーン (before_execute):
   ├─ SecurityMiddleware: 自律性レベル、ツール許可リスト、コマンド/パスポリシー
   ├─ EntityRateLimitMiddleware: エンティティごとのアクション制限
   ├─ AuditMiddleware: ツール実行ログ
   └─ OutputSizeLimitMiddleware: 出力サイズ制限

   各ミドルウェアの判定:
   ├─ Continue → 次のミドルウェアへ
   ├─ Block(reason) → ToolResult { success: false, error: reason }
   └─ RequireApproval(intent) → ApprovalBroker に委任
       ├─ Approved → 続行
       ├─ ApprovedWithGrant → PermissionStore に保存 → 続行
       └─ Denied { reason } → ToolResult { success: false, error: reason }

3. ツール実行: tool.execute(args, ctx)

4. ミドルウェアチェーン (after_execute):
   ├─ ToolResultSanitizationMiddleware: 外部コンテンツのマーカーラップ
   └─ SecretScrubMiddleware: API キー/トークンの除去
```

**ExecutionContext 構造体**:

```rust
pub struct ExecutionContext {
    pub security: Arc<SecurityPolicy>,
    pub autonomy_level: AutonomyLevel,
    pub entity_id: String,
    pub turn_number: u32,
    pub workspace_dir: PathBuf,
    pub allowed_tools: Option<HashSet<String>>,
    pub permission_store: Option<Arc<PermissionStore>>,
    pub rate_limiter: Arc<EntityRateLimiter>,
    pub tenant_context: TenantPolicyContext,
    pub approval_broker: Option<Arc<dyn ApprovalBroker>>,
}
```

### 6.5 信頼境界注入

`augment_prompt_with_trust_boundary()` がシステムプロンプトに信頼ポリシーを追加:

- ツール利用可能時: `## Tool Result Trust Policy` セクション + `[[external-content:tool_result:*]]` マーカー
- ツールなし時: プロンプトをそのまま返す

---

## 7. メモリシステム

### 7.1 データモデル

#### MemoryEventInput (イベント入力)

```rust
pub struct MemoryEventInput {
    pub entity_id: String,          // 例: "user:123"
    pub slot_key: String,           // 例: "preferences.language"
    pub event_type: MemoryEventType,
    pub value: String,              // メモリコンテンツ
    pub source: MemorySource,
    pub confidence: f64,            // [0.0, 1.0]
    pub importance: f64,            // [0.0, 1.0]
    pub layer: MemoryLayer,
    pub privacy_level: PrivacyLevel,
    pub provenance: Option<MemoryProvenance>,
}
```

#### 列挙型

**MemoryEventType**:
`FactAdded`, `FactUpdated`, `PreferenceSet`, `PreferenceUnset`, `InferredClaim`, `ContradictionMarked`, `SoftDeleted`, `HardDeleted`, `TombstoneWritten`, `SummaryCompacted`

**MemorySource** (信頼度デフォルト):
| ソース | デフォルト信頼度 |
|--------|----------------|
| `ExplicitUser` | 0.95 |
| `ToolVerified` | 0.90 |
| `System` | 0.80 |
| `Inferred` | 0.70 |

**MemoryLayer**:
`Working` (デフォルト), `Episodic`, `Semantic`, `Procedural`, `Identity`

**PrivacyLevel**: `Public`, `Private`, `Secret`

**ForgetMode**: `Soft`, `Hard`, `Tombstone`

#### BeliefSlot (信念スロット)

エンティティごとのスロットの現在の状態を追跡:

- 勝者イベント、ソース、信頼度、重要度
- ステータス: active, soft_deleted, hard_deleted, tombstoned

### 7.2 SQLite バックエンド（デフォルト）

**ファイル**: `src/core/memory/sqlite/`

#### スキーマ (`schema.rs`)

| テーブル               | 目的                                                                                                             |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `memories`             | コアテーブル (id, key, content, category, layer, provenance, embedding BLOB, timestamps)                         |
| `memories_fts`         | FTS5 仮想テーブル (BM25 キーワード検索、トリガーで自動同期)                                                      |
| `embedding_cache`      | LRU キャッシュ (content_hash → embedding, accessed_at)                                                           |
| `memory_events`        | イベントログ (entity_id, slot_key, event_type, source, confidence, importance, layer, privacy_level, timestamps) |
| `belief_slots`         | 現在の信念状態 (entity_id, slot_key, value, status, winner_event_id, source, confidence, importance)             |
| `retrieval_docs`       | 検索結果フォーマット用プロジェクション                                                                           |
| `deletion_ledger`      | 削除追跡 (entity_id, slot_key, deleted_value, confidence, importance, marked_at)                                 |

#### ハイブリッド検索 (`search.rs`)

```
最終スコア = vector_weight × vec_score + keyword_weight × kw_score
            (デフォルト: 0.7 × cosine_similarity + 0.3 × BM25_normalized)
```

- **ベクトル検索**: 格納されたエンベディングに対する cosine similarity (0–1)
- **キーワード検索**: FTS5 BM25 スコアリング — min-max 正規化で [0, 1] にスケーリング（バッチ内の最小/最大 BM25 スコアを基準）
- **重複排除**: ID ベースの統合、スコア正規化、重み適用

> **注意**: BM25 は本来非有界のため、正規化方法がランキング安定性に直結する。現在は検索バッチ内の min-max 正規化を採用。クエリ間での一貫性が必要な場合は rank-based blending への移行を検討。

#### エンベディングキャッシュ

- SHA-256 コンテンツハッシュ → エンベディング BLOB
- LRU エビクション: アクセス時刻順に上位 `cache_max` (デフォルト 10,000) を保持
- 同一コンテンツの冗長 API 呼び出しを回避

#### 競合解決 (`repository.rs`)

信念スロットの勝者決定ルール:

1. ソース優先度: `ExplicitUser` > `ToolVerified` > `System` > `Inferred`
2. タイムスタンプ（新しい方が優先）
3. 信頼度

**矛盾ペナルティ**: `confidence -= (0.12 + 0.10 × confidence + 0.08 × importance)`

> **不変条件**: ペナルティ適用後、confidence は `[0.0, 1.0]` にクランプされる。importance も `[0.0, 1.0]` の範囲で管理されること。confidence=0.1, importance=1.0 の場合ペナルティ=0.21 となり負値に達するため、クランプ処理は必須。

#### リインデックス

安全なリインデックスプロセス:

1. テンポラリ DB 作成
2. データシード
3. 同期
4. アトミックスワップ（失敗時ロールバック）

### 7.3 LanceDB バックエンド（feature-gated）

**ファイル**: `src/core/memory/lancedb/`

- Arrow ネイティブカラムナ型ベクトル DB
- 非同期バックフィルワーカー（指数バックオフ: 200ms → 30s、最大5リトライ）
- FTS + ベクトルインデックスのハイブリッド検索

**Forget セマンティクスの劣化**:
| モード | 動作 |
|--------|------|
| Soft | マーカー書き換え: `__LANCEDB_DEGRADED_SOFT_FORGET_MARKER__` |
| Tombstone | マーカー書き換え: `__LANCEDB_DEGRADED_TOMBSTONE_MARKER__` |
| Hard | 行の物理削除 |

### 7.4 Markdown バックエンド

**ファイル**: `src/core/memory/markdown.rs`

- `workspace/MEMORY.md` — キュレートされた長期メモリ
- `workspace/memory/YYYY-MM-DD.md` — 日次ログ（追記専用）

フォーマット:

```markdown
- **key** [md:layer=semantic;provenance_source_class=explicit_user]: value
```

制限: ベクトル検索なし、物理削除不可

### 7.5 エンベディングシステム

**ファイル**: `src/core/memory/embeddings.rs`

```rust
pub trait EmbeddingProvider: Send + Sync {
    fn name(&self) -> &str;
    fn dimensions(&self) -> usize;  // 0 = keyword-only
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
}
```

| 実装                     | 次元数    | 用途                         |
| ------------------------ | --------- | ---------------------------- |
| `NoopEmbedding`          | 0         | キーワードのみ               |
| `DeterministicEmbedding` | 設定可能  | テスト (FNV-1a + SplitMix64) |
| OpenAI                   | 1536/3072 | text-embedding-3-small/large |
| Anthropic                | 設定可能  | claude embeddings            |
| Local                    | 設定可能  | Ollama/Hugging Face          |

**ベクトル演算** (`vector.rs`):

- `cosine_similarity(a, b)` → f32 [0, 1]
- `vec_to_bytes(v)` / `bytes_to_vec(bytes)` — Little-endian シリアライズ
- `hybrid_merge(vector_results, keyword_results, weights)` — ID 重複排除 + スコア正規化 + 重み適用

### 7.6 メモリ統合 (Consolidation)

**ファイル**: `src/core/memory/consolidation.rs`

Working/Episodic メモリを Semantic サマリーに変換:

1. エンティティごとのウォーターマーク追跡（最後に統合されたイベント数）
2. チェックポイント時: ユーザーメッセージ + アシスタント応答から統合値を構築
3. Semantic 層メモリとして保存 (provenance: `memory.consolidation.session_to_semantic`)
4. ウォーターマークを `memory_consolidation_state.json` に永続化

### 7.7 メモリ衛生 (Hygiene)

**ファイル**: `src/core/memory/hygiene/`

**リテンション階層**:
| レイヤー | 保持期間 |
|---------|---------|
| Working | 2日 |
| Episodic | 30日 |
| Semantic | 永久 |
| Procedural | 永久 |
| Identity | 永久 |

**衛生プロセス**: `run_if_due(config, workspace_dir)` が間隔経過時に実行

- プルーニング: リテンション超過エントリの soft/hard/tombstone 削除
- ファイルシステムクリーンアップ: 孤立した日次ログの削除
- 状態管理: 最終実行タイムスタンプの永続化

### 7.8 バックエンド能力マトリクス

| バックエンド | Soft Forget | Hard Forget | Tombstone | ベクトル検索 | キーワード検索 |
| ------------ | ----------- | ----------- | --------- | ------------ | -------------- |
| SQLite       | ✓ 完全      | ✓ 完全      | ✓ 完全    | ✓ cosine     | ✓ FTS5 BM25    |
| LanceDB      | ⚠ 劣化      | ✓ 完全      | ⚠ 劣化    | ✓ ANN        | ✓ FTS          |
| Markdown     | ⚠ 劣化      | ✗ 不可      | ⚠ 劣化    | ✗ 不可       | ✓ テキスト     |

### 7.9 Forget セマンティクス契約

全バックエンドが守るべき **不変条件**:

1. **Soft Forget**: 論理削除。`recall_scoped()` はマーク済みエントリを返さない。ストレージは残存。
2. **Hard Forget**: 物理削除。コンテンツ、エンベディング、イベントログから完全に除去。
3. **Tombstone**: 永続的な削除マーカー。将来の同一スロットへの書き込みをブロック。

**バックエンド別の制約**:

- **SQLite**: 3モード全て完全サポート。FTS5 インデックスもトリガーで同期削除。
- **LanceDB**: Soft/Tombstone はマーカー文字列での代替実装（劣化セマンティクス）。エンベディングベクトルは残存するため、ベクトル検索でヒットする可能性がある。
- **Markdown**: 追記専用。Hard 削除不可。Soft/Tombstone はストライクスルーマーカーで対応。

> **注意**: Forget 操作はエンベディングキャッシュ、コンパクションサマリー、コンソリデーション出力には波及しない。コンプライアンス要件がある場合、派生データのパージも別途実装が必要。

---

## 8. LLMプロバイダシステム

### 8.1 プロバイダアーキテクチャ

**ファイル**: `src/core/providers/`

```
create_provider(name)
    ├─ プライマリ: Anthropic, OpenAI, Gemini, Ollama, OpenRouter
    ├─ OpenAI互換: 20+ プロバイダ (同一 OpenAiCompatibleProvider)
    ├─ カスタム: custom:URL, anthropic-custom:URL
    └─ 不明: エラー

create_resilient_provider(name, reliability)
    └─ ReliableProvider { providers: Vec<(name, Box<dyn Provider>)>, retries, backoff }

create_provider_with_oauth_recovery(config, name)
    └─ OAuthRecoveryProvider { initial, recover_fn, rebuild_fn }
```

### 8.2 レスポンス型

**ProviderResponse**:

```rust
pub struct ProviderResponse {
    pub text: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub model: Option<String>,
    pub content_blocks: Vec<ContentBlock>,
    pub stop_reason: Option<StopReason>,
}
```

**ContentBlock**:

```rust
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
    Image { source: ImageSource },
}
```

**StopReason**: `EndTurn`, `ToolUse`, `MaxTokens`, `Error`

### 8.3 ストリーミング

**StreamEvent**:

```rust
pub enum StreamEvent {
    ResponseStart { model: Option<String> },
    TextDelta { text: String },
    ToolCallDelta { index: usize, id: Option<String>, name: Option<String>, input_json_delta: String },
    ToolCallComplete { index: usize, id: String, name: String, input: Value },
    Done { stop_reason: Option<StopReason>, input_tokens: Option<u64>, output_tokens: Option<u64> },
}
```

**StreamSink trait**: `CliStreamSink` (ターミナル出力), `ChannelStreamSink` (チャネル出力), `NullStreamSink` (破棄)

### 8.4 シークレットスクラビング

**ファイル**: `src/core/providers/scrub.rs`

検出パターン (25+):
`sk-`, `xoxb-`, `xoxp-`, `xoxs-`, `xoxa-`, `xapp-`, `ghp_`, `github_pat_`, `hf_`, `glpat-`, `ya29.`, `AIza`, `Authorization: Bearer`, `api_key=`, `access_token=`, `refresh_token=`, `id_token=`, JSON バリアント等

全マッチを `[REDACTED]` に置換。ミドルウェア経由で全 LLM I/O に適用。

### 8.5 ReliableProvider

**ファイル**: `src/core/providers/reliable.rs`

```rust
pub struct ReliableProvider {
    providers: Vec<(String, Box<dyn Provider>)>,  // (name, provider)
    retries: u32,
    backoff_ms: u64,
}
```

プライマリ → フォールバック1 → フォールバック2 の順で試行。各プロバイダに対し最大 `retries` 回リトライ（指数バックオフ）。

> **リトライ増幅防止**: Provider レベルのリトライと AgentLoop レベルの `RateLimited` ハンドリングが多重化しないよう注意。障害時に Provider が指数バックオフでリトライしている間、ToolLoop は `Error` ストップで上位に返す。ToolLoop が独自にリトライすることはない。

---

## 9. ツールシステム

### 9.1 ツール一覧

| ツール              | ファイル                     | 説明                                        |
| ------------------- | ---------------------------- | ------------------------------------------- |
| `shell`             | `tools/shell.rs`             | ターミナルコマンド実行                      |
| `file_read`         | `tools/file_read.rs`         | ファイル読み取り                            |
| `file_write`        | `tools/file_write.rs`        | ファイル書き込み                            |
| `memory_store`      | `tools/memory_store.rs`      | メモリへの保存                              |
| `memory_recall`     | `tools/memory_recall.rs`     | メモリ検索                                  |
| `memory_forget`     | `tools/memory_forget.rs`     | メモリ削除                                  |
| `memory_governance` | `tools/memory_governance.rs` | メモリライフサイクル管理                    |
| `browser_open`      | `tools/browser_open.rs`      | 許可された HTTPS URL を開く                 |
| `browser`           | `tools/browser/`             | フルブラウザ自動化                          |
| `composio`          | `tools/composio.rs`          | 1000+ アプリ統合 (Gmail, Notion, GitHub 等) |
| MCP ツール群        | `plugins/mcp/`               | Model Context Protocol ツール               |

### 9.2 ToolRegistry

**ファイル**: `src/core/tools/registry.rs`

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    middleware: Vec<Arc<dyn ToolMiddleware>>,
}
```

メソッド:

- `register(tool)` — ツール登録
- `unregister(name)` — ツール登録解除
- `get(name)` — ツール取得
 `tool_names()` — 登録済みツール名一覧取得
- `specs()` — 全ツール仕様取得
- `specs_for_context(ctx)` — コンテキスト依存のツール仕様取得 (allowed_tools フィルタ)
- `execute(name, args, ctx)` — ミドルウェアチェーン経由のツール実行

### 9.3 ミドルウェアチェーン

```rust
#[async_trait]
pub trait ToolMiddleware: Send + Sync + std::fmt::Debug {
    async fn before_execute(
        &self, tool_name: &str, args: &Value, ctx: &ExecutionContext,
    ) -> Result<MiddlewareDecision>;

    async fn after_execute(
        &self, tool_name: &str, result: &mut ToolResult, ctx: &ExecutionContext,
    );
}

pub enum MiddlewareDecision {
    Continue,
    Block(String),
    RequireApproval(ActionIntent),
}
```

**デフォルトミドルウェアスタック** (実行順):

1. `SecurityMiddleware` — 自律性レベル、ツール許可リスト、コマンド/パスポリシーチェック
2. `EntityRateLimitMiddleware` — エンティティごとのアクション上限
3. `AuditMiddleware` — 全ツール実行のログ記録
4. `OutputSizeLimitMiddleware` — 出力サイズ上限の適用
5. `ToolResultSanitizationMiddleware` — 外部コンテンツのマーカーラップ
6. `SecretScrubMiddleware` — API キー/トークンの除去

### 9.4 ActionIntent / ActionOperator

外部アクション実行のフレームワーク:

```rust
pub struct ActionIntent {
    pub intent_id: String,      // UUID v4
    pub action_kind: String,    // 例: "shell"
    pub operator: String,       // 例: "discord:user-1"
    pub payload: Value,
    pub requested_at: String,   // RFC3339
}

#[async_trait]
pub trait ActionOperator: Send + Sync {
    fn name(&self) -> &str;
    async fn apply(&self, intent: &ActionIntent, verdict: Option<&ActionPolicyVerdict>) -> Result<ActionResult>;
}
```

`NoopOperator` がデフォルト — アクションを実行せず、監査ログ (`action_intents/YYYY-MM-DD.jsonl`) に記録。

---

## 10. トランスポート層

### 10.1 チャネルシステム

#### ChannelMessage 型

```rust
pub struct ChannelMessage {
    pub id: String,
    pub sender: String,              // ユーザー ID (例: Discord user ID)
    pub content: String,
    pub channel: String,             // チャネル名
    pub conversation_id: Option<String>,  // 会話コンテキスト
    pub thread_id: Option<String>,
    pub reply_to: Option<String>,
    pub message_id: Option<String>,
    pub timestamp: u64,
    pub attachments: Vec<MediaAttachment>,
}
```

#### チャネル実装一覧

| チャネル | ファイル           | 接続方式              | 特徴                             |
| -------- | ------------------ | --------------------- | -------------------------------- |
| CLI      | `cli.rs`           | stdin/stdout          | 常時利用可能、依存なし           |
| Telegram | `telegram/`        | Bot API long-poll     | メディア対応、ユーザー許可リスト |
| Discord  | `discord/`         | WebSocket + HTTP API  | スラッシュコマンド、スレッド対応 |
| Slack    | `slack.rs`         | RTM API + webhook     | リアルタイムメッセージ           |
| Matrix   | `matrix.rs`        | Homeserver federation | ルームベースメッセージング       |
| WhatsApp | `whatsapp/`        | Cloud API webhooks    | 署名検証付き                     |
| Email    | `email_channel.rs` | IMAP/SMTP             | 添付ファイル対応 (feature-gated) |
| IRC      | `irc/`             | RFC 1459              | SASL/NickServ 認証、TLS 対応     |
| iMessage | `imessage/`        | macOS 統合            | プラットフォーム固有             |

#### メッセージフロー

**インバウンド** (チャネル → エージェント):

1. チャネルの `listen()` が監視付きリスナーを起動 (`runtime.rs`)
2. リスナーが `ChannelMessage` を受信 → mpsc チャネル経由で送信
3. `startup.rs` が各メッセージのハンドラタスクをスポーン
4. `message_handler.rs` で処理:
   - 外部 ingress ポリシーの適用（安全性チェック）
   - メモリへの自動保存（有効時）
   - チャネル固有の自律性レベル + ツール許可リスト解決
   - ツールループ実行
   - タイピングインジケーター表示

**アウトバウンド** (エージェント → チャネル):

1. ツールループが `ToolLoopResult` を返す
2. `message_handler.rs` が `reply_to_origin()` で適切なチャネルを特定
3. `send_chunked()` が `max_message_length()` に基づいてメッセージを分割
4. 各チャンクを `send()` で送信
5. メディア添付を `send_media()` で送信（対応チャネルのみ）

#### チャネルポリシー

```rust
pub struct ChannelPolicy {
    pub autonomy_level: Option<AutonomyLevel>,
    pub tool_allowlist: Option<HashSet<String>>,
}
```

チャネルごとに自律性レベルとツール許可リストを設定可能。

### 10.2 HTTP ゲートウェイ

**ファイル**: `src/transport/gateway/`

#### ルーティング

| メソッド | パス                   | 認証                           | 説明                        |
| -------- | ---------------------- | ------------------------------ | --------------------------- |
| GET      | `/health`              | なし                           | ヘルスチェック              |
| POST     | `/pair`                | なし                           | ペアリングコード交換        |
| POST     | `/webhook`             | Bearer Token or Webhook Secret | 汎用 webhook ingress        |
| GET      | `/ws`                  | Bearer Token                   | WebSocket アップグレード    |
| POST     | `/v1/chat/completions` | API Key                        | OpenAI 互換 API             |
| GET      | `/whatsapp`            | Meta Verify Token              | WhatsApp webhook 検証       |
| POST     | `/whatsapp`            | 署名検証                       | WhatsApp メッセージ ingress |

#### AppState

```rust
pub struct AppState {
    pub provider: Arc<dyn Provider>,
    pub registry: Arc<ToolRegistry>,
    pub rate_limiter: Arc<EntityRateLimiter>,
    pub max_tool_loop_iterations: u32,
    pub permission_store: Arc<PermissionStore>,
    pub model: String,
    pub temperature: f64,
    pub openai_compat_api_keys: Option<Vec<String>>,
    pub mem: Arc<dyn Memory>,
    pub auto_save: bool,
    pub webhook_secret: Option<Arc<str>>,
    pub pairing: Arc<PairingGuard>,
    pub whatsapp: Option<Arc<WhatsAppChannel>>,
    pub whatsapp_app_secret: Option<Arc<str>>,
    pub defense_mode: GatewayDefenseMode,
    pub defense_kill_switch: bool,
    pub security: Arc<SecurityPolicy>,
    pub replay_guard: Arc<ReplayGuard>,
}
```

#### OpenAI 互換レイヤー

`POST /v1/chat/completions` で OpenAI API 互換エンドポイントを提供:

- API キー認証 (`openai_compat_auth.rs`)
- ChatCompletion リクエスト/レスポンス型 (`openai_compat_types.rs`)
- SSE ストリーミングレスポンス (`openai_compat_streaming.rs`)

#### セキュリティレイヤー

1. **ペアリング** — ワンタイムコード → Bearer トークン (SHA-256 ハッシュ化、永続化)
2. **Webhook Secret** — オプションの `X-Webhook-Secret` ヘッダー
3. **外部 Ingress ポリシー** — 高リスクコンテンツ (URL、コード等) のブロック
4. **リプレイガード** — Webhook 重複処理の防止
5. **レート制限** — エンティティごとのアクション上限
6. **Defense Modes** — Audit (ログのみ), Warn (受理+警告), Enforce (拒否)

### 10.3 デーモン/スーパーバイザ

**ファイル**: `src/platform/daemon/supervisor.rs`

4つの監視対象コンポーネント:

1. **Gateway** — HTTP サーバ
2. **Channels** — 全チャネルリスナー
3. **Heartbeat** — 定期ヘルスチェック
4. **Scheduler** — Cron ジョブランナー

各コンポーネント:

- 障害時に指数バックオフで再起動 (初期 2s、最大 60s)
- 最大 10 回の再起動（サーキットブレーカー）
- 診断にヘルス状態を報告

---

## 11. セキュリティシステム

### 11.1 SecurityPolicy (Deny-by-Default)

**ファイル**: `src/security/policy/`

```rust
pub enum AutonomyLevel {
    ReadOnly,    // 読み取り専用（ツール実行不可）
    Supervised,  // 承認が必要（デフォルト）
    Full,        // 制限内で自律実行
}
```

**コマンド Allowlist** (`command.rs`):

- 許可コマンド: git, npm, cargo, ls, cat, grep, find, echo, pwd, wc, head, tail
- git インジェクション防止: `core.sshcommand`, 資格情報窃取をブロック
- ネットワークエグレス防止: push, send-email をブロック
- 環境変数操作の検出

**パス Allowlist** (`path.rs`):

 禁止パス: `/etc`, `/root`, `/home`, `/usr`, `/bin`, `/sbin`, `/lib`, `/opt`, `/boot`, `/dev`, `/proc`, `/sys`, `/var`, `/tmp`, `~/.ssh`, `~/.gnupg`, `~/.aws`, `~/.config`
- パストラバーサル防止: `..`, symlink エスケープ, URL エンコード (`%2f`)
- ワークスペース境界の強制

**レート制限** (`trackers.rs`):

- `ActionTracker`: `max_actions_per_hour` (デフォルト: 20)
- `CostTracker`: `max_cost_per_day_cents` (デフォルト: 500)

### 11.2 暗号化ボールト (ChaCha20-Poly1305)

**ファイル**: `src/security/secrets.rs`

```
キーファイル: ~/.asteroniris/.secret_key (0600 パーミッション)
暗号文フォーマット: enc2:<hex(12byte_nonce ‖ ciphertext ‖ 16byte_tag)>
```

- ChaCha20-Poly1305 AEAD (ランダム 12byte nonce)
- レガシー XOR 暗号 (`enc:`) からの自動マイグレーション
- `zeroize` でキーバッファをドロップ時にゼロ化
- `secrets.encrypt = false` でプレーンテキストフォールバック

### 11.3 書き戻しガード (Writeback Guard)

**ファイル**: `src/security/writeback_guard/`

ペルソナの自己腐敗を防止:

**サイズ制限** (`constants.rs`):
| フィールド | 上限 |
|-----------|------|
| current_objective | 280 文字 |
| recent_context_summary | 1,200 文字 |
| memory items | 8 個 |
| self-tasks | 5 個 |
| self-task 有効期限 | 72 時間 |

**ポイズンパターン検出**: "ignore previous instructions", "system prompt", "override safety", "exfiltrate" 等のジェイルブレイクキーワード

**不変フィールド**: `schema_version`, `identity_principles_hash`, `safety_posture` は変更不可

**検証フロー**: サイズチェック → フィールド許可リスト → ポイズンパターン検出 → RFC3339 タイムスタンプ検証

### 11.4 ペアリングセキュリティ

**ファイル**: `src/security/pairing.rs`

1. 起動時に 6 桁の数値ペアリングコードを生成（インタラクティブ確認用のみ）
2. 確認後、ランダムな高エントロピー Bearer トークンを発行
3. Bearer トークンは SHA-256 ハッシュとして保存（64 文字 hex）
4. ブルートフォース保護: 5 回の失敗 → 300 秒ロックアウト
5. 定時比較（タイミング攻撃防止）
6. 公開バインド検出: localhost 以外はトンネルまたは `allow_public_bind=true` が必要

> **セキュリティ注意**: 6 桁コードはインタラクティブ確認専用。Bearer トークンは 6 桁コードから派生されず、別途高エントロピーで生成される。トークンローテーションが必要な場合はペアリングを再実行。

### 11.5 承認システム

**ファイル**: `src/security/approval.rs`

```rust
#[async_trait]
pub trait ApprovalBroker: Send + Sync {
    async fn request_approval(&self, request: &ApprovalRequest) -> Result<ApprovalDecision>;
}

pub enum ApprovalDecision {
    Approved,
    ApprovedWithGrant(PermissionGrant),
    Denied { reason: String },
}

pub enum RiskLevel { Low, Medium, High }
```

実装: `CliApprovalBroker`, `TelegramApprovalBroker`, `DiscordApprovalBroker`, `TextReplyApprovalBroker`

**PermissionStore** (`permissions.rs`):

- グラント管理: `GrantScope::Session` or `GrantScope::Permanent`
- 一度承認されたツール + パターンの組み合わせは再承認不要

### 11.6 SSRF 防止

**ファイル**: `src/security/url_validation.rs`

- `is_private_ip(ip)` — プライベート IP 範囲の検出
- `is_private_host(host)` — プライベートホスト名の検出
- `validate_url_not_ssrf(url)` — URL の SSRF バリデーション

---

## 12. プランナーシステム

**ファイル**: `src/core/planner/`

### データ構造

```rust
pub struct Plan {
    pub id: String,
    pub description: String,
    pub steps: Vec<PlanStep>,
    pub dag: DagContract,
}

pub struct PlanStep {
    pub id: String,
    pub description: String,
    pub action: StepAction,
    pub status: StepStatus,
    pub depends_on: Vec<String>,
    pub output: Option<String>,
    pub error: Option<String>,
}

pub enum StepAction {
    ToolCall { tool_name: String, args: Value },
    Prompt { text: String },
    Checkpoint { label: String },
}

pub enum StepStatus { Pending, Running, Completed, Failed, Skipped }
```

### DAG コントラクト (`dag_contract.rs`)

```rust
pub struct DagContract {
    pub nodes: Vec<DagNode>,
    pub edges: Vec<DagEdge>,
}
```

- `validate()` — サイクル検出
- `topological_sort()` — 実行順序の決定

### PlanExecutor (`executor.rs`)

```rust
impl PlanExecutor {
    pub async fn execute(plan: &mut Plan, runner: &dyn StepRunner) -> Result<ExecutionReport>;
}
```

実行フロー:

1. DAG トポロジカルソートで実行順序を取得
2. 各ステップを順次実行
3. ステップ失敗時: 下流ステップを全て `Skipped` にマーク (`mark_downstream_skipped`)
4. 独立ブランチは失敗の影響を受けずに続行

### StepRunner trait

```rust
#[async_trait]
pub trait StepRunner: Send + Sync {
    async fn run_step(&self, step: &PlanStep) -> Result<StepOutput>;
}
```

`ToolStepRunner` がデフォルト実装: `ToolRegistry` 経由でツールを実行。

---

## 13. セッション管理

**ファイル**: `src/core/sessions/`

### SessionStore trait

```rust
pub trait SessionStore {
    fn create_session(&self, channel: &str, user_id: &str) -> Result<Session>;
    fn get_session(&self, id: &str) -> Result<Option<Session>>;
    fn get_or_create_session(&self, channel: &str, user_id: &str) -> Result<Session>;
    fn list_sessions(&self, channel: Option<&str>) -> Result<Vec<Session>>;
    fn delete_session(&self, id: &str) -> Result<bool>;
    fn update_session_state(&self, id: &str, state: SessionState) -> Result<()>;
    fn append_message(&self, session_id: &str, role: MessageRole, content: &str,
                      input_tokens: Option<u64>, output_tokens: Option<u64>) -> Result<ChatMessage>;
    fn get_messages(&self, session_id: &str, limit: Option<usize>) -> Result<Vec<ChatMessage>>;
    fn count_messages(&self, session_id: &str) -> Result<usize>;
    fn delete_messages_before(&self, session_id: &str, before_id: &str) -> Result<usize>;
}
```

### データ構造

```rust
pub struct Session {
    pub id: String,           // UUID v4
    pub channel: String,
    pub user_id: String,
    pub state: SessionState,  // Active / Archived / Compacted
    pub model: Option<String>,
    pub metadata: Option<Value>,
    pub created_at: String,   // RFC3339
    pub updated_at: String,
}

pub struct ChatMessage {
    pub id: String,
    pub session_id: String,
    pub role: MessageRole,    // User / Assistant / System
    pub content: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub created_at: String,
}

pub struct SessionConfig {
    pub enabled: bool,               // デフォルト: true
    pub max_history: usize,          // デフォルト: 100
    pub compaction_threshold: usize, // デフォルト: 50
}
```

### SqliteSessionStore

SQLite ベースの実装:

- `sessions` テーブル: UNIQUE(channel, user_id, state)
- `chat_messages` テーブル: session_id FK、created_at インデックス
- `get_or_create_session()` — 既存の active セッションを返すか新規作成
- `get_messages(limit)` — 最新 N 件を時系列で返す

### コンパクション (`compaction.rs`)

長いセッションのメッセージプルーニング:

- `compaction_threshold` (デフォルト 50) を超えるとトリガー
- 古いメッセージを削除し、セッション状態を `Compacted` に更新

---

## 14. ペルソナシステム

**ファイル**: `src/core/persona/`

### StateHeader

ペルソナの状態を JSON で管理:

| フィールド                 | 可変性 | 説明                                       |
| -------------------------- | ------ | ------------------------------------------ |
| `schema_version`           | 不変   | スキーマバージョン                         |
| `identity_principles_hash` | 不変   | コアアイデンティティのハッシュ             |
| `safety_posture`           | 不変   | 安全姿勢                                   |
| `current_objective`        | 可変   | 現在の目標 (max 280 文字)                  |
| `open_loops`               | 可変   | 未解決タスク                               |
| `next_actions`             | 可変   | 計画されたステップ                         |
| `commitments`              | 可変   | 約束事項                                   |
| `recent_context_summary`   | 可変   | 最近のインタラクション要約 (max 1200 文字) |
| `last_updated_at`          | 可変   | RFC3339 タイムスタンプ                     |

### リフレクションフロー

1. **正規状態の読み込み**: メモリからペルソナ状態ヘッダーを取得
2. **リフレクトメッセージの構築**: 正規状態 + ユーザーメッセージ + アシスタント応答
3. **リフレクションプロバイダの呼び出し**: 決定論的システムプロンプトで送信
4. **ペイロードのパース**: 更新された state_header と memory_append を JSON から抽出
5. **書き戻しガードで検証**: 不変フィールドの変更チェック、サイズ制限、ポイズンパターン
6. **状態の永続化**: 検証済み状態をメモリバックエンドに保存

---

## 15. 評価システム

**ファイル**: `src/core/eval/`

### EvalHarness

決定論的評価ランナー:

- 固定シードによる再現可能な結果生成
- ベースラインスイートのデフォルト提供 (`default_baseline_suites()`)
- シード変更時の警告検出 (`detect_seed_change_warning()`)
- エビデンスファイルの書き出し (`write_evidence_files()`)

### データ構造

```rust
pub struct EvalSuiteSpec {
    pub name: &'static str,
    pub scenarios: Vec<EvalScenarioSpec>,
}

pub struct EvalScenarioSpec {
    pub id: &'static str,
    pub success_target_percent: u8,
    pub min_cost_cents: u32,
    pub max_cost_cents: u32,
    pub min_latency_ms: u32,
    pub max_latency_ms: u32,
    pub retry_cap: u32,
}

pub struct EvalReport {
    pub seed: u64,
    pub suites: Vec<EvalSuiteSummary>,
    pub summary_fingerprint: u64,
}

pub struct EvalSuiteSummary {
    pub suite: String,
    pub case_count: u32,
    pub success_rate_bps: u32,     // 基準点 (bps)
    pub avg_cost_cents: u32,
    pub avg_latency_ms: u32,
    pub avg_retries_milli: u32,    // ミリ単位
}
```

**出力フォーマット**: CSV (`render_csv()`) およびテキスト (`render_text_summary()`)

---

## 16. プラグインシステム

### 16.1 SkillForge

**ファイル**: `src/plugins/skillforge/`

パイプライン: **Scout → Gate → Evaluate → Integrate**

1. **Scout** (`scout.rs`): 外部ソースからのスキル発見
2. **Gate** (`gate.rs`): 4 層セキュリティゲート (`GateInput` → `GateVerdict`)
3. **Evaluate** (`evaluate.rs`): 適格候補のスコアリング
4. **Integrate** (`integrate.rs`): マニフェスト生成

**SkillTier** (`tiers.rs`): スキルの品質分類

**ReasonCode** (`patterns.rs`): 拒否理由コード

**SkillOverrides** (`overrides.rs`): 手動オーバーライド設定

**SkillForgeConfig** (`config.rs`): パイプライン設定

### 16.2 Skills

**ファイル**: `src/plugins/skills/`

- `loader.rs` — スキルの読み込みと管理
- Symlink ベースのスキルインストール

### 16.3 MCP (Model Context Protocol)

**ファイル**: `src/plugins/mcp/` (feature-gated: `rmcp`)

- `client_manager.rs` — `create_mcp_tools()` ファクトリ
- `client_proxy_tool.rs` — `McpToolProxy`: MCP ツールをネイティブ `Tool` trait オブジェクトとしてブリッジ
- `client_connection.rs` — MCP サーバ接続管理
- `bridge.rs` — MCP ブリッジ
- `content.rs` — コンテンツ変換ユーティリティ
- `server/` — MCP サーバ実装

### 16.4 Integrations

**ファイル**: `src/plugins/integrations/`

- `registry.rs` — 統合レジストリ
- `inventory.rs` — `build_scope_lock_inventory()`: スコープロックインベントリ構築
- `integration_capability_matrix.json` — 統合ケイパビリティマトリクス
- `inventory_scope_lock.json` — インベントリスコープロック

---

## 17. ランタイムシステム

### 17.1 環境アダプタ

**ファイル**: `src/runtime/environment/`

```rust
pub trait RuntimeAdapter: Send + Sync {
    fn name(&self) -> &str;
    fn has_shell_access(&self) -> bool;
    fn has_filesystem_access(&self) -> bool;
    fn storage_path(&self) -> &Path;
    fn supports_long_running(&self) -> bool;
    fn memory_budget(&self) -> Option<usize>;
}
```

実装: `NativeRuntime` (通常環境), `DockerRuntime` (コンテナ環境)

### 17.2 トンネル

**ファイル**: `src/runtime/tunnel/`

```rust
#[async_trait]
pub trait Tunnel: Send + Sync {
    fn name(&self) -> &str;
    async fn start(&self) -> Result<()>;
    async fn stop(&self) -> Result<()>;
    async fn health_check(&self) -> bool;
    fn public_url(&self) -> Option<String>;
}
```

| 実装               | 説明              |
| ------------------ | ----------------- |
| `CloudflareTunnel` | Cloudflare Tunnel |
| `TailscaleTunnel`  | Tailscale         |
| `NgrokTunnel`      | Ngrok             |
| `CustomTunnel`     | カスタムコマンド  |
| `NoneTunnel`       | No-op             |

### 17.3 可観測性

**ファイル**: `src/runtime/observability/`

```rust
pub trait Observer: Send + Sync {
    fn record_event(&self, event: &str, metadata: &Value);
    fn record_metric(&self, name: &str, value: f64);
    fn record_autonomy_lifecycle(&self, event: &str, metadata: &Value);
    fn record_memory_lifecycle(&self, event: &str, metadata: &Value);
}
```

実装: `LogObserver`, `OtelObserver`, `PrometheusObserver`, `NoopObserver`

### 17.4 使用量追跡

**ファイル**: `src/runtime/usage/tracker.rs`

`UsageTracker` trait + `SqliteUsageTracker` 実装によるトークン使用量・コスト追跡。

### 17.5 診断

**ファイル**: `src/runtime/diagnostics/`

- `doctor/` — `asteroniris doctor` コマンドのシステム診断チェック
- `heartbeat/` — デーモンモードでの定期ヘルスチェック

---

## 18. 設定システム

### 18.1 Config 構造

**ファイル**: `src/config/schema/core/types.rs`

```toml
# ~/.asteroniris/config.toml

api_key = "enc2:..."            # 暗号化された API キー
provider = "openrouter"
model = "anthropic/claude-3.5-sonnet"
temperature = 0.7

[memory]
backend = "sqlite"              # sqlite | lancedb | markdown | none
embedding_provider = "openai"
embedding_model = "text-embedding-3-small"
embedding_dimensions = 1536
vector_weight = 0.7
keyword_weight = 0.3
embedding_cache_size = 10000
auto_save = true

[gateway]
port = 3000
host = "127.0.0.1"
require_pairing = true
allow_public_bind = false
defense_mode = "enforce"        # audit | warn | enforce
cors_origins = []

[channels.telegram]
bot_token = "..."
allowed_users = ["123456"]
autonomy_level = "supervised"

[channels.discord]
bot_token = "..."
guild_id = "..."
autonomy_level = "supervised"

[autonomy]
level = "supervised"            # read_only | supervised | full
external_action_execution = "disabled"
workspace_only = true
allowed_commands = ["git", "npm", "cargo", "ls", "cat"]
forbidden_paths = ["/etc", "/root"]
max_actions_per_hour = 20
max_cost_per_day_cents = 500

[reliability]
fallback_providers = ["openai"]
provider_retries = 3
provider_backoff_ms = 1000

[observability]
backend = "log"                 # none | log

[secrets]
encrypt = true
```

### 18.2 環境変数オーバーライド

**ファイル**: `src/config/schema/core/env_overrides.rs`

| 環境変数                            | 対象                       |
| ----------------------------------- | -------------------------- |
| `ASTERONIRIS_API_KEY` / `API_KEY`   | LLM API キー               |
| `ASTERONIRIS_PROVIDER` / `PROVIDER` | プロバイダ名               |
| `ASTERONIRIS_MODEL`                 | モデル識別子               |
| `ASTERONIRIS_WORKSPACE`             | ワークスペースディレクトリ |
| `ASTERONIRIS_GATEWAY_HOST` / `HOST` | ゲートウェイバインドホスト |
| `ASTERONIRIS_GATEWAY_PORT` / `PORT` | ゲートウェイバインドポート |
| `ASTERONIRIS_TEMPERATURE`           | サンプリング温度 (0.0–2.0) |

**優先順序**: 環境変数 > config.toml > デフォルト値

---

## 19. テスト構造

### インテグレーションテスト

6 つのインテグレーションテストバイナリ (`tests/`):

| ファイル     | テスト対象                                     |
| ------------ | ---------------------------------------------- |
| `memory.rs`  | メモリバックエンド (SQLite, LanceDB, Markdown) |
| `gateway.rs` | HTTP ゲートウェイ                              |
| `agent.rs`   | エージェント会話ループ                         |
| `persona.rs` | ペルソナシステム                               |
| `runtime.rs` | ランタイムシステム                             |
| `project.rs` | プロジェクト全体                               |

**重要**: インテグレーションテストは明示的な `#[path]` を使用:

```rust
// tests/memory.rs — 正しいパターン
#[path = "support/memory_harness.rs"]
mod memory_harness;
#[path = "memory/comparison.rs"]
mod comparison;
```

### ユニットテスト

- 各ファイル末尾に `#[cfg(test)] mod tests`
- `tokio::test` で非同期テスト
- `tempfile::TempDir` でファイルシステム分離
- テスト内では `unwrap()` / `expect()` 使用可

### テスト実行

```bash
cargo test                                # 全テスト
cargo test-dev                            # 4スレッド並列
cargo test-dev-tests                      # インテグレーションのみ
cargo test --test memory -- comparison    # 名前サブストリングでフィルタ
BACKEND=sqlite cargo test --test memory   # バックエンド指定
```

### CI カバレッジ

- 40% ライン閾値
- スキップ: `inventory_scope_lock::inventory_scope_lock`

---

## 20. ビルドとデプロイ

### ビルドプロファイル

| プロファイル | opt-level        | LTO  | codegen-units | strip | panic  |
| ------------ | ---------------- | ---- | ------------- | ----- | ------ |
| dev          | デフォルト       | なし | 256           | なし  | unwind |
| test         | 0                | なし | 256           | なし  | unwind |
| release      | "z" (サイズ優先) | true | 1             | true  | abort  |
| dist         | "z"              | fat  | 1             | true  | abort  |

### Cargo エイリアス (`.cargo/config.toml`)

```bash
cargo build-fast         # 機能セット縮小ビルド
cargo build-minimal      # bundled-sqlite のみ
cargo test-fast          # 機能セット縮小テスト
cargo check-all          # 全機能チェック
cargo coverage           # llvm-cov HTML レポート
```

### Pre-push フック

`.githooks/pre-push` が `fmt` + `clippy` + `test` を強制:

```bash
git config core.hooksPath .githooks
```

### 依存ポリシー

- 新しい依存は正当化が必要
- 機能を最小化、デフォルトを無効化
- 許可ライセンス: MIT, Apache-2.0, BSD-2/3-Clause, ISC, MPL-2.0, Zlib, BSL-1.0, 0BSD, CC0-1.0

### Docker

```bash
docker build -t asteroniris .
```

`Dockerfile` + `docker-compose.yml` を提供。

---

## 21. 拡張ガイド

### 新しいチャネルの追加

1. `src/transport/channels/<name>.rs` (または `<name>/mod.rs`) に `Channel` trait を実装
2. `src/transport/channels/factory.rs` の `build_channels()` に追加
3. `src/transport/channels/mod.rs` で `pub mod` + `pub use` re-export
4. `src/config/schema/channels.rs` に設定構造体を追加
5. `mod.rs` をシンファサードとして維持（ロジックはサブモジュールに）

### 新しいプロバイダの追加

1. `src/core/providers/<name>.rs` に `Provider` trait を実装
2. `src/core/providers/factory.rs` の `create_provider()` match に追加
3. `src/core/providers/mod.rs` で `pub mod` re-export
4. API キー解決: `resolve_api_key()` にプロバイダ固有環境変数を追加

### 新しいツールの追加

1. `src/core/tools/<name>.rs` に `Tool` trait を実装
2. `src/core/tools/factory.rs` の `all_tools()` に追加
3. `tool_descriptions()` にツール説明を追加
4. `src/core/tools/mod.rs` で `pub use` re-export

### 新しいメモリバックエンドの追加

1. `src/core/memory/<name>/` ディレクトリに `Memory` trait を実装
2. `src/core/memory/factory.rs` の `create_memory()` match に追加
3. `src/core/memory/capability.rs` に能力マトリクスを追加
4. `src/core/memory/mod.rs` で re-export

### 新しいトンネルの追加

1. `src/runtime/tunnel/<name>.rs` に `Tunnel` trait を実装
2. `src/runtime/tunnel/factory.rs` の `create_tunnel()` に追加
3. `src/config/schema/tunnel.rs` に設定を追加

---

## 22. システム不変条件と運用指針

### 22.1 並行性モデル

- **SQLite**: WAL モードで運用。読み取りは並行、書き込みはシリアライズ。`busy_timeout` でロック競合を処理。
- **SessionStore**: 同期トレイト（`#[async_trait]` なし）。Tokio ランタイム上では `spawn_blocking` 経由で呼び出される。マルチチャネル負荷下ではテールレイテンシのボトルネックになりうる点に注意。将来的に非同期化を検討。
- **Memory トレイト**: `#[async_trait]` で非同期。内部で SQLite へのアクセスは `spawn_blocking` + `Mutex` で保護。
- **ツールループ**: シングルスレッドで順次実行（ツール呼び出しごとに LLM 応答を待機）。並列ツール実行は未サポート。
- **デーモン**: Gateway + Channels + Heartbeat + Scheduler を `tokio::select!` で並行監視。各コンポーネントは独立したタスク。

### 22.2 マイグレーション戦略

- **SQLite スキーマ**: `schema.rs` で起動時に `CREATE TABLE IF NOT EXISTS` を実行。バージョニングやインクリメンタルマイグレーションは未実装。スキーマ変更時はリインデックスプロセス（テンポラリ DB → データシード → アトミックスワップ）を使用。
- **ボールト**: レガシー XOR 暗号 (`enc:`) から ChaCha20-Poly1305 (`enc2:`) への自動マイグレーションをサポート。
- **Config**: TOML スキーマは `#[serde(default)]` で後方互換性を維持。新フィールド追加時はデフォルト値必須。

### 22.3 セキュリティ不変条件

**全サブシステムが守るべき制約**:

1. **パス正規化**: 全ファイルアクセスは正規化済み絶対パスで検証。`..` トラバーサル、symlink エスケープ、URL エンコード (`%2f`) を検出・拒否。
2. **シェルメタキャラクタ**: コマンド実行前にインタプリターインジェクションを防止。allowlist にないコマンドは実行不可。
3. **SSRF 防止**: `validate_url_not_ssrf()` でプライベート IP 範囲、ローカルホスト名、DNS リバインディングを検出。
4. **シークレットスクラビング**: 全 LLM 入出力で 25+ パターンを検出し `[REDACTED]` に置換。ツール結果もミドルウェア経由でスクラブ。
5. **監査ログ**: 全ツール実行が AuditMiddleware 経由で記録。アクションインテントは `action_intents/YYYY-MM-DD.jsonl` に永続化。

### 22.4 Session UNIQUE 制約について

`UNIQUE(channel, user_id, state)` により、同一 (channel, user_id) に対して各状態は最大1つ。これは意図的な設計:

- Active セッションは常に1つ（`get_or_create_session()` が保証）
- Compacted 時: 古いメッセージを削除し、同じセッションを Compacted 状態に更新（新規作成ではなく状態遷移）
- 履歴管理が必要な場合は、セッションをアーカイブしてから新規作成（delete → create のシーケンス）

> **制約**: 複数のアーカイブ済みセッションを保持する必要がある場合、UNIQUE 制約の変更が必要。現時点では「1ユーザー = 1アクティブセッション」のモデルを採用。

---

## 付録: 型の早見表

### ProviderMessage

```rust
pub struct ProviderMessage {
    pub role: MessageRole,       // User / Assistant / System
    pub content: Vec<ContentBlock>,
}
```

### ToolResult

```rust
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub attachments: Vec<OutputAttachment>,
}
```

### ToolSpec

```rust
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,  // JSON Schema
}
```

### OutputAttachment

```rust
pub struct OutputAttachment {
    pub mime_type: String,
    pub filename: Option<String>,
    pub path: Option<String>,      // ローカルファイルパス
    pub url: Option<String>,       // リモート URL
}
```

### ToolCallRecord

```rust
pub struct ToolCallRecord {
    pub tool_name: String,
    pub args: Value,
    pub result: ToolResult,
    pub iteration: u32,
}
```

### ToolLoopResult

```rust
pub struct ToolLoopResult {
    pub final_text: String,
    pub tool_calls: Vec<ToolCallRecord>,
    pub attachments: Vec<OutputAttachment>,
    pub iterations: u32,
    pub tokens_used: Option<u64>,
    pub stop_reason: LoopStopReason,
}
```
