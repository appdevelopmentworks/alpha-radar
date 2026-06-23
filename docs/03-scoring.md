# 03. スコアリング & 近接度エンジン

本ツールの心臓部。**方向スコア（-100〜+100）** と **近接度スコア（0〜100）** の2軸を、レジーム判定を起点に算出する。`src-tauri/src/regime/`・`scoring/`・`proximity/`。

## パイプライン順序（厳守）

```
1. regime 判定        (ADX/Choppiness)
2. カテゴリサブスコア  S_f = mean(s_i in f)        ← s_i は indicators/ から
3. レジーム重み付き合成 S_raw = Σ w_f(regime)*S_f   ← 逆張りは符号調整
4. 品質ゲート          S_tf = S_raw * gate()
5. 正規化              Score_tf = clamp(round(S_tf*100), -100, +100)
6. MTF 統合            Score_final = mtf(Score_daily, weekly, monthly)
7. 近接度              proximity_score, signal_state  ← 日足から
8. アクション度        actionability                  ← リスト並べ替え用
```

> **原則①の実装点（最重要）:** 逆張り指標と順張り指標は正反対のシグナル。レジーム判定なしに加算すると自己相殺する。必ず regime → 重み切替（トレンド中は逆張りの符号反転/ゼロ化）→ 合成 の順。

---

## 1. レジーム判定（`regime/mod.rs`）

```
enum Regime { TrendUp, TrendDown, Range, Transition }

fn detect_regime(adx, plus_di, minus_di, chop) -> Regime {
    if adx > 25.0 {
        if plus_di > minus_di { TrendUp } else { TrendDown }
    } else if adx < 20.0 {
        Range
    } else {
        Transition            // 20..=25
    }
    // Choppiness で補強: CHOP が高い(>61.8)場合 Range 寄り、低い(<38.2)場合 Trend 寄りに
    // 境界(Transition)を CHOP で TrendX / Range に倒す
}
```

連続レジームスコア（重み補間に使用、任意）: `regime_strength = clamp((adx-20)/30, 0, 1)`。

---

## 2. カテゴリサブスコア（`scoring/composite.rs`）

```
S_trend          = mean(trend 系 s_i)        // None は除外
S_momentum       = mean(momentum 系 s_i)
S_mean_reversion = mean(mean_reversion 系 s_i)
volatility_gate  = gate(bb_width, liquidity) // 0..1
```

---

## 3. レジーム別カテゴリ重み（`scoring/weights.rs`）

`RegimeWeightTable`（既定。`ScanConfig.category_weights` で上書き可）:

| カテゴリ \ レジーム | TrendUp | TrendDown | Range | Transition |
|---|---|---|---|---|
| trend | 0.40 | 0.40 | 0.10 | 0.25 |
| momentum | 0.40 | 0.40 | 0.20 | 0.30 |
| mean_reversion | **−0.20**※ | **−0.20**※ | 0.55 | 0.30 |

```
fn weighted_composite(s, regime, w) -> f64 {
    let mr = match regime {
        TrendUp | TrendDown => -s.mean_reversion,  // ★符号反転: 押し目は継続買い
        _                   =>  s.mean_reversion,
    };
    let raw = w.trend(regime)*s.trend
            + w.momentum(regime)*s.momentum
            + w.mean_reversion(regime)*mr;
    raw
}
```

> 符号反転の意味: 強い上昇トレンド中の RSI オーバーソールドは「フェード（売り）」ではなく「押し目買いの継続シグナル」。逆張りサブスコア（買い=+）をそのまま足すと momentum の買いと整合してしまうため、トレンド中は反転させて「逆張り的売りシグナルを抑制／押し目を買い方向に寄与」させる。実装は上記の `-s.mean_reversion` と負の重み（−0.20）の組合せで、**トレンド中の逆張り売りシグナルを減衰**させる効果を持つ。チューニング（P8）で符号・重みを最適化する。

---

## 4. 品質ゲート & 単一TF スコア

```
S_tf = weighted_composite(...) * volatility_gate
Score_tf = clamp((S_tf * 100.0).round(), -100.0, 100.0)
// volatility_gate: スクイーズ未解放(sqz_on)や極端な低流動性で 1.0→減衰
```

---

## 5. MTF 統合（`scoring/mtf.rs`）★確定仕様

役割分離: **近接度（タイミング）は日足。合成スコアは方向の確信度。**

