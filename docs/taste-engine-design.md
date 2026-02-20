# General Taste Engine — 設計提案書

---

## §1 概要 / Overview

General Taste Engine は、Web/UI・映像/モーション・文章/テキスト・音/オーディオを横断して審美的品質を評価する、AsteronIris の新モジュールである。

核心的洞察は一つ：「感性」は単一のルールでは定義不可能。「このフォントが美しい」「このカットが気持ちいい」という判断は、ジャンル・文化・目的・受け手によって変わる。だから「普遍的に良い」を主張しなくて済む設計にする。代わりに、「普遍コア（Universal Critic）＋ 領域アダプタ（Domain Adapter）＋ 文脈条件（Context）」の合成モデルを現実解として採用する。

本提案の対象ドメインは4つ：

- **Web/UI**: レイアウト、コンポーネント設計、インタラクション品質
- **映像/モーション**: カット構成、イージング、テロップ、トランジション
- **文章/テキスト**: 論旨構造、レトリック、密度、文体の一貫性
- **音/オーディオ**: リズム、ダイナミクス、グルーヴ、間の設計

目的は「万能の審美」ではない。新ジャンルに投入されたとき、「上手い逸脱を短時間で掴む能力」を持つ meta-taste を仕様化することが本提案のゴールである。プロの仕事とテンプレートの差は「要素」ではなく「配分・順序・曲線・間」に宿る。そこを観測・評価・改善提案できるエンジンを構築する。

---

## §2 感性の形式的定義 / Formal Definition

### Core Formulation

感性を条件付き選好関数として定義する：

```
P(good | X, C)  または  D(X, C) — スコア分布
```

「普遍的に良い」を主張せず、「与えられた文脈の中で、人間が好む確率を最大化する」に落とす。これにより、文化・ジャンル・目的の違いを文脈変数 C に吸収できる。

### X — 入力：Artifact

評価対象の成果物。画像・動画・音・文章・インタラクション記録を統一的に扱うため、Rust enum `Artifact` で型化する：

- `Text { content, format }` — Plain / Markdown / HTML
- `Image { bytes, mime }` — PNG / JPEG / WebP / SVG
- `Audio { bytes, mime }` — WAV / MP3 / FLAC
- `Video { bytes, mime }` — MP4 / WebM
- `Interaction { events }` — UI操作トレース（hover / scroll / 遷移のタイムライン）

### C — 文脈：TasteContext

評価の文脈を規定するパラメータ群：

- **domain**: UI / Video / Text / Audio / General
- **genre**: "brutalist UI"、"lofi hiphop"、"literary fiction" 等の具体的ジャンル
- **purpose**: 訴求目的（説得、情報伝達、エンタメ、ブランディング等）
- **audience**: 想定受け手の属性
- **culture**: 文化的背景（地域・言語・慣習）
- **brand**: name + adjectives + banned（禁止表現リスト）
- **constraints**: 技術的・法的・予算的制約

### 出力：スコア分布

NIMA (Talebi & Milanfar, 2018) に倣い、単一スカラーではなく**スコア分布（10-bin histogram）**で返す。分布の平均 = score、分散 = 判断の割れ具合。polarizing な作品ほど高分散になる。Earth Mover's Distance (EMD) を損失関数に使うことで、順序構造を尊重した学習が可能になる。

---

## §3 アーキテクチャ / Architecture

三層構造 + 学習ストレージの4コンポーネント。各層の仕様を以下に記す。

---

### §3.1 Perception Layer（知覚層）

**役割**: マルチモーダル入力を同じ embedding 空間に写す。

**設計方針**: ImageBind (Girdhar et al., Meta FAIR 2023) のアプローチを採用する。画像を pivot modality として、各モダリティ（テキスト、オーディオ、映像、インタラクション）のエンコーダを画像 embedding 空間にアラインする。画像-テキスト間は CLIP (Radford et al., 2021)、画像-オーディオ間は CLAP (Elizalde et al., 2023) の事前学習を活用できる。

UniBind (Luo et al., 2024) の知見として、画像中心のアラインメントはモダリティ間で表現品質に偏りが生じる。これを解消するため、LLM生成テキスト記述を balanced anchor として使用し、均一な cross-modal alignment を実現する。

