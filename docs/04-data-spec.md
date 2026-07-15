# 04. データ仕様

CSV 入力・OHLCV モデル・SQLite スキーマ・差分更新・Python サイドカー I/O 契約・yfinance 仕様。`src-tauri/src/data/` と `sidecar/fetch.py`。

## CSV 入力スキーマ

| 列名 | 型 | 必須 | 例 | 備考 |
|---|---|---|---|---|
| `symbol` | string | ✓ | `7974.T`, `AAPL`, `BTC-USD` | **yfinance 形式**（後述）。先頭列＝コード（後述の別名/フォールバック対応） |
| `name` | string | - | `任天堂` | 省略時は yfinance から補完 |
| `asset_class` | enum | - | `equity` / `crypto` | 省略時はサフィックス等から推定 |
| `weight` | float | - | `1.0` | ポートフォリオ加重表示用（任意） |

- ヘッダ行必須。区切りはカンマ。
- **文字コード**: UTF-8（BOM許容）に加え **Shift_JIS / CP932 も自動判別**（`data::csv::decode_bytes`：UTF-8 BOM → 妥当な UTF-8 → Shift_JIS フォールバック）。日本の Excel が既定で出力する SJIS の CSV をそのまま読める。
- **列名の柔軟対応**: コード列はヘッダ名が `symbol` でなくても認識する。別名（大小無視）= `code` / `ticker` / `コード` / `ティッカー` / `銘柄` / `銘柄コード` / `証券コード`。**いずれも一致しない場合は先頭列をコードとして採用**（要求: 「CSVの一列目がコード」）。`name`（`名称`/`名前`/`銘柄名`/`会社名`/`社名`）・`asset_class`（`種別`/`資産クラス`）・`weight`（`ウェイト`/`比率`/`重み`）も別名対応。
- 重複 `symbol` は1回だけ取得・スコア（最初の行のメタを採用）。
- パースは `csv` crate。1行の不正は `RowError` に積みスキャン継続。

### yfinance シンボル形式
- 日本株（東証）: 4桁コード + `.T`（例 `7974.T` = 任天堂）。
- 米株: ティッカーそのまま（例 `AAPL`）。
- 暗号資産: Yahoo 形式 `BTC-USD`, `ETH-USD`（**日足/週足のみ。取引所ネイティブ・3D足不可**）。
- `asset_class` 推定: `-USD`/既知クリプト記号 → crypto、`.T` 等の取引所サフィックス or 純ティッカー → equity。
- **日本株コードの正規化**: 裸の4文字 TSE コード（`7974` や 2024年以降の英数 `130A`）は自動で `.T` を付与（`data::universe::normalize_symbol`）。よって `7974` でも `7974.T` でも同一銘柄として認識・キャッシュ共有される（CSV / テキスト入力の両方に適用）。4文字英字ティッカー（`AAPL` `MSFT`）は対象外。

## OHLCV 内部モデル

```rust
struct Candle {
    ts: i64,        // UNIX秒（バー確定時刻, UTC）
    open: f64, high: f64, low: f64, close: f64,
    volume: f64,
    adj_close: f64, // auto_adjust 後は close と一致しうる（保持）
}
enum Tf { Daily, Weekly, Monthly }   // yfinance: "1d"/"1wk"/"1mo"
```

## SQLite スキーマ（DDL）

