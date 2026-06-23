# 02. インジケーター仕様

各インジケーターの計算式・既定パラメータ・出力（生値 + 正規化サブスコア `s_i ∈ [-1,+1]`）・実装上の注意。すべて Rust 純粋関数（`src-tauri/src/indicators/`）。週足・月足は**各足を yfinance から直接取得**して計算する（日足からのリサンプリング禁止）。

## サブスコア正規化の方針（`normalize.rs`）

- **有界オシレーター**（RSI, %B, Williams %R 等）: レンジを線形に [-1,+1] へ写像。
- **非有界**（MACD ヒスト, z-score 等）: `tanh(x / k)` で圧縮（`k` はATRや標準偏差でスケール）。
- **トレンド系**（MA並び, Supertrend, ADX方向）: 符号 × 強度。
- すべて最終 `clamp(-1, +1)`。NaN（履歴不足）は `None` を返し、合成時はそのカテゴリ平均から除外。

サブスコアの符号規約: **+ = 買い方向、− = 売り方向**。逆張り系もこの規約で出力し、レジーム調整（符号反転/ゼロ化）は `scoring/` 側で行う（`docs/03-scoring.md`）。

---

## トレンド系（`trend.rs`）

### ADX / DMI（既定 14）
- `+DI`, `-DI`, `ADX` を Wilder 平滑で算出。レジーム判定の主役（`docs/03`）。
- サブスコア: `s = sign(+DI - -DI) * clamp(ADX/50, 0, 1)`（方向 × トレンド強度）。

### Supertrend（ATR 10, multiplier 3.0）
- `basic_upper = (H+L)/2 + mult*ATR`, `basic_lower = (H+L)/2 - mult*ATR`、トレンド方向のフリップで final band 決定。
- サブスコア: 価格がライン上=+、下=−。`s = dir * clamp(|close - line| / (mult*ATR), 0, 1)`。

### EMA リボン（20 / 50 / 200）
- 3本のEMAの**並び順（パーフェクトオーダー）**と**傾き**を連続スコア化。
- `order_score`: 20>50>200 で +1、逆順で −1、混在は部分点。`slope_score`: 各EMAの直近変化率の符号平均。
- サブスコア: `s = clamp(0.6*order_score + 0.4*slope_score, -1, +1)`。

### 一目均衡表（9 / 26 / 52）
- 転換線・基準線・先行スパンA/B（雲）・遅行スパン。日本株で有効。
- 多段サブスコア（各 ±1 を重み付き合算）: 価格 vs 雲（上=+/中=0/下=−, 重 0.4）, 転換 vs 基準（0.3）, 遅行 vs 26本前価格（0.2）, 雲のねじれ方向（0.1）。
- サブスコア: 上記加重和を `clamp`。

---

## モメンタム系（`momentum.rs`）

### MACD（12 / 26 / 9、MTF対応）
- `macd = EMA12 - EMA26`, `signal = EMA9(macd)`, `hist = macd - signal`。
- サブスコア: `s = tanh(hist / (k * ATR))`（k 既定 0.5）。ゼロライン越え・シグナルクロスは近接度（`p_thresh` 補助）でも使用。
- MTF: 日足/週足/月足それぞれで算出し `scoring/mtf.rs` で統合（チャートは4色ヒストグラムで表示）。

### RSI（14）
- 標準 Wilder RSI。**レジーム依存の解釈**（`docs/03`）: トレンド中は 40–50 の押し目=買い継続、レンジ中は 30/70 を逆張り境界。
- サブスコア（レンジ時）: `s = (50 - RSI)/50 * (-1)` → オーバーボート=−、オーバーソールド=+。トレンド時の解釈は scoring 側で切替。

### Squeeze Momentum（LazyBear; BB 20/2.0, KC 20/1.5）
- **スクイーズ判定:** BB が KC の内側 = `sqz_on`（低ボラ）。BB が KC を出る = `sqz_off`（解放）。
- **モメンタム値:** `val = linreg(close - avg(avg(highest(H,20),lowest(L,20)), SMA(close,20)), 20, 0)`（線形回帰の値）。
- サブスコア: 解放後 `s = sign(val) * clamp(|val|/atr_scale, 0,1)`。`sqz_on` は近接度 `p_sqz` の核（`docs/03`）。
- 状態フラグ（black=sqz_on, gray=sqz_off, blue/その他）はチャートのスクイーズ点に使用。

### TSI（25 / 13）
- `TSI = 100 * EMA13(EMA25(Δclose)) / EMA13(EMA25(|Δclose|))`、シグナル線併用。
- サブスコア: `s = clamp(TSI/50, -1, +1) ` をベースに、シグナルクロス・ゼロラインで補正。