**入力正規化**: MUSIQ (Ke et al., Google 2022) の resolution-agnostic multi-scale tokenization を採用。固定サイズへのリサイズは構図・アスペクト比を破壊するため、hash-based 2D spatial embedding で任意解像度に対応する。

**映像特有の処理**: FAST-VQA (Wu et al., ECCV 2022) の Grid Mini-patch Sampling (GMS) を採用。ネイティブ解像度で小パッチをサンプリングし、テクスチャ品質とグローバル構造を両立させる。均一フレーム抽出は局所品質情報を破壊するため不採用。

モダリティごとの知覚特徴：

- **画像/動画**: 構図、階層、質感、動き、トランジション、視線誘導
- **音**: リズム、ダイナミクス、テンション変化、同期
- **文章**: 論理構造、レトリック、密度、読み心地、文体
- **インタラクション**: 時間軸上の挙動（hover / scroll / 遷移）、応答性

---

### §3.2 Universal Critic（普遍コア）

**役割**: ジャンル横断で効く7つの審美軸でスコア化する。

**設計**: DOVER (Wu et al., ICCV 2023) の two-branch architecture を採用。人間の品質判断は常に aesthetic（好み・共感）と technical（技術的品質）の混合であり、これを二系統に分離し、軸ごとに独立した inductive bias を持たせる。

**7つの普遍軸 (Axis enum)**:

- **A. Coherence（一貫性）**: 要素同士が同じ世界観に属しているか。テキストでは BBScore (Zhao et al., 2023) の Brownian Bridge coherence metric が定量化可能。文埋め込みの軌跡が滑らかなブリッジに従うかで全体的な coherence を測定する。

- **B. Hierarchy（階層/主従）**: 何が主役かが瞬時に分かるか。視覚的には注目度マップ、テキストでは情報構造分析で評価する。

- **C. Rhythm（リズム）**: 時間/間/反復の設計。映像のカット割り、UIのスクロール体験、文章の段落配分、音のBPM/グルーヴ、全てに存在する普遍的な軸。

- **D. Contrast（コントラスト）**: 差が効いているか。密度、明暗、速度、語彙、音圧等の対比を評価する。

- **E. Craft（工芸度）**: 細部の処理がプロ水準か。タイポグラフィ、エッジ処理、補間品質、編集点、語尾統一等の精度を見る。

- **F. Intentionality（意図の強さ）**: 偶然の寄せ集めではなく、選択が見えるか。VibeCheck (Bai et al., 2024) の contrastive analysis を採用。「汎用ベースライン」との差分で「意図的な選択」を定量化する。

- **G. Novelty（新規性）**: 既視感を抜けつつ、事故っていないか。QUASAR (Chowdhury et al., 2024) の exemplar-based scoring で「典型」からの距離を測定する。

**スコアリング方式**: 各軸 0.0–1.0 のスコア分布。NIMA方式の10-bin histogram を軸ごとに出力する。

**キャリブレーション**: LLM-Rubric (Gambhir et al., 2024) の手法を適用する。LLMベースの評価器は軸間で相関する傾向がある（coherenceを問われてもfluencyを混ぜて評価する等）。軸ごとの明示的 rubric 定義 + human anchor examples によるキャリブレーションで分離精度を保証する。

「プロっぽさ」の核は特に **Craft + Rhythm + Intentionality** の3軸。ここを Universal Critic の主要 KPI として重点配置する。

---

### §3.3 Domain Adapter（領域アダプタ）

**役割**: 普遍コアの判定結果を受け、ジャンル固有の「どう直すか」を出力する。

**設計原則**: 感性は普遍コアで判定し、改善はアダプタで実行する。この分離が必須。各ジャンルに修正オペレータ (correction operator) を持ち、普遍コアのスコアを具体的な改善提案に変換する。

**UIClip (Xiao et al., Apple/CMU 2024) + UICrit (Duan et al., Berkeley/DeepMind, UIST 2024) の知見**: 汎用 aesthetic モデルに対し、ドメイン特化クリティークデータで 55% の性能向上が確認されている。Domain Adapter は「汎用モデルのファインチューン」ではなく「ドメイン専門家データで独立に訓練」すべきである。

