# 05. UI 仕様

Next.js + TypeScript（Tauri 2 内）。**主画面は近接度ランキング（レーダー）**、チャートはドリルダウン、設定でパラメータ調整。UI 文字列は日本語。

## 画面マップ

| 画面 | ルート | 役割 |
|---|---|---|
| Scanner / Radar | `/` | CSV ドロップ → スキャン → 近接度ランキング（主画面） |
| Chart | `/chart/[symbol]` | 個別銘柄マルチペインチャート（ドリルダウン） |
| Settings | `/settings` | プリセット・重み・閾値・近接度パラメータ |

## (A) Scanner / Radar（主画面）

### ドラッグ&ドロップ（`DropZone.tsx`）
- 画面に大きめのドロップボックス。CSV ドロップで即 `scan_universe` 実行。クリックでファイル選択も可。
- ドロップ後: 進捗バー（`n/N 銘柄処理済み`）、完了でリスト表示。取得失敗行は折りたたみの「エラー（k件）」で一覧。

### ランキングリスト（`RankingTable.tsx`）
- **既定ソート: `actionability` 降順**（タイミングが近い銘柄が上）。
- **方向で2ブロック表示:** 上に「買い接近」（緑系）、下に「売り接近」（赤系）。中立はデフォルト折りたたみ。
- 列:

| 列 | 内容 | 表示 |
|---|---|---|
| 銘柄 / 名称 | symbol + name | クリックで `/chart/[symbol]` |
| 状態 | SignalState | バッジ（`SignalBadge.tsx`）: Triggered=濃色・強調 / Primed=中間色 / Active=淡色（出遅れ） |
| 近接度 | proximity_score 0–100 | 横バー（`ProximityBar.tsx`） |
| 方向スコア | Score_final −100..+100 | 数値 + 色（赤–灰–緑グラデ） |
| 的中率 | marker_hit_rate / marker_samples | `56% (42)`。FR-8 マーカー規則を日足全履歴でバックテストし、`marker_horizon_bars`（既定10）営業日後の終値が順行した割合。標本数併記、履歴不足は「—」 |
| 直近マーカー | last_marker {kind, dir, bars_ago} | 全種（確定/QT/前兆）から最新1件をバッジ表示（例「前兆買 本日」「QT売 3日前」）。配色はチャートのマーカーレイヤーと一致（確定=緑/赤、QT=青/橙、前兆=淡色）。経過は営業日ベース、同一バーは 確定>QT>前兆。経過日数でソート可（既定昇順=新しい順）、なしは「—」（ADR-16） |
| レジーム | Regime（日足） | ラベル |
| 内訳 | trend/momentum/mean_rev | ミニバー or ツールチップ |
| 経過 | bars_since_trigger | 数値（新しいほど良い） |
| ATR / 損切り | atr, suggested_stop | 数値 |
| 更新 | scanned_at | 相対時刻 |

- **色規約:** 方向スコア = `-100`赤 / `0`灰 / `+100`緑 の連続スケール。状態バッジ = Triggered 濃・Primed 中・Active 淡で「鮮度」を明示。
- ソート/フィルタ: actionability・近接度・方向スコア・状態・レジーム・asset_class・しきい値。
- エクスポート: 現在のリストを CSV / JSON 出力（クライアント提案資料転用）。

## (B) Chart（ドリルダウン, `MultiPaneChart.tsx`）

lightweight-charts v5 マルチペイン。エントリーポイントと指標が分かりやすいことを最優先。

- **ペイン構成（添付画像2準拠）:**
  1. 価格: ローソク足 + EMAリボン / Supertrend / 一目（トグル） + **買い/売りマーカー**
  2. MACD（MTF）: 4色ヒストグラム + MACD/シグナル線
  3. Squeeze Momentum: linreg ヒストグラム + スクイーズ点（sqz_on/off クロス）
  4. 合成スコア: `Score_final` 時系列 + buy/sell しきい値ライン