```sql
CREATE TABLE IF NOT EXISTS symbols (
    symbol       TEXT PRIMARY KEY,
    name         TEXT,
    asset_class  TEXT NOT NULL,        -- 'equity' | 'crypto'
    last_seen    INTEGER               -- 最終スキャン時刻
);

CREATE TABLE IF NOT EXISTS ohlcv (
    symbol  TEXT NOT NULL,
    tf      TEXT NOT NULL,             -- '1d' | '1wk' | '1mo'
    ts      INTEGER NOT NULL,          -- バー確定時刻(UTC秒)
    open REAL, high REAL, low REAL, close REAL, volume REAL, adj_close REAL,
    PRIMARY KEY (symbol, tf, ts)
);
CREATE INDEX IF NOT EXISTS idx_ohlcv_sym_tf_ts ON ohlcv(symbol, tf, ts);

CREATE TABLE IF NOT EXISTS scores (
    symbol            TEXT NOT NULL,
    scanned_at        INTEGER NOT NULL,
    regime            TEXT,
    score_final       REAL,
    score_daily REAL, score_weekly REAL, score_monthly REAL,
    s_trend REAL, s_momentum REAL, s_mean_reversion REAL,
    signal_state      TEXT,
    direction         TEXT,
    proximity_score   REAL,
    bars_since_trigger INTEGER,
    actionability     REAL,
    atr               REAL,
    suggested_stop    REAL,
    marker_hit_rate   REAL,     -- FR-8 マーカー規則の日足バックテスト順行率 [0,1]
    marker_samples    INTEGER,  -- 的中率の標本数（判定窓が取れたマーカー数）
    PRIMARY KEY (symbol, scanned_at)
);
CREATE INDEX IF NOT EXISTS idx_scores_scan ON scores(scanned_at);

CREATE TABLE IF NOT EXISTS scan_runs (
    scanned_at  INTEGER PRIMARY KEY,
    csv_path    TEXT,
    config_json TEXT,                  -- ScanConfig スナップショット
    n_symbols   INTEGER,
    n_errors    INTEGER
);
```

> 設定（重み・閾値）はスキャン時に `scan_runs.config_json` へスナップショット。再現性のため。

## 差分更新ロジック（`data/cache.rs`）

```
for symbol in universe:
  for tf in [1d, 1wk, 1mo]:
    last_ts = SELECT max(ts) FROM ohlcv WHERE symbol=? AND tf=?
    if last_ts is None:
        request full history (period="max")
    elif now - last_ts > one_bar(tf):
        request start = last_ts (重複バーは upsert で吸収)
    else:
        skip (キャッシュ新鮮)
# 不足分のみをまとめてサイドカーへバッチ要求
```

- upsert: `INSERT ... ON CONFLICT(symbol,tf,ts) DO UPDATE`（最新バーの再取得で確定値に更新）。
- 「新鮮」の判定は TF 依存（日足=当日/前営業日、週足=今週、月足=今月）。

## Python サイドカー I/O 契約（`sidecar/fetch.py`）

**起動:** Tauri `externalBin`。Rust が stdin に JSON、サイドカーは stdout に JSON（大規模時は Parquet ファイルパス）を返す。ステートレス・取得のみ。

### リクエスト（Rust → サイドカー, stdin JSON）
```json
{
  "requests": [
    { "symbol": "7974.T", "interval": "1d",  "start": 1719100800, "end": null },
    { "symbol": "7974.T", "interval": "1wk", "start": null,       "end": null },
    { "symbol": "BTC-USD","interval": "1mo", "start": null,       "end": null }
  ],
  "auto_adjust": true,
  "output": "json"          // または "parquet"（その場合は path を返す）
}
```
- `start`/`end` は UNIX 秒。`start=null` は `period="max"`。
- 複数銘柄・複数足を **1プロセスでバッチ**（yfinance の `download` をまとめて使用）。

