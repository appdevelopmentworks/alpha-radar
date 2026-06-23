# 06. 実装フェーズ計画（Claude Code 向け）

P0–P8。各フェーズの目標・タスク・成果物・完了条件（DoD）・推奨モデル。**フェーズを飛ばさず、`docs/` を都度同期する。**

## モデル選択方針（Shin のポリシー）

- **Opus 級:** アーキテクチャ/設計レビュー、数学が重い P2/P3 の設計。
- **Sonnet 級:** 通常の実装。
- **Fable 5:** 曖昧な上流判断、数理エージェント、オーケストレーション。

## 依存グラフ

```
P0 (sidecar) ─┐
              ├─> P4 (取込/バッチ/キャッシュ) ─> P5 (リスト) ─┐
P1 (指標)  ───┴─> P2 (regime+方向) ─> P3 (近接度) ──────────┴─> P6 (チャート) ─> P7 (検証) ─> P8 (調整)
```
- **P0 と P1 は独立・並行可。**
- P2 は P1、P3 は P2、P4 は P0+P2、P5 は P3+P4、P6 は P5、P7 は P6、P8 は最後。

---

## P0 — Python サイドカー（データ取得）
**目標:** yfinance で 1d/1wk/1mo を取得し JSON/Parquet を返すステートレスバイナリ + Rust からの起動。
**タスク:**
- `sidecar/fetch.py`: stdin JSON リクエスト → yfinance `download`（バッチ, auto_adjust, リトライ/バックオフ）→ stdout JSON（`docs/04` 契約）。
- `requirements.txt`、PyInstaller ビルド。
- `src-tauri/src/data/sidecar.rs`: `new_sidecar` 起動・JSON 送受信・エラー解釈。
- `tauri.conf.json` の `externalBin` 登録。
**成果物:** 任意銘柄リストの3足 OHLCV を取得できる。
**DoD:** 日本株/米株/クリプト各1銘柄で 1d/1wk/1mo が取得でき、不正銘柄が errors で返る。手動 round-trip テスト緑。
**モデル:** Sonnet（契約は本書既定）。

## P1 — Rust インジケーター計算エンジン（P0と並行可）
**目標:** 全指標を純粋関数で実装、ゴールデン値一致、生値+サブスコア出力。
**タスク:**
- `indicators/normalize.rs` → 基礎（EMA/RSI/ATR/SMA/linreg）→ ADX/DMI・Supertrend・BB/KC → MACD・SqueezeMomentum → 一目・TSI・ConnorsRSI2・%B・%R・z-score・Choppiness（`docs/02`）。
- `tests/fixtures/` のゴールデン値（pandas-ta 生成）で単体テスト。
**成果物:** `indicators::*` が `(raw, sub_score)` を返す。
**DoD:** 全指標がゴールデン値（許容誤差内）一致。サブスコアが [-1,+1]。`cargo test` 緑。**ゴールデンテストなしの指標は未完了扱い。**
**着手前の確定:** ゴールデン値基準=TA-Lib/pandas-ta 数値 + TradingView 目視（`docs/02`/`07`）。
**モデル:** Sonnet（実装）、境界値の流儀差判断は Opus/Fable 5。

## P2 — レジーム判定 + 方向スコアリング（単一TF → MTF）
**目標:** regime 判定、レジーム重み（逆張り符号反転）、単一TF合成、MTF（α+ゲート）。
**タスク:**
- `regime/mod.rs`（ADX/Choppiness、4レジーム）。
- `scoring/weights.rs`（レジーム別重み表 + 上書き）、`composite.rs`（単一TF）、`mtf.rs`（α=0.55/0.30/0.15・weekly_gate・monthly_mod）。`docs/03` の数式に厳密準拠。
**成果物:** `Score_final` と内訳。
**DoD:** クラフトしたシナリオ（強上昇/レンジ/逆行週足 等）で期待挙動。符号反転・週足ゲート・月足ソフト修正が効く。単体テスト緑。
**モデル:** 設計レビュー Opus、実装 Sonnet。