**ドメイン別の修正オペレータ例**:

- **映像 (VideoOp)**: カット構成、尺配分、イージング設計、stagger、トランジション、テロップ処理
- **UI/Web (UiOp)**: レイアウト再設計、コンポーネント再編、デザイントークン適用、モーション設計、パフォーマンス最適化
- **文章 (TextOp)**: 論旨再構成、比喩/反復の活用、語尾統一、情報密度調整、アウトライン追加
- **音 (AudioOp)**: BPM/グルーヴ調整、帯域バランス、音の立ち上がり、間の設計、ダイナミクス改善

各 Suggestion は `{ op, rationale, priority }` の構造で、単なる指摘ではなく**改善根拠と優先度を含む**。

---

## §4 検出仕様 / Detection Specification

**課題**: LLMが「テンプレ vs プロ」を見抜けない最大要因は入力の情報不足（静止画1枚、説明文だけ）にある。

**核心**: プロっぽさは「要素」ではなく「配分・順序・曲線・間」に宿る。そこを測る仕様が必要。

### 観測すべき4つの特徴カテゴリ

1. **時間情報 (Temporal)**: 動画/インタラクション/文章の「展開」を観測する。静止画だけの評価は禁止。映像では FAST-VQA の fragment sampling、テキストでは BBScore の軌跡分析を使う。

2. **差分情報 (Differential)**: 遷移点、編集点、段落の切り替わり、速度曲線の変化を観測する。「変化の質」を見ることで、プロの編集判断が浮かび上がる。

3. **均一性検知 (Uniformity Detection)**: 同じ duration / 同じ ease / 同じ語尾 / 同じ構文の濫用を検出し減点する。テンプレートの最大特徴は「均一さ」であり、これを定量的に捉える。

4. **意図的な揺らぎ検知 (Intentional Variance)**: 均一を崩す設計（強弱、溜め、余韻、間）を加点する。プロの最大特徴は「計算された不均一さ」であり、これを均一性スコアとの差分で検出する。

### モダリティ別の特徴抽出アプローチ

| モダリティ | 時間情報 | 差分情報 | 均一性 | 意図的揺らぎ |
|---|---|---|---|---|
| 映像 | フレームレベル fragment | カットポイント速度曲線 | duration/ease 反復 | stagger/緩急設計 |
| UI | scroll/hover 時系列 | ページ遷移パターン | コンポーネント配置反復 | アクセント/ブレイク |
| 文章 | 段落展開軌跡 (BBScore) | トピック遷移マップ | 構文/語尾パターン | レトリック密度変化 |
| 音 | BPM/テンション曲線 | セクション遷移エネルギー | リズムパターン反復 | グルーヴ揺らぎ/間 |

---

## §5 学習パイプライン / Learning Pipeline

**基本方針**: 全ジャンル対応の感性はルールではなく「人間の選好」から学習する部分が中心になる。

### ペア比較 (Pairwise Comparison)

AとBどちらが良いか。ジャンル横断の主観は、絶対採点より比較の方が安定して教師信号が強い。Website Aesthetics (Peng et al., 2023) の実証研究でも comparison-based が rating-based を一貫して上回ることが示されている。

**データ形式**: `(X, C, Y)` で保存する。

- X: 成果物 (Artifact)
- C: 文脈 (TasteContext — domain, genre, purpose, audience, culture, brand)
- Y: 比較結果 (left / right / tie / abstain) + 任意の理由ラベル

### ランキングモデル

ImageReward (Xu et al., 2023) に倣い Bradley-Terry model を採用する：

```
P(A > B) = σ(r(A) - r(B))
```

r は学習済み reward function。137K pairs が信頼性ある reward model の最低規模目安とされている。

**集計アルゴリズム**: UI-Bench (2025) で採用された TrueSkill (Microsoft) を使う。単純な Elo より優れた Bayesian スキルレーティングで、推移律・不確実性・疎な比較グラフを正しく処理できる。

### モデル構造

一つの巨大モデルに全部押し込むより、分離した構造が堅牢：

- **普遍コアのランキングモデル（全ジャンル共通）**: 7軸それぞれに対する Bradley-Terry reward function
- **ジャンル別の小さな補正（LoRA / Expert / Adapter）**: ジャンルが増えても破綻しにくい構造