- **MTF サマリー（`MtfSummary.tsx`, 画像1相当）:** チャート上部に 日足/週足/月足 の Regime と velocity（Acceleration/Deceleration）をテーブル表示。
- **マーカー:** `BUY @ price` / `SELL @ price`（series-markers, `zOrder` 制御）。画像1の◇相当。**Supertrend 脚ごとに最大1個**：脚の方向をスコアが初めて閾値強度で確認したバーに表示（FR-8 / ADR-14）。
- 指標値は **`get_chart_data` の Rust 計算結果を描画**（フロントで再計算しない＝リストと完全一致）。
- 足切替（日足/週足/月足）。
- **初期表示範囲:** 全体 fit ではなく直近 `chart_bars` 本にズーム（設定の「初期表示ローソク足数」、既定120＝スイング目安。`ChartData.initial_bars` で受領）。足数が設定値より少ない銘柄は全体表示。時間軸のパン/ズームは自由。

## (C) Settings（`/settings`）

- **プリセット:** Conservative / Standard / Aggressive（閾値・重みのセット）。
- **閾値:** buy/sell（既定 ±40）スライダー。
- **MTF:** α（日足0.55 / 週足0.30 / 月足0.15）、weekly_gate（整合1.0 / 中立0.8 / 逆行0.4）、monthly_mod（整合1.1 / 逆行0.85 / **無効化トグル**）。
- **近接度:** `fresh_bars_n`（既定1–2）、`approach_floor`、`max_dist_atr`、集約方式（weighted/max）。
- **チャート表示:** `chart_bars`（初期表示ローソク足数、既定120）。表示専用パラメータで計算には不使用。
- **的中率:** `marker_horizon_bars`（既定10）— マーカーの何営業日後の終値で順行判定するか（リスク/対象セクション）。
- 変更は `update_config` で保存。スキャン時に `scan_runs.config_json` へスナップショット。

## コンポーネントツリー（概略）

```
app/
├── layout.tsx
├── page.tsx                  # Scanner: <DropZone/> + <RankingTable/>
│   └─ <RankingTable/>        #   <SignalBadge/>, <ProximityBar/>, <ScoreCell/>
├── chart/[symbol]/page.tsx   # <MtfSummary/> + <MultiPaneChart/> + <ChartToolbar/>
└── settings/page.tsx         # <PresetSelector/> + <ParamSliders/>
lib/
├── invoke.ts                 # scanUniverse / getChartData / evaluateModel / get|updateConfig
└── types.ts                  # Rust DTO ミラー (SymbolScore, ChartData, ScanConfig, ...)
```

## 状態管理 / データ取得

- Tauri `invoke` を `lib/invoke.ts` に集約。型は `lib/types.ts`（Rust DTO ミラー、`ts-rs` 自動生成推奨）。
- スキャン結果はページ状態（React state）で保持。再スキャンで置換。ブラウザストレージは使わない（Tauri 永続化は SQLite/Rust 側）。
- ローディング・エラーを明示（進捗、エラー件数、リトライ）。

## lightweight-charts v5 実装メモ

- **マルチペイン:** v5 のペインAPIでサブチャート（MACD/SQZMOM/スコア）を価格と縦積み。各ペインに `addSeries`（ライン/ヒストグラム/カスタム）。
- **マーカー:** series-markers プラグインで価格系列に BUY/SELL を付与。`zOrder` で前面化。
- **帰属表示:** `attributionLogo` を有効化（TradingView ライセンス要件・必須）。
- **同期:** 全ペインの時間軸を同期（クロスヘア・スクロール）。
- 描画データは Rust 由来の `ChartData` をそのまま `setData`。フロント再計算禁止。

## アクセシビリティ / 体感

- 「いま仕掛けられる」銘柄が**3秒で分かる**ことを最優先（Triggered を最上部・濃色・強調）。
- 出遅れ（Active）は淡色で沈め、誤って飛び乗らせない。
- 数値は等幅、色だけに依存しない（バッジ文言 + 形）。

---

## 実装状況（P5 + P6 完了）