## P3 — 近接度エンジン
**目標:** Primed/Triggered/Active 状態、proximity_score、actionability。
**タスク:**
- `proximity/mod.rs`: `p_thresh`/`p_sqz`/`p_cr`/`p_pull`、集約、状態機械、鮮度減衰、actionability（`docs/03`）。日足ベース。
**成果物:** `SymbolScore` の近接度軸が埋まる。
**DoD:** スクイーズ仕込み→解放、トレンド中の押し目接近、しきい値接近のシナリオで状態・近接度が妥当。単体テスト緑。
**モデル:** 設計 Opus/Fable 5、実装 Sonnet。

## P4 — CSV取込 + バッチスキャン + キャッシュ
**目標:** ドロップ済み CSV を端から端まで処理して `ScanResult` を返す。
**タスク:**
- `data/csv.rs`（パース/正規化）、`data/cache.rs`（SQLite DDL・差分更新・upsert）、`data/universe.rs`。
- `commands/scan_universe`（P1–P3 を結線、rayon 並列、行エラー収集、進捗）。
**成果物:** `scan_universe(csv, config)` 動作。
**DoD:** 数百銘柄が回る。差分更新でキャッシュ効く。失敗行が `RowError` に積まれ全体は止まらない。
**モデル:** Sonnet。

## P5 — フロント: 近接度ランキングリスト
**目標:** `docs/05` の Scanner 画面。
**タスク:** `DropZone`・`RankingTable`・`SignalBadge`・`ProximityBar`・`lib/invoke.ts`・`lib/types.ts`。actionability 降順・買い売り2分割・状態バッジ・ソート/フィルタ/エクスポート。進捗/エラー表示。
**成果物:** ドロップ→スキャン→ランキングの一連が動く。
**DoD:** 「いま仕掛けられる」銘柄が最上部・濃色で3秒で分かる。エクスポート可。
**モデル:** Sonnet（UI は frontend-design 準拠）。

## P6 — フロント: チャートビュー
**目標:** `docs/05` のマルチペインチャート + マーカー + MTFサマリー。
**タスク:** `get_chart_data`（Rust: OHLC+指標系列+マーカー+MTFサマリー）、`MultiPaneChart`（lightweight-charts v5 ペイン）、`MtfSummary`、`attributionLogo`。
**成果物:** 行クリック→4ペイン+買い売りマーカー。
**DoD:** チャートの指標とリストのスコアが**完全一致**（同一 Rust 計算）。マーカー位置が `Score_final` クロスと一致。
**モデル:** Sonnet。

## P7 — 評価ハーネス（検証）★P8 より前に必須
**目標:** スコア/近接度モデルの妥当性を統計的に検証。
**タスク:** `eval/`（`prediction-eval` 流用）: 二項有意検定（vs 50%）、MFE/MAE、シグナル後Nバーのリターン・期待値・プロフィットファクター、**近接度の妥当性**（高近接度がリターンで分離するか）、レジーム別/asset_class別層別、ウォークフォワード（IS/OOS）。`commands/evaluate_model`。
**成果物:** `EvalReport`。
**DoD:** 既定パラメータでレポートが出る。エッジの有無を数値で判断できる。`docs/07` の指標を網羅。
**モデル:** 数理は Opus/Fable 5、実装 Sonnet。

## P8 — パラメータ/重みチューニング + ウォークフォワード
**目標:** OOS で期待値プラスを確認してから運用パラメータを確定。
**タスク:** レジーム別重み表・MTF α・ゲート係数・閾値・近接度パラメータをウォークフォワードで最適化。過剰最適化を `docs/07` の OOS 分離で監視。
**DoD:** OOS で期待値プラス・統計的に妥当な設定を1つ確定。プリセットに反映。
**モデル:** Fable 5（数理/曖昧上流）、検証 Opus。

---

## 各フェーズ共通の作法
- 着手時に該当 `docs/` を読み、完了時に `docs/` と `README.md` の状態を更新。
- マジックナンバーはコードに埋めず `Config`/`ScanConfig`/`MtfConfig` から。既定値は本書/`docs/03`。
- ネットワークはサイドカー経由のみ。レート制限を尊重。
- 指標・スコアは純粋関数・決定的。テストファースト（特に P1）。