### Fine-grained vs Coarse Feedback

Google DeepMind (2024) の知見として、軸別の fine-grained 評価が coarse な「どちらが良い」を必ず上回るわけではない。モデルアーキテクチャとデータ量に依存する。v1 では coarse comparison から開始し、データ蓄積後に fine-grained per-axis comparison に移行する。

### Social Signal Bootstrap

Social Reward (ICLR 2024) の知見として、明示的ペア評価は高コスト。暗黙的ソーシャルフィードバック（GitHub stars、Spotify ストリーム、Medium クラップス）で preference data を初期ブートストラップできる。

**バイアス警告**: LAION Aesthetic Predictor (2022) の bias audit が示す通り、crowd-sourced aesthetic preferences には人口統計・文化的バイアスが内在する。デプロイ前にバイアス監査が必須。

---

## §6 メタ感性と適応 / Meta-Taste & Adaptation

**目標**: 「最初から全部知ってる万能の審美」ではなく、「新ジャンルでも"上手い逸脱"を短時間で掴む能力」の仕様化。

### Two Acceptance Criteria

1. **ゼロショット一般性 (Zero-shot Generality)**: 未知ジャンルでも、平均的な人間の好み順位と高い一致率でランキング可能。CLIP / ImageBind embeddings + Universal Critic の組み合わせで実現する。

2. **高速適応 (Few-shot Adaptation)**: 新ジャンルに対して少数の比較（20〜50回のA/B）で、そのジャンルの上位作品を安定して当てられる。

### 適応メカニズム（3段階、段階的に導入）

**Level 1 — Exemplar-based (QUASAR方式)**: embedding 空間内の reference exemplars（良い作品の例）との近傍スコアリング。ファインチューニング不要で即時適応できる。v1 で導入。

**Level 2 — Task Vector Composition (Zhu et al., ECCV 2024)**: 既存ドメイン（Web、映像、文章、音）ごとに事前計算した task vector（weight delta）の線形結合で、新ジャンルのモデルを合成する。推論時 O(1)、per-user training 不要でスケーラブル。

**Level 3 — MAML-based Meta-Learning (BLG-PIAA, Zhu et al., 2022)**: 各ジャンルを独立「タスク」として扱い、meta-initialization から少数 gradient steps で適応する。DGS-MAML (2025) の flat-minima 最適化で、訓練タスク分布から大きく外れた新ジャンルにも汎化できる。

**重要な知見**: BLG-PIAA が示したのは、「平均的な審美」で事前学習したモデルは personalization の初期化として不適切だということ。meta-learning で直接 individual preference data から学習した方が few-shot 適応が優れる。

---

## §7 AsteronIris統合設計 / Integration Design

AsteronIris の trait + factory + config パターンに準拠する。skillforge の Scout → Evaluate → Integrate パイプラインを参考にしつつ、独立モジュールとして設計する。

### モジュール構成

```
src/taste/
├── mod.rs          # thin facade: pub mod + pub use + create_taste_engine()
├── types.rs        # Artifact, TasteContext, TasteReport, AxisScores, PairComparison
├── engine.rs       # TasteEngine trait + DefaultTasteEngine orchestrator
├── perceiver.rs    # Perceiver trait + modality-specific implementations
├── critic.rs       # UniversalCritic trait + LLM-based / heuristic implementations
├── adapter.rs      # DomainAdapter trait + domain-specific correction operators
├── store.rs        # TasteStore trait + SQLite implementation
└── learner.rs      # TasteLearner trait + Bradley-Terry / Elo implementation
```

### 公開 Trait（subsystem boundary）

```rust
#[async_trait]
pub trait TasteEngine: Send + Sync {
    async fn evaluate(&self, artifact: Artifact, ctx: TasteContext) -> anyhow::Result<TasteReport>;
    async fn record_comparison(&self, comparison: PairComparison) -> anyhow::Result<()>;
}
```

### 主要データ型

全型に `#[derive(Debug, Clone, Serialize, Deserialize)]` を付与する。