### レスポンス（サイドカー → Rust, stdout JSON）
```json
{
  "results": [
    {
      "symbol": "7974.T", "interval": "1d",
      "candles": [
        { "ts": 1719100800, "open": 7030, "high": 7080, "low": 7001,
          "close": 7079, "volume": 1234500, "adj_close": 7079 }
      ]
    }
  ],
  "errors": [
    { "symbol": "XXXX.T", "interval": "1d", "reason": "no data / delisted" }
  ]
}
```
- エラーは**構造化**で返す（例外で全体を落とさない）。Rust が `RowError` に変換。
- **非有限値の扱い**: yfinance は欠損や当日未確定バーで `NaN`/`Inf` を返すことがある。Python 既定の `json.dump` はこれを **`NaN`/`Infinity` リテラル**として出力するが、これは不正な JSON で Rust の `serde_json` が拒否する（`expected value` エラー）。そのためサイドカーは **OHLC が欠損する足を `dropna` で除外**し、残る `volume`/`adj_close` の非有限値を有限へ丸め、最終出力は `json.dumps(..., allow_nan=False)`（すり抜け時は構造化エラーにフォールバック）で**必ず正当な JSON**を返す。
- `output="parquet"` の場合: `{ "parquet_path": "/tmp/alpha-radar-xxxx.parquet", "errors": [...] }` を返し Rust 側で読む（大規模ユニバース向け）。

## yfinance 仕様・運用

- 使用インターバル: `1d` / `1wk` / `1mo`（いずれも履歴ほぼ無制限）。日中足は v1 不使用。
- `auto_adjust=True`（splits/dividends 調整）。`repair` は必要に応じ有効化。
- **レート制限対策:** バッチ `download`、リクエスト間スロットリング、失敗時の指数バックオフ + 再試行（例 最大3回、`2^n` 秒）。cookie/crumb 認証は yfinance に委譲。
- 取得失敗率・レイテンシを許容する設計（キャッシュ前提）。サイドカーは取得結果と errors を返すのみ、リトライ方針は Rust 側が制御してもよい。
- 銘柄名・メタ（`name`）は必要時に `Ticker.info` で補完（任意・キャッシュ）。

## DuckDB（任意）

分析クエリ（評価ハーネスのリターン集計・層別など）で SQLite の `ohlcv`/`scores` を読む用途に限定使用。書き込みは SQLite を真実源とする。

---

## 実装状況（P0 + P4 完了）

- **P0 サイドカー**: `sidecar/fetch.py`（uv 管理・yfinance、stdin/stdout JSON、`auto_adjust`、リトライ/指数バックオフ、構造化エラー）。実データで round-trip 検証済み（AAPL 1d / 7974.T 1wk / BTC-USD 1mo / 不正銘柄=errors）。`sidecar/mock_fetch.py` はネットワーク非依存のモック。Rust 側は `src-tauri/src/data/sidecar.rs`（`SidecarClient`、コマンド注入式）。
  - 現状は**1プロセス内で銘柄ごとに逐次取得**（プロセスレベルのバッチは満たす）。`yf.download` のまとめ取得・`output="parquet"`・`Ticker.info` 名称補完は将来最適化。
  - **パッケージング（PyInstaller バンドル + `tauri.conf.json` の `externalBin` 登録）は未了**。dev は `uv run --project sidecar python sidecar/fetch.py` で起動（`SidecarClient::dev_uv`）。
- **P4 取込/キャッシュ/スキャン**: `data/csv.rs`（寛容パース・行エラー）、`data/cache.rs`（rusqlite bundled、上記 DDL、差分更新 `FetchDecision::{Full,From,Skip}`、OHLCV upsert、`scores`/`scan_runs` 保存。**追加列は `Cache::open` 内の加算的マイグレーション**（`pragma_table_info` で欠損列を `ALTER TABLE ADD COLUMN` — 既存 DB は旧行 NULL のまま利用継続）で反映）、`data/universe.rs`。`commands/scan_universe`（差分判定→1バッチ取得→upsert→メモリロード→**rayon 並列スコアリング**→`SymbolScore`→保存→`ScanResult`）。
  - キャッシュ（rusqlite `Connection`）は `!Sync` のため、DB アクセスは逐次・純粋スコアリングのみ rayon 並列。
  - `min_bars`（既定60）未満は `RowError`、上位足欠落は MTF ゲートで degrade（`docs/03`）。設定は `scan_runs.config_json` にスナップショット（ADR-10）。
- **内部 `Candle`**: docs の `adj_close` を保持（`auto_adjust` 後は `close` と一致）。`Tf` の文字列は `1d/1wk/1mo`。