- **P5 Scanner**（`frontend/app/page.tsx`、`components/DropZone|RankingTable|SignalBadge|ProximityBar|ScoreCell`、`lib/invoke.ts|types.ts`）: ドロップ→`scan_universe`→actionability 降順・買い/売り2ブロック・中立折りたたみ・状態バッジ（鮮度3段）・近接度バー・スコア色グラデ・内訳ミニバー・ソート/フィルタ・CSV/JSON エクスポート・エラー集約。
  - **入力経路2つ**: ① ティッカー直接入力テキストボックス（カンマ/スペース区切り、**`Enter` で実行**・改行は `Shift+Enter`。「スキャン実行」ボタンも併存）→ `scan_symbols` コマンド（`parse_symbols_str` で正規化）。② CSV ドロップ/選択 → `scan_universe`（ファイルパス）。Tauri ネイティブ drag-drop（`onDragDropEvent`＝パス取得）+ `tauri-plugin-dialog` の `open()`。
  - **ナビ保持**: スキャン結果・チャート表示トグル・**レーダーのビュー状態（ソート列/方向・資産フィルタ・検索語・中立開閉）**を `ScanProvider`（layout の React Context、`lib/radar-view.ts` に型/既定を定義）に保持。**リスト⇔チャートを行き来しても再スキャン不要**で、チャートから「レーダーへ戻る」してもソート/フィルタが復元される（ページ再マウントで既定に戻らない）。
  - **進捗**: スキャン中は **Tauri イベント `scan-progress`**（Rust `commands::ScanProgress { phase, done, total }`）を購読してプログレスバー表示。`phase="fetch"`（バッチ1回のネットワーク取得＝不確定アニメーション「データ取得中…」）→ `phase="load"`（銘柄ごとに `done/total` の確定バー「スコアリング n/N」）→ `done`。複数銘柄スキャン時の体感を改善。バッチ取得は粒度が無いため fetch 区間のみ不確定表示（レート制限ガードのためバッチ取得は維持）。
  - **的中率列**: `SymbolScore.marker_hit_rate / marker_samples`（Rust がスキャン時に計算）。FR-8 のマーカー規則（`scoring::marker_events` — チャートの売買マーカーと同一関数）を日足全履歴に適用し、各マーカーの `marker_horizon_bars`（既定10・設定可）営業日後の終値リターンが順行した割合。表示は `56% (42)`（標本数併記、ツールチップに内訳）、判定窓が取れない銘柄は「—」（ソートでは末尾）。CSV/JSON エクスポートにも含む。
  - **列ヘッダーソート**: 各列ヘッダー（銘柄/状態/近接度/方向スコア/的中率/レジーム/経過/ATR/損切り）をクリックで並べ替え。同じ列を再クリックで昇順/降順トグル、アクティブ列に ▲/▼ 表示。数値列は既定降順・銘柄名は昇順、`null` は常に末尾。状態は鮮度ランク（Triggered>Primed>Active>Neutral）、レジームは強度ランク（上昇>下降>レンジ>転換）でソート。ソートは買い/売り/中立の各ブロック内に適用（方向2ブロック構造は維持）。**既定は actionability 降順**（ヘッダーには無い軸＝列クリックで上書き）。`内訳` は単一値でないため非ソート列。旧ソート用ドロップダウンはヘッダーソートに統合し撤去。