```
// 第1段: α 加重合成（確定値）
Score_MTF = 0.55*Score_daily + 0.30*Score_weekly + 0.15*Score_monthly

// 第2段: 方向ゲート（エッジの本体）
fn weekly_gate(daily_signal_dir, weekly_regime) -> f64 {
    match weekly_regime {
        aligned_with(daily_signal_dir) => 1.0,
        Range | Transition             => 0.8,
        opposed_to(daily_signal_dir)   => 0.4,   // 逆張り・低確信。ゼロにはしない
    }
}
fn monthly_mod(daily_signal_dir, monthly_regime, enabled) -> f64 {
    if !enabled { return 1.0; }
    match monthly_regime {
        aligned  => 1.1,    // 上限キャップ
        opposed  => 0.85,   // ★ハードゲートにしない
        _        => 1.0,
    }
}

Score_final = clamp(Score_MTF * weekly_gate * monthly_mod, -100, +100)
```

**設計判断の根拠:**
- **日足が単独過半(0.55):** エントリーのタイミングは日足にしか存在しない（週足は週1確定、月足はさらに遅い）。上位足に薄められてはいけない。
- **週足=月足の2倍(0.30 vs 0.15):** スイングの保有期間(約1–4週)は本質的に週足スケールの値動き。月足はバイアス程度。幾何級数的減衰(各上位足は約4–5倍遅い→寄与逓減)とも整合。
- **月足はソフト修正子(ハードゲート禁止):** 月足は回転が遅く、ハードゲート化すると大底で日足・週足が反転した初動ロングを長期間ブロックする。よってゼロ化せず微修正に留める。`monthly_mod_enabled=false` で完全無効化も可能。
- **週足は強めのゲート:** 上位足トレンドは"票"ではなく"文脈"。逆行シグナルは減衰（Connors RSI2 の200日MAフィルター・上位足MACDフィルターが一貫して支持）。
- α加重とゲートで週足はやや二重計上だが、捉える情報が異なる（加重=週足スコアの強さ / ゲート=方向整合）。**エッジを生むのはゲート側で α は補助。** 定数は P8 ウォークフォワードの調整起点で不可侵ではない。

---

## 6. 近接度エンジン（`proximity/mod.rs`）★主機能

方向とは別軸で「いま／もうすぐ仕掛けられる」度合いを 0〜100 で算出。**日足ベース。**

### シグナル状態（state machine）

```
enum Direction { Buy, Sell, None }
enum SignalState {
    PrimedBuy, TriggeredBuy, ActiveBuy,
    Neutral,
    PrimedSell, TriggeredSell, ActiveSell,
}
// 付随: bars_since_trigger: Option<u32>
```

### 近接度コンポーネント（各 [0,1]、regime が示す方向についてのみ計算）

```
// 1) 合成しきい値への接近（トリガー前の側からの距離 + 接近速度）
//    approach_floor: 近接を測り始める下限（例 0 か 閾値の半分）
p_thresh = clamp((Score_final - approach_floor) / (buy_threshold - approach_floor), 0, 1)
         * velocity_bonus(dScore_over_k_bars)     // 上昇中なら 1.0→最大1.3 等

// 2) スクイーズ近接（Squeeze Momentum）
//    sqz_on かつ未解放 = 仕込み度。解放直後 = トリガー
p_sqz = if sqz_on { clamp(bars_in_squeeze / typical_squeeze_len, 0, 1) }
        else if just_released { 1.0 } else { 0.0 }

// 3) 平均回帰トリガー近接（Connors RSI2、価格>200MA のときのみ買い側）
//    RSI2 が <5 トリガーへ上から接近 = 押し目買いが近い
p_cr = clamp((cr_buy_zone - RSI2) / cr_buy_zone, 0, 1)   // cr_buy_zone 例 10

// 4) 主要構造への接近（押し目）: 価格と主要MA/バンド端の距離を ATR 単位で
p_pull = clamp(1 - dist_to_key_level_in_atr / max_dist_atr, 0, 1)  // max_dist_atr 例 2.0
```

### 集約・状態判定・鮮度減衰