```rust
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Artifact {
    Text { content: String, format: Option<TextFormat> },
    Image { bytes: Vec<u8>, mime: String },
    Audio { bytes: Vec<u8>, mime: String },
    Video { bytes: Vec<u8>, mime: String },
    Interaction { events: Vec<InteractionEvent> },
}

#[derive(Default)]
pub struct TasteContext {
    pub domain: Domain,
    pub genre: Option<String>,
    pub purpose: Option<String>,
    pub audience: Option<String>,
    pub culture: Option<String>,
    pub brand: Option<BrandContext>,
    pub constraints: Vec<String>,
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[serde(rename_all = "snake_case")]
pub enum Axis {
    Coherence, Hierarchy, Rhythm, Contrast, Craft, Intentionality, Novelty,
}

pub struct TasteReport {
    pub axis: AxisScores,  // BTreeMap<Axis, f32>
    pub domain: Domain,
    pub highlights: Vec<Evidence>,
    pub suggestions: Vec<Suggestion>,
}

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Suggestion {
    General { title: String, rationale: String, priority: Priority },
    Ui { op: UiOp, rationale: String, priority: Priority },
    Text { op: TextOp, rationale: String, priority: Priority },
    Video { op: VideoOp, rationale: String, priority: Priority },
    Audio { op: AudioOp, rationale: String, priority: Priority },
}

pub struct PairComparison {
    pub domain: Domain,
    pub ctx: TasteContext,
    pub left_id: String,
    pub right_id: String,
    pub winner: Winner,  // Left / Right / Tie / Abstain
    pub created_at_ms: u64,
}
```

### Factory Function

```rust
pub fn create_taste_engine(
    config: &TasteConfig,
    // optional dependencies
) -> anyhow::Result<Arc<dyn TasteEngine>> { ... }
```

### Config（TOML）

```toml
[taste]
enabled = false
backend = "llm"       # "llm" | "heuristic" | "neural"
axes = ["coherence", "hierarchy", "rhythm", "contrast", "craft", "intentionality", "novelty"]
# modality toggles
text_enabled = true
image_enabled = false  # feature-gated: taste-image
video_enabled = false  # feature-gated: taste-video
audio_enabled = false  # feature-gated: taste-audio
```

### Tool 統合

Agent が会話中に使える Tool として公開する：

- `taste.evaluate`: Artifact + TasteContext → TasteReport
- `taste.compare`: 2つの Artifact ID + winner + context → 比較記録 + learner 更新

### Feature Gates

```rust
#[cfg(feature = "taste")]        // module itself
#[cfg(feature = "taste-image")]  // image perceiver
#[cfg(feature = "taste-video")]  // video perceiver
#[cfg(feature = "taste-audio")]  // audio perceiver
```

### 内部 Trait（pub(crate)）

`Perceiver`、`UniversalCritic`、`DomainAdapter`、`TasteStore`、`TasteLearner` はモジュール内部に閉じる。public API を安定させながら実装を自由に進化させるための境界設計。

---

## §8 段階的実装計画 / Phased Rollout

### Phase 1 — Text Critic（v0.1）

`Artifact::Text` のみ + LLMベース UniversalCritic で3軸スコアリング（Coherence / Hierarchy / Intentionality）+ `Suggestion::General` と `Suggestion::Text`。学習なし、推論のみ。

- **Deliverables**: types.rs、engine.rs、critic.rs (LLM backend)、adapter.rs (text only)
- **工数目安**: 1–2日

### Phase 2 — UI Domain Adapter + Tool

UI/Web ドメインアダプタ（rule-based suggestions）+ `taste.evaluate` Tool 統合。Agent が会話中に UI スクリーンショットを評価できるようになる。

- **Deliverables**: adapter.rs (UI ops)、tool integration
- **工数目安**: 1–2日

### Phase 3 — Pair Comparison Learning

SQLite に比較データ永続化 + in-process Bradley-Terry / TrueSkill profile。`taste.compare` Tool を追加。profile を使って axis scores を re-rank / weight する。

- **Deliverables**: store.rs、learner.rs、taste.compare tool
- **工数目安**: 2–3日

### Phase 4 — Image Perceiver

`Artifact::Image` を `feature = "taste-image"` で追加。VLM inference を既存 providers/ 経由で実行。全7軸に拡張する。

