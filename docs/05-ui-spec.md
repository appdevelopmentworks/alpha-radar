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
- **マーカー:** `BUY @ price` / `SELL @ price`（series-markers, `zOrder` 制御）。画像1の◇相当。
- 指標値は **`get_chart_data` の Rust 計算結果を描画**（フロントで再計算しない＝リストと完全一致）。
- 足切替（日足/週足/月足）。

## (C) Settings（`/settings`）

- **プリセット:** Conservative / Standard / Aggressive（閾値・重みのセット）。
- **閾値:** buy/sell（既定 ±40）スライダー。
- **MTF:** α（日足0.55 / 週足0.30 / 月足0.15）、weekly_gate（整合1.0 / 中立0.8 / 逆行0.4）、monthly_mod（整合1.1 / 逆行0.85 / **無効化トグル**）。
- **近接度:** `fresh_bars_n`（既定1–2）、`approach_floor`、`max_dist_atr`、集約方式（weighted/max）。
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
  - **入力経路2つ**: ① ティッカー直接入力テキストボックス（カンマ/スペース/改行区切り、`Ctrl+Enter` で実行）→ `scan_symbols` コマンド（`parse_symbols_str` で正規化）。② CSV ドロップ/選択 → `scan_universe`（ファイルパス）。Tauri ネイティブ drag-drop（`onDragDropEvent`＝パス取得）+ `tauri-plugin-dialog` の `open()`。
  - **ナビ保持**: スキャン結果と config を `ScanProvider`（layout の React Context）に保持し、**リスト⇔チャートを行き来しても再スキャン不要**。
  - **進捗**: 現状はスキャン中インジケータのみ。`n/N` の逐次進捗は Tauri イベントのストリーミングが必要（将来）。
  - **列ヘッダーソート**: 各列ヘッダー（銘柄/状態/近接度/方向スコア/レジーム/経過/ATR/損切り）をクリックで並べ替え。同じ列を再クリックで昇順/降順トグル、アクティブ列に ▲/▼ 表示。数値列は既定降順・銘柄名は昇順、`null` は常に末尾。状態は鮮度ランク（Triggered>Primed>Active>Neutral）、レジームは強度ランク（上昇>下降>レンジ>転換）でソート。ソートは買い/売り/中立の各ブロック内に適用（方向2ブロック構造は維持）。**既定は actionability 降順**（ヘッダーには無い軸＝列クリックで上書き）。`内訳` は単一値でないため非ソート列。旧ソート用ドロップダウンはヘッダーソートに統合し撤去。
- **P6 Chart**（`frontend/app/chart/page.tsx`、`components/MultiPaneChart|MtfSummary`、Rust `commands/get_chart_data`）: lightweight-charts v5 の4ペイン（価格+EMAリボン/Supertrend/一目+BUY/SELLマーカー / MACD 4色ヒスト+線 / Squeeze 4色 / 合成スコア+しきい値線）+ MTF サマリー + 足切替。**全系列は Rust 計算**でリストのスコアと一致（ADR-06）。`attributionLogo` 有効。
  - **ルーティング**: 静的エクスポート（`output: 'export'`）が任意 symbol の動的ルートを生成できないため、`/chart/[symbol]` ではなく **`/chart?symbol=` クエリ方式**（`useSearchParams` + `Suspense`）。
  - **表示トグル**: チャート上部にチェックボックス（EMAリボン / Supertrend / 一目 / MACD / Squeeze / 売買マーカー）。既定は一目均衡表 OFF（最も線が多いため）。**チャート生成（`data` 依存）と表示切替（`visible` 依存）の useEffect を分離**し、トグル時は再生成せず各シリーズの `applyOptions({visible})` のみ — チャートが**リサイズ／再描画されない**。ペインは固定4段構成のため、MACD/Squeeze を OFF にするとそのペインは空のまま残る（リサイズを起こさないための割り切り）。
  - **初期ウィンドウサイズ**: 1200×900（`tauri.conf.json` の `app.windows[0]`）。
  - スクイーズの on/off ドット行は未実装（val 4色ヒストで代替、将来追加）。
- **Settings 画面 `/settings`（C）実装済み**（`frontend/app/settings/page.tsx`）: プリセット切替（保守/標準/積極、`get_presets`）+ しきい値（買い/売り/品質ゲート、スライダー）+ MTF（α 日/週/月・週足ゲート・月足修正子・有効化トグル）+ 近接度（`fresh_bars_n`・`approach_floor`・`max_dist_atr`）+ リスク（最小バー数・損切りATR倍率）を編集 → **保存（`update_config`）**。
  - **設定はバックエンドに永続化**（`app_data_dir/config.json`）。`get_config`/`scan_*`/`get_chart_data`/`evaluate_model` は**アクティブ設定をロード**するため、保存した設定がスキャン・チャート双方に一貫適用される（チャートとリストのスコア一致を維持）。フロントは設定を受け渡さない。
  - 既定（未保存時）は P8 確定の **Standard** プリセット。`indicators`/`regime`/`weights` は設定画面では非編集（保存時はそのまま素通し）。`weighted` 集約方式は未実装のため近接度の集約トグルは省略。