- **P6 Chart**（`frontend/app/chart/page.tsx`、`components/MultiPaneChart|MtfSummary`、Rust `commands/get_chart_data`）: lightweight-charts v5 の4ペイン（価格+EMAリボン/Supertrend/一目+BUY/SELLマーカー / MACD 4色ヒスト+線 / Squeeze 4色 / 合成スコア+しきい値線）+ MTF サマリー + 足切替。**全系列は Rust 計算**でリストのスコアと一致（ADR-06）。`attributionLogo` 有効。
  - **ルーティング**: 静的エクスポート（`output: 'export'`）が任意 symbol の動的ルートを生成できないため、`/chart/[symbol]` ではなく **`/chart?symbol=` クエリ方式**（`useSearchParams` + `Suspense`）。
  - **表示トグル**: チャート上部にチェックボックス（EMAリボン / Supertrend / 一目 / MACD / Squeeze / **スコア** / 売買マーカー / Q-Trend / QT前兆 / **STフリップ**）。**既定 ON = EMAリボン・MACD・Squeeze・Q-Trend・QT前兆・STフリップ**、既定 OFF = Supertrend・一目・スコア・売買マーカー（ユーザー指定の初期ビュー）。**チャート生成（`data` 依存）と表示切替（`visible` 依存）の useEffect を分離**し、トグル時は再生成せず `applyOptions({visible})` + ペイン stretch factor の更新のみ（ズーム維持）。
  - **動的ペイン（ADR-16）**: MACD/Squeeze/スコアの OFF で該当ペインを `setStretchFactor(0)`（≈2px）まで収縮し、価格ペインが残余高さを吸収。可視サブペインは約130px 固定。**`setHeight` は使用禁止**（最小30pxクランプ+兄弟ペイン再膨張の実装挙動のため）。`layout.panes.enableResize: false`。スコア OFF でしきい値ライン・軸ラベルも自動的に消える（price line はシリーズ可視性に連動）。
  - **ウインドウ追従（ADR-16）**: `.chart-page` は 100vh のフレックス列・流体幅、`.chart-host` は `flex:1`（min-height 420px）。ウインドウのリサイズにチャートが縦横追従（`autoSize` + host の ResizeObserver でペイン高さ再計算）。大画面ではページスクロールなし。
  - **STフリップ（ADR-16）**: `ST LONG`（緑▲）/`ST SHORT`（赤▼）。ATS 視覚比較専用・単独エッジ実測なし。**専用パラメータ `stflip_atr`/`stflip_mult`（既定 10 / 2.0 — ATS のステータス行 "2 10" の近似）で表示用 Supertrend を別計算**し、スコアリング/確定マーカー用の Supertrend（10 / 3.0）とは独立（乗数 3.0 だとフリップが遅く・少なくなり ATS とずれるため）。
  - **トグル状態の保持**: チェックボックスの表示状態（`ChartVisibility`）は `ScanProvider`（layout の Context、`lib/chart-visibility.ts` に型/既定を定義）に保持。チャート→レーダーへ戻る→再度チャートを開いても**直前のインジケーター表示状態が復元**される（ページ再マウントで既定に戻らない）。
  - **初期ウィンドウサイズ**: 1200×900（`tauri.conf.json` の `app.windows[0]`）。
  - **初期表示ローソク足数**: チャート生成時に `fitContent()` ではなく `timeScale().setVisibleLogicalRange` で直近 `ChartData.initial_bars`（＝設定 `chart_bars`、既定120）本にズーム。足数不足の銘柄は `fitContent()` にフォールバック。設定画面（チャート表示セクション）で調整。全足（日/週/月）に同一本数を適用。**表示専用**で Rust 計算・スコアには一切影響しない。
  - スクイーズの on/off ドット行は未実装（val 4色ヒストで代替、将来追加）。
  - **Q-Trend レイヤー（ADR-15）**: 価格ペインにラチェット式トレンドライン（青 `#2196f3`）+ フリップマーカー（`QT BUY` 青▲ / `QT SELL` 橙▼ / STRONG は `QT STRONG` 表示）+ 前兆サークル（`QT前兆`、買い待ち=淡青●下 / 売り待ち=淡橙●上）。トグル「Q-Trend」（線+フリップ）「QT前兆」（サークル）は**既定 ON**。既存の確定マーカー（ADR-14、緑/赤）とは独立トグルで併存。マーカーは単一プラグインで3ソース（確定/QT/前兆）をトグル合成し time 昇順ソートして `setMarkers`（lightweight-charts の昇順要件）。全系列 Rust 計算（`ChartData.qtrend / qt_markers / qt_precursors`）。
- **Settings 画面 `/settings`（C）実装済み**（`frontend/app/settings/page.tsx`）: プリセット切替（保守/標準/積極、`get_presets`）+ しきい値（買い/売り/品質ゲート、スライダー）+ MTF（α 日/週/月・週足ゲート・月足修正子・有効化トグル）+ 近接度（`fresh_bars_n`・`approach_floor`・`max_dist_atr`）+ リスク（最小バー数・損切りATR倍率）+ チャート表示（`chart_bars` 初期表示ローソク足数）を編集 → **保存（`update_config`）**。
  - **設定はバックエンドに永続化**（`app_data_dir/config.json`）。`get_config`/`scan_*`/`get_chart_data`/`evaluate_model` は**アクティブ設定をロード**するため、保存した設定がスキャン・チャート双方に一貫適用される（チャートとリストのスコア一致を維持）。フロントは設定を受け渡さない。
  - 既定（未保存時）は P8 確定の **Standard** プリセット。`indicators`/`regime`/`weights` は設定画面では非編集（保存時はそのまま素通し）。`weighted` 集約方式は未実装のため近接度の集約トグルは省略。
