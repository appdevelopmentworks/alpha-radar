# Alpha Radar

**Swing Entry Confluence Scanner** — ウォッチリスト（CSV）から「いま／もうすぐ仕掛けられる」銘柄をレーダーのように炙り出す、スイングトレード向けテクニカルシグナル・スキャナー兼チャートツール。

AILEAP の投資インテリジェンス系（`alpha-sentinel` / Alpha Valuation Terminal / `prediction-eval`）のテクニカル分析レイヤー。

---

## これは何か

- CSV で渡した複数銘柄を一括スキャンし、**エントリー近接度（Imminence）** でランキング表示する。
- 単体インジケーターの「精度」ではなく、**レジーム選別 × コンフルエンス（複数指標の合意）× ATRリスク管理** で期待値を狙う。
- 方向（買い/売り・確信度: -100〜+100）と、**タイミングの近さ（0〜100）** を別軸で評価する。
- 個別銘柄はマルチペインチャート（価格 + MACD + Squeeze Momentum + 合成スコア）でドリルダウン。

> 設計の核は「方向スコアが強くても“もう動いた後”のことがある。本当に欲しいのは“これからトリガーする／いまトリガーした”銘柄」という点。詳細は `docs/03-scoring.md`。

## 技術スタック

| レイヤー | 採用 |
|---|---|
| デスクトップ | Tauri 2 |
| フロントエンド | Next.js + TypeScript |
| バックエンド（計算） | Rust（インジケーター/レジーム/スコア/近接度の単一真実源） |
| データ取得 | Python サイドカー（yfinance、PyInstaller/Nuitka で同梱） |
| チャート | lightweight-charts v5（マルチペイン） |
| 永続化 | SQLite（+ 必要に応じ DuckDB） |

データソースは **yfinance**。タイムフレームは **日足（主軸）/ 週足 / 月足**（履歴無制限）。リアルタイム不要・スイング判断が主用途。

## ドキュメント一覧

Claude Code はまずルートの **`CLAUDE.md`** を読むこと。詳細仕様は `docs/`：

| ファイル | 内容 |
|---|---|
| [`CLAUDE.md`](./CLAUDE.md) | Claude Code 運用指示・規約・言語ポリシー・ガードレール（英語） |
| [`docs/00-requirements.md`](./docs/00-requirements.md) | 要求定義書 v0.3（マスター仕様） |
| [`docs/01-architecture.md`](./docs/01-architecture.md) | アーキテクチャ・ディレクトリ構成・Tauriコマンド・サイドカー・CI |
| [`docs/02-indicators.md`](./docs/02-indicators.md) | インジケーター仕様（計算式・パラメータ・サブスコア・ゴールデン値） |
| [`docs/03-scoring.md`](./docs/03-scoring.md) | レジーム判定・コンフルエンス・MTF・近接度エンジンの数式 |
| [`docs/04-data-spec.md`](./docs/04-data-spec.md) | CSV/OHLCV/SQLiteスキーマ・サイドカーI/O契約・yfinance仕様 |
| [`docs/05-ui-spec.md`](./docs/05-ui-spec.md) | 画面仕様（ドラッグ&ドロップ・ランキングリスト・チャート） |
| [`docs/06-implementation-plan.md`](./docs/06-implementation-plan.md) | 実装フェーズ計画 P0–P8（タスク・完了条件・モデル選択） |
| [`docs/07-testing.md`](./docs/07-testing.md) | テスト戦略（ゴールデン値・検証ハーネス・ウォークフォワード） |
| [`docs/08-decisions-adr.md`](./docs/08-decisions-adr.md) | 決定ログ / ADR（確定事項と根拠。再議しないこと） |

## 状態

**P0〜P8 実装済み + 設定画面 + 配布パッケージング完了**: 足場、アイコン、**P1 インジケーター全指標**（ゴールデン値テスト緑）、**P2 レジーム判定 + 方向スコアリング**、**P3 近接度エンジン**、**P0 Python サイドカー**、**P4 CSV取込 + バッチスキャン + キャッシュ**、**P5 近接度ランキング画面**、**P6 マルチペインチャート**、**P7 評価ハーネス**、**P8 ウォークフォワード・チューニング**を実装。`CSV → yfinance → キャッシュ → スコア → ランキング → チャート` が**実データでエンドツーエンド動作**（17銘柄ユニバースで検証・チューニング済み）。