---

## 逆張り系（`mean_reversion.rs`）

> いずれも **+ = 買い、− = 売り** で出力。トレンド中の符号反転/無効化は scoring 側。

### Connors RSI(2)（RSI period 2, 200日MAフィルター）
- `RSI(2)`。フィルター: 価格 > SMA200（上昇トレンド）でのみ買いセットアップ有効。
- ルール: 上昇トレンドで `RSI2 < 5` 付近=買い、`RSI2 > 95` 付近=売り。
- サブスコア: `s = clamp((50 - RSI2)/50, -1, +1)`（低RSI2=+）。フィルター不成立時はゼロ。近接度 `p_cr` の核。

### Bollinger %B（20 / 2.0）
- `%B = (close - lower) / (upper - lower)`。
- サブスコア: `s = clamp((0.5 - %B)*2, -1, +1)`（%B<0=強い買い、>1=強い売り）。

### Williams %R（14）
- `%R = -100 * (highestH - close)/(highestH - lowestL)`。
- サブスコア: `s = clamp((-50 - %R)/50 * (-1)... )` → −80以下=買い、−20以上=売り。実装は `s = clamp(((-50) - %R)/-50, -1, +1)` を検証して確定。

### MA乖離 z-score（vs SMA20、直近100本std）
- `z = (close - SMA20) / std(close - SMA20, 100)`。
- サブスコア: `s = clamp(-z/2, -1, +1)`（平均回帰圧。正乖離=売り、負乖離=買い）。

---

## ボラ / フィルター（`volatility.rs`）

> シグナルではなくゲート・リスク・近接度に使用。サブスコアは出さない（または品質ゲート 0..1）。

### ATR（14）
- Wilder ATR。損切り幅・ポジションサイジング・`p_pull` の距離単位・`tanh` スケールに使用。

### BB幅 / ケルトナー（20）
- `bb_width = (upper-lower)/middle`。スクイーズ本体（ブレイク前の収縮）。`p_sqz` と品質ゲートに使用。

### Choppiness Index（14）
- `CHOP = 100*log10(Σ ATR / (maxH-minL)) / log10(n)`。高=レンジ、低=トレンド。ADX とともにレジーム判定を補強。

---

## ゴールデン値戦略（テスト基準）

- **数値基準: TA-Lib（または pandas-ta）** を正とし、固定 OHLCV スライスに対する期待値で単体テスト（`tests/fixtures/`）。決定的・文書化済みで再現可能。
- **視覚照合: TradingView** で数本の銘柄を目視確認し、シグナルに効く流儀差（RSI/ADX の seeding、EMA の初期化、ケルトナーの ATR 定義など）がないことを確認。
- TA-Lib と TradingView は ADX/RSI の初期化等で微差が出ることがある。**シグナル境界に影響しない範囲**であることをテストで担保し、影響する場合は TradingView 流儀に寄せる（本ツールは TradingView 由来の指標を再現する意図のため）。
- ゴールデン値生成は小さな Python スクリプト（pandas-ta）で行い、`tests/fixtures/` に JSON 保存（`docs/07-testing.md`）。

## 実装順（P1）

`normalize.rs` → ATR/EMA/RSI 等の基礎 → ADX/DMI・Supertrend・BB/KC → MACD・SqueezeMomentum → 一目・TSI・ConnorsRSI2・%B・%R・z-score・Choppiness。各指標はゴールデンテスト緑で「完了」。

> **実装状況（P1 完了）:** 上記すべてを `src-tauri/src/indicators/`（`mod.rs` 基礎 + `trend.rs` / `momentum.rs` / `mean_reversion.rs` / `volatility.rs`）に実装済み。ゴールデン基準は **TA-Lib 0.6.8**（ADR-13 が第一に挙げる基準。pandas-ta 0.3.14b0 は PyPI 取り下げ）で、`src-tauri/tests/golden.rs` が許容誤差 1e-6 で照合（緑）。TA-Lib 非対応の指標（Supertrend・一目・Squeeze・TSI・MA乖離zスコア・Choppiness、および MACD のシグナル線）は `tools/golden/gen_golden.py` 内に Rust と同一アルゴリズムの参照実装を持つ。**ATR は TA-Lib のウォームアップ・シードに合わせた**（TradingView との差はウォームアップのみ＝シグナル無関係）。**Williams %R のサブスコア符号**は docs 上「検証して確定」とされていた箇所を、オーバーソールド=買い(+)・オーバーボート=売り(−)の向きに確定した。
