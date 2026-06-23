# 07. テスト戦略

「外部の勝率は信用せず自前で検証する」を実装で担保する。3層: ①ゴールデン値単体テスト ②結合テスト ③評価ハーネス（モデル妥当性）。

## ① ゴールデン値単体テスト（指標 / P1）

- **基準:** TA-Lib / pandas-ta を正とし、固定 OHLCV スライスに対する期待値を `tests/fixtures/` に JSON 保存。決定的・再現可能。
- **対象:** `docs/02` の全指標 + サブスコア正規化。境界（オーバーボート/ソールド、スクイーズ on/off、雲の上下）を必ず含む。
- **許容誤差:** 絶対/相対誤差の小さい閾値（例 1e-6〜1e-4）。**シグナル境界に影響する差は不可**。影響する場合は TradingView 流儀に寄せる（本ツールは TradingView 由来指標の再現が意図）。
- **視覚照合:** 数本の実銘柄で TradingView と目視比較し、流儀差（RSI/ADX seeding、EMA 初期化、ケルトナー ATR 定義）がシグナルに効かないことを確認。

### ゴールデン値生成スクリプト（例）
固定 CSV → 各指標値を計算 → `*.golden.json` 出力。CIでは生成済み JSON を検証（再生成はしない）。

> **実装メモ（セッション1）:** ゴールデン基準は **TA-Lib**（ADR-13 が第一に挙げる基準）を採用。pandas-ta 0.3.14b0 が PyPI から取り下げられ、後継 0.4.x はライセンス・API が不安定なため。生成器は `tools/golden/gen_golden.py`（uv 管理 Python 3.12 / `ta-lib==0.6.8`、テスト時のみ）で、決定的合成 OHLCV（seed 42）を `tests/fixtures/sample_basic_1d.csv` に書き出し、TA-Lib 値を `*.golden.json` に出力する。再生成: `uv run --project tools/golden python tools/golden/gen_golden.py`。Rust 側は `src-tauri/tests/golden.rs` が許容誤差 1e-6 で照合。ATR は TA-Lib のウォームアップ・シードに合わせる（TradingView との差はウォームアップのみで、シグナル領域では収束 = docs/02 の流儀差）。

```
# 概念
df = pd.read_csv("tests/fixtures/sample_7974T_1d.csv")
out = { "rsi14": ta.rsi(df.close,14).tolist(),
        "adx14": ta.adx(df.high,df.low,df.close,14).to_dict(...),
        ... }
json.dump(out, open("tests/fixtures/sample_7974T_1d.golden.json","w"))
```

### Rust 側テスト（例）
```rust
#[test]
fn rsi14_matches_golden() {
    let candles = load_fixture("sample_7974T_1d.csv");
    let golden  = load_golden("sample_7974T_1d.golden.json");
    let got = indicators::momentum::rsi(&candles, 14);
    assert_series_close(&got, &golden.rsi14, 1e-4);
}
```

## ② 結合テスト

- **サイドカー ↔ Rust round-trip:** モック/実 yfinance で 1d/1wk/1mo を取得し `Candle` に正しく載るか、errors が `RowError` に変換されるか。
- **CSV → scan → ランキング:** 小さなユニバース CSV で `scan_universe` を回し、`SymbolScore`（方向 + 近接度 + actionability）が埋まり、降順ソート・買い売り分割が正しいか。
- **キャッシュ/差分更新:** 2回目スキャンで不足分のみ取得・upsert されるか。重複銘柄が1回だけ取得されるか。
- **degrade:** 週足/月足欠落銘柄が weekly_gate=0.8 / monthly_mod=1.0 でブロックされず処理されるか。

## ③ 評価ハーネス（モデル妥当性 / P7）★P8 の前提

`eval/`（`prediction-eval` 流用）。スコア/近接度が**実際にエッジを持つか**を統計的に検証。

| 指標 | 内容 |
|---|---|
| 二項有意検定 | 方向的中率がベースライン50%に対し統計的有意か（p値） |
| MFE / MAE | シグナル後の最大含み益/含み損の分布 |
| 期待値 | `win_rate*avg_win − loss_rate*avg_loss`（勝率ではなくこれを見る） |
| プロフィットファクター | 総利益 / 総損失 |
| 近接度の妥当性 | proximity_score 高低でシグナル後リターンが分離するか（高近接度ほど良いか） |
| Triggered vs Active | Triggered（新鮮）が Active（出遅れ）よりリターン優位か |