P1 — インジケーター計算エンジン（ゴールデン値テスト、TA-Lib 基準・許容誤差 1e-6）:
- **基礎**: SMA / EMA / RSI / ATR / linreg、サブスコア正規化（`normalize.rs`）
- **トレンド**: ADX/DMI・EMAリボン(20/50/200)・Supertrend(ATR10/3.0)・一目均衡表(9/26/52)
- **モメンタム**: MACD(12/26/9)・Squeeze Momentum(BB20/KC20/linreg)・TSI(25/13)
- **逆張り**: Connors RSI(2)+200MAフィルタ・Bollinger %B・Williams %R・MA乖離zスコア
- **ボラ/フィルタ**: Bollinger幅・Keltner・Choppiness Index
- TA-Lib 非対応の指標（Supertrend/一目/Squeeze/TSI/zスコア/Choppiness）は `tools/golden/gen_golden.py` 内に Rust と同一アルゴリズムの参照実装を持ち、決定的合成データで照合。

P2 — レジーム判定 + 方向スコアリング（`regime/` `scoring/`、docs/03 準拠）:
- **レジーム判定**: ADX>25/<20 + Choppiness 境界補強で 4 レジーム（TrendUp/Down/Range/Transition）
- **カテゴリ重み + 逆張り符号反転**（ADR-07）: トレンド中は逆張りサブスコアを反転し、押し目を継続買いに寄与
- **品質ゲート**（スクイーズ減衰）→ 単一TFスコア `clamp(round(S·gate·100), ±100)`
- **MTF 統合**（ADR-09）: α=0.55/0.30/0.15 + 週足強ゲート(1.0/0.8/0.4) + 月足ソフト修正子(1.1/0.85)、`direction_score()` で `Score_final` 算出

P3 — 近接度エンジン（`proximity/`、日足ベース、ADR-08）:
- **コンポーネント**: `p_thresh`(しきい値接近+速度ボーナス)・`p_sqz`(スクイーズ蓄積/解放)・`p_cr`(Connors RSI2+200MA)・`p_pull`(主要EMAへのATR距離)→ max 集約
- **状態機械**: Triggered(新鮮クロス→近接度≥90) / Primed(形成中) / Active(出遅れ→鮮度減衰) / Neutral
- **`actionability` = proximity_score × (0.5 + 0.5·|Score_final|/100)**（ランキング用、タイミング×確信度）

P0 — Python サイドカー（`sidecar/`、ADR-02、uv 管理）:
- `fetch.py`: yfinance バッチ取得（stdin/stdout JSON、`auto_adjust`、リトライ/指数バックオフ、構造化エラー）。**実データで検証済み**（AAPL 1d / 7974.T 1wk / BTC-USD 1mo / 不正銘柄=errors）
- `mock_fetch.py`: ネットワーク非依存のモック（`cargo test` を hermetic に保つ round-trip 用）
- `data/sidecar.rs`: `SidecarClient`（プロセス spawn + JSON round-trip、コマンド注入式 = dev は uv / release は externalBin）

P4 — CSV取込 + バッチスキャン + キャッシュ（`data/` `commands/`、docs/04）:
- `data/csv.rs`: ヘッダベース・寛容パース（symbol 正規化・asset_class 推定・重複排除・行エラー収集）
- `data/cache.rs`: SQLite（rusqlite bundled）、docs/04 の DDL、**差分更新**（last_ts → Full/From/Skip）、OHLCV upsert、scores/scan_runs 保存
- `commands/scan_universe`: CSV → 差分判定 → サイドカー1バッチ取得 → upsert → メモリロード → **rayon 並列スコアリング** → `SymbolScore` 組み上げ → 保存 → `ScanResult`。行単位エラーで全体は止まらない。`get_config` も結線

P5 — 近接度ランキング画面（`frontend/`、docs/05、主画面 `/`）:
- `DropZone`（Tauri ネイティブ drag-drop + dialog プラグインでファイル選択）→ `scan_universe` 実行
- `RankingTable`: **actionability 降順・買い/売り2ブロック・中立折りたたみ**、状態バッジ（Triggered 濃 / Primed 中 / Active 淡）、近接度バー、方向スコア（赤–灰–緑グラデ）、カテゴリ内訳、ATR/損切り。ソート/フィルタ、CSV/JSON エクスポート、エラー集約表示
- `lib/invoke.ts`（Tauri command ラッパ）・`lib/types.ts`（Rust DTO ミラー）