```
// regime 対応の集約（重みは方向スコアと整合 / または max 採用）
proximity_raw = weighted_or_max(p_thresh, p_sqz, p_cr, p_pull ; regime)
proximity_score = (proximity_raw * 100).round()   // 0..100

// 状態判定
if triggered_within(fresh_bars_n) {            // 直近 fresh_bars_n(既定1-2) バーで閾値クロス
    state = Triggered{dir};
    proximity_score = max(proximity_score, 90)
} else if setup_forming_no_trigger() {
    state = Primed{dir}
} else if bars_since_trigger > fresh_bars_n {
    state = Active{dir};
    proximity_score *= decay(bars_since_trigger)   // 出遅れは減点 e.g. 0.95^bars
} else {
    state = Neutral
}
```

### アクション度（リスト並べ替え用）

```
actionability = proximity_score * (0.5 + 0.5 * Score_final.abs() / 100.0)
// タイミング(近接度) × 確信度(|方向スコア|) の混合。リストは降順。
```

---

## 7. エッジケース / 数値処理

- **履歴不足:** 200MA/52本一目などに足りない場合、その指標は `None`。カテゴリ平均から除外。全カテゴリ算出不能なら `SignalState::Neutral`・近接度0。
- **上位足データ欠落:** 週足/月足が取れない銘柄は `weekly_gate=0.8`(中立)・`monthly_mod=1.0` で degrade（ブロックしない）。UI に「MTF部分欠落」を表示。
- **NaN/Inf:** 計算途中の NaN は伝播させず early-return（その指標 None）。ゼロ除算（バンド幅0等）はガード。
- **新規上場/データ薄:** 最小バー数（例 60）未満はスキャン対象外として `RowError` に。
- **決定性:** カテゴリ平均・MTF 加重の集約順序を固定（HashMap 反復順に依存しない）。

## チューニング対象（P8）

レジーム別カテゴリ重み表、MTF α、weekly/monthly ゲート係数、buy/sell 閾値、`approach_floor`、`max_dist_atr`、`fresh_bars_n`、近接度集約方式（weighted vs max）。**P7 の検証（`docs/07-testing.md`）を通してからチューニングする。**

---

## 実装状況

- **P2 完了**（§1–§5）: レジーム判定 `src-tauri/src/regime/mod.rs`、方向スコアリング `src-tauri/src/scoring/`（`weights.rs` = 重み付き合成 + 逆張り符号反転、`composite.rs` = カテゴリ集約・品質ゲート・単一TFスコア、`mtf.rs` = α加重・週足ゲート・月足修正子、`mod.rs` = `direction_score` オーケストレーション）。重み表・しきい値・MTF係数は `config.rs` の `ScanConfig`/`RegimeWeightTable`/`MtfConfig`（既定値は本書の表）。クラフトしたシナリオ（強上昇／レンジ／逆行週足／上位足欠落 degrade）の単体テスト緑。
  - **RSI(14) の扱い**: docs/02 では momentum 系だが、サブスコアは「レンジ志向」（`(50-RSI)/50`、オーバーボート=−）で出力し、スコアリングでは **mean_reversion カテゴリ**に割り当てる。これによりトレンド中の符号反転（§3）が RSI の「トレンド時=押し目買い継続／レンジ時=逆張り」の切替を自動的に実現する。
  - **品質ゲート**: 現状はスクイーズ未解放(`sqz_on`)で `squeeze_gate`(既定0.8)に減衰。流動性ゲートは P4（出来高/ADV）で追加予定。
- **P3 完了**（§6）: 近接度エンジン `src-tauri/src/proximity/mod.rs`。コンポーネント `p_thresh`(velocity bonus付)・`p_sqz`(スクイーズ蓄積/解放スパイク)・`p_cr`(Connors RSI2、200MAフィルタ+方向ゲート)・`p_pull`(主要EMAへのATR距離) → **max集約** → 状態機械（Triggered=直近 `fresh_bars_n` バー内のしきい値クロス→近接度≥90 / Primed=未クロスのセットアップ形成 / Active=過去クロス→`active_decay` 減衰 / Neutral）→ `actionability = proximity_score*(0.5+0.5*|Score_final|/100)`。係数は `config.rs` の `ProximityConfig`。クラフトしたシナリオ（しきい値クロス→Triggered→Active減衰、スクイーズ蓄積→解放、押し目接近、形成中=Primed）の単体テスト緑。
  - **集約方式**: 既定は `max`（いずれかのコンポーネントがトリガー間際なら近接度が立つ）。weighted との選択は P8 のチューニング対象。
  - **方向判定**: 当該バーの日足スコア符号で Buy/Sell を決める（`p_cr` は 200MA フィルタで買い/売りを別ゲート）。