- **層別:** レジーム別（TrendUp/TrendDown/Range/Transition）・asset_class別に分けて評価（トレンド中とレンジ中は別物）。
- **ウォークフォワード:** in-sample で重み決定 → out-of-sample で検証。**OOS でのみエッジを信頼**。
- **退化チェック:** 全銘柄 Buy になっていないか、シグナル頻度が異常でないか、近接度が飽和していないか。

### 出力
`EvalReport { binomial_p, mfe_mae, expectancy, profit_factor, proximity_lift, by_regime, by_asset_class, walkforward }`。UI もしくは JSON で確認。

> **実装状況（P7 完了）:** `src-tauri/src/eval/mod.rs` + `commands/evaluate_model`。各銘柄の日足履歴を歩き、近接度状態（Triggered/Active）が示す方向で **前方 N バー（既定10）** のリターン・MFE/MAE を測定。`Stats`（hit_rate・二項p値・期待値・PF・MFE/MAE）を、全体 / IS / OOS / レジーム別 / asset_class別 / 状態別（Triggered vs Active）/ 近接度バケット（high/mid/low）で集計。`Degeneracy`（buy比率・signals/symbol・近接度飽和）。二項p値は正規近似（連続性補正）、PF は999キャップ、非有限は0サニタイズ（JSON安全）。
> - **既知の限界:** バー単位の Active サンプルは時系列重複（自己相関）するため、広域サンプルの p 値は楽観的。シグナル頻度の指標には離散クロス（`is_cross`）を使用。真のウォークフォワード最適化は P8。
> - **所見（17銘柄ユニバース：米大型株+日本株+クリプト、141,354サンプル、既定パラメータ）:** 全体 hit 52.1%・期待値 **+0.38%/10bar**・PF 1.16（**IS 52.2% ≒ OOS 52.1%** ＝過剰最適化なし、未調整なので当然）。
>   - **レジーム別が決定的**: TrendUp は **PF 1.52・期待値+1.02%** と強いエッジ。一方 **TrendDown は PF 0.94・期待値 −0.20% とマイナス**（≒2020–2026 の概ね上昇相場での売り/ショートは不利）。Range/Transition は弱い正。→ **ロング偏重・ショート抑制**が P8 の最初の調整方針。
>   - **近接度リフトは単調**（low +0.31% < mid +0.49% < high≥70 +0.52%）＝近接度軸に予測価値あり。**crypto**（PF 1.29）> equity（1.15）。退化なし（buy比率0.65・飽和8%）。
>   - 再現: `cargo test --test e2e_live live_eval_universe -- --ignored --nocapture`（要 uv・ネットワーク）。
>
> **P8 チューニング（完了）:** `src-tauri/src/eval/tuning.rs`。仮説駆動の候補 `ScanConfig` をウォークフォワード評価（**IS expectancy で選択 → OOS で確認**、OOS を選択に使わない）。`cargo test --test e2e_live live_tune -- --ignored --nocapture` で実行。
>   - **結果（17銘柄）:** `long_bias`（売りしきい値 −40→**−55**＝低確信ショート抑制）が IS で選択され **OOS でも確認**: ベースライン OOS 期待値 +0.17%/PF 1.08 → **+0.67%/PF 1.36**（約4倍）。P7 の「ショート側が不利」という所見と整合。週足ゲート強化・品質ゲート・高しきい値は限界的効果のみ。
>   - **反映:** `ScanConfig::preset(Standard)` = この確定値。アプリ既定（`get_config`）は Standard を返す。`Conservative`/`Aggressive` プリセットも用意（`get_presets`）。生の `ScanConfig::default()` は未調整ベースライン（チューニング候補の基準）として保持。
>   - **限界:** ユニバース17銘柄・概ね上昇相場・重複サンプルのため確信度は中程度。運用前に銘柄・期間・レジームの拡充が望ましい。

## CI ゲート（GitHub Actions）

- `cargo test`（ゴールデン値 + 結合）緑が**マージ条件**。
- `cargo clippy` 警告ゼロ目標。
- frontend typecheck / lint 緑。
- サイドカーの OS 別ビルド成功。
- （任意）評価ハーネスのスモーク（小ユニバースでレポートが生成されること）。

## フィクスチャ方針

- `tests/fixtures/` に固定 CSV（数銘柄 × 各足）+ `*.golden.json`。実データのスナップショットを少量同梱（再現性のため）。サイズ大の生データはコミットしない（`.gitignore`）。
- 評価用の長期 OHLCV はローカルキャッシュ（SQLite）から供給。CI ではスモークのみ。