P6 — マルチペインチャート（`frontend/`、lightweight-charts v5、docs/05、`/chart?symbol=`）:
- Rust `commands/get_chart_data`: OHLC + EMAリボン/Supertrend/一目 + MACD(4色ヒスト) + Squeeze(4色) + 合成スコア(しきい値線) + BUY/SELL マーカー + MTF サマリーを**全て Rust 計算**（チャートとリストのスコアが完全一致＝再計算しない）
- `MultiPaneChart`（価格/MACD/Squeeze/スコアの4ペイン・`attributionLogo` 有効）・`MtfSummary`・足切替
- ※静的エクスポート制約により動的ルートではなく `?symbol=` クエリ方式

P7 — 評価ハーネス（`eval/`、`commands/evaluate_model`、docs/07、ADR: P8 より前に必須）:
- 履歴を歩いて近接度状態が示す方向で**前方Nバーのリターン**を測定 → **二項有意検定・期待値・プロフィットファクター・MFE/MAE**
- **層別**: 全体 / IS / OOS / レジーム別 / asset_class別 / Triggered vs Active / 近接度バケット（近接度リフト）+ **退化チェック**（buy比率・signals/symbol・近接度飽和）
- 所見（17銘柄）: 全体 PF 1.16、**TrendUp PF 1.52 / TrendDown PF 0.94（ショート不利）**、近接度リフト単調。重複サンプルで p 値は楽観的（限界明記）

P8 — ウォークフォワード・チューニング（`eval/tuning.rs`、ADR: OOS 確認後に確定）:
- 仮説駆動の候補を **IS expectancy で選択 → OOS で確認**（OOS を選択に使わない）
- **確定:** `long_bias`（売りしきい値 −40→**−55**＝低確信ショート抑制）が OOS 期待値を **+0.17%→+0.67%・PF 1.08→1.36** に改善
- `ScanConfig::preset(Standard)` = 確定値（アプリ既定）。`Conservative`/`Aggressive` も用意（`get_presets`）。`cargo test --test e2e_live live_tune -- --ignored` で再現

| 区分 | 確定バージョン |
|---|---|
| Tauri | CLI 2.10.1 / crate `tauri 2.10`（ビルド解決 2.11.3）/ `tauri-build 2.5.6` |
| Next.js / React / TS | 16.2.9 / 19.2.4 / 5（App Router・`output: 'export'`） |
| Rust | edition 2021 / toolchain stable（1.96） |
| ゴールデン基準 | **TA-Lib 0.6.8**（uv 管理 Python 3.12、テスト時のみ）※ pandas-ta 0.3.14b0 が PyPI から取り下げのため、ADR-13 が第一に挙げる TA-Lib を採用 |

- **アイコンソース**: `docs/icon.png`（1254² 正方形）→ `cargo tauri icon` で生成。
- **追加依存**: Rust = `csv`・`rusqlite`(bundled)・`tauri-plugin-dialog`。フロント = `@tauri-apps/api`・`@tauri-apps/plugin-dialog`・`lightweight-charts` 5.2。サイドカー = `uv sync --project sidecar`（yfinance）。
- **テスト**: Rust 単体 58 + ゴールデン 4 スイート + チャート結合 緑、`clippy` 警告ゼロ。`-- --ignored` でサイドカー round-trip / **実データ E2E**（`tests/e2e_live.rs`: scan→chart→eval→tune）。
- **配布（パッケージング完了）**: `pwsh tools/package-sidecar.ps1`（PyInstaller で `fetch.py`→単一exe・`externalBin` 配置）→ `cargo tauri build --bundles nsis` で **NSIS インストーラ**生成（`Alpha Radar_0.1.0_x64-setup.exe`）。リリースアプリは**単体起動**（埋め込みフロント・dev サーバ不要）、`SidecarClient::resolve()` が同梱 `fetch.exe`/dev `uv` を自動判定。`src-tauri/binaries/` は gitignore（ビルド成果物）。
- **次工程**: 評価/チューニングの **UI 露出**（現状はコマンド/JSON）、スキャン進捗イベントのストリーミング、評価ユニバースの拡充、CI（GitHub Actions）での OS 別サイドカービルド + リリース自動化（`docs/01` CI 節）。

### 開発コマンド

```
# 開発（Tauri + Next.js dev サーバ）   : cargo tauri dev
# 本番バンドル                          : cargo tauri build
# フロント静的エクスポート              : npm run build            (frontend/ 内、out/ 生成)
# Rust テスト（ゴールデン値含む）        : cargo test --manifest-path src-tauri/Cargo.toml
# Rust lint                             : cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
# ゴールデン値の再生成                  : uv run --project tools/golden python tools/golden/gen_golden.py
```

## ライセンス / 帰属

- lightweight-charts は TradingView の帰属表示（`attributionLogo`）が必要。
- yfinance は Yahoo の非公式ライブラリ。個人・研究用途の範囲で利用すること。