- **Deliverables**: perceiver.rs (image backend)、7-axis expansion
- **工数目安**: 2–3日

### Phase 5 — Video/Audio + External Neural

映像/音声パーシーバー + より高度なドメインオペレータ。外部 neural trainer integration（RPC boundary 経由）。

- **Deliverables**: video/audio perceivers、external trainer adapter
- **工数目安**: 3–5日

---

## §9 成功基準 / Success Criteria

- **ゼロショット一般性**: 未知ジャンルにおいて、人間評価者の好み順位との Kendall τ ≥ 0.5（中程度の一致）
- **高速適応**: 20–50件のペア比較後、同一ジャンルの Kendall τ ≥ 0.7（強い一致）
- **軸間独立性**: 7軸のスコア間の平均ペアワイズ相関 |r| < 0.5（各軸が異なる次元を測定していることを確認）
- **Template/Pro 判別精度**: 既知ドメイン（Web/映像）で F1 ≥ 0.80
- **適応速度**: 新ドメイン投入から calibration 完了まで ≤ 50 comparisons
- **レイテンシ**: 単一 Artifact 評価 < 5秒（テキスト）/ < 15秒（画像、VLM経由）

---

## §10 技術参照 / Technical References

本設計で参照した主要論文・技術を以下にまとめる。

| 技術 / Paper | 年 | 対象 | 主要手法 | 本設計での用途 |
|---|---|---|---|---|
| NIMA (Talebi & Milanfar) | 2018 | 画像 | スコア分布予測 + EMD loss | スコア出力形式の設計 |
| MUSIQ (Ke et al.) | 2022 | 画像 | Multi-scale ViT、解像度非依存 | Perception Layer のトークン化 |
| CLIP (Radford et al.) | 2021 | マルチ | Contrastive Language-Image Pre-training | Aesthetic embedding backbone |
| ImageBind (Girdhar et al.) | 2023 | マルチ | Image-pivot 6モダリティ joint embedding | Perception Layer アーキテクチャ |
| UniBind (Luo et al.) | 2024 | マルチ | LLM-anchored balanced embedding | Cross-modal alignment 改善 |
| FAST-VQA (Wu et al.) | 2022 | 映像 | Grid Mini-patch Sampling + FANet | 映像品質の temporal sampling |
| DOVER (Wu et al.) | 2023 | 映像 | Aesthetic/technical two-branch | Universal Critic 二系統設計 |
| QUASAR (Chowdhury et al.) | 2024 | 画像 | Non-parametric exemplar scoring | Few-shot ドメイン適応 |
| VisualCritic | 2024 | 画像 | LMM fine-tuned for quality perception | 説明可能性レイヤー |
| LAION Aesthetic Predictor | 2022 | 画像 | CLIP linear probe for aesthetics | Backbone 選定 + バイアス警告 |
| ImageReward (Xu et al.) | 2023 | 画像 | Bradley-Terry on 137K pairs | ペア比較学習パイプライン |
| Social Reward | 2024 | 画像 | Implicit social feedback as reward | Preference data ブートストラップ |
| UIClip (Xiao et al.) | 2024 | UI/Web | CLIP adapted for UI quality | Web ドメインアダプタ |
| UICrit (Duan et al.) | 2024 | UI/Web | Expert critique dataset (983 UIs) | UI スコアリング rubric |
| UI-Bench | 2025 | UI/Web | TrueSkill expert pairwise ranking | ランキング集計アルゴリズム |
| BBScore (Zhao et al.) | 2023 | テキスト | Brownian Bridge coherence metric | テキスト coherence 測定 |
| LLM-Rubric (Gambhir et al.) | 2024 | テキスト | Calibrated multi-dim LLM evaluation | 軸間キャリブレーション |
| VibeCheck (Bai et al.) | 2024 | テキスト | Contrastive style analysis | Voice/style 次元の定量化 |
| BLG-PIAA (Zhu et al.) | 2022 | 画像 | MAML for personalized IAA | Domain Adapter meta-learning |
| Task Vectors (Zhu et al.) | 2024 | 画像 | Weight-space task composition | スケーラブルなドメイン適応 |
| DGS-MAML | 2025 | 汎用 | Flat-minima meta-learning | OOD ジャンル汎化 |
