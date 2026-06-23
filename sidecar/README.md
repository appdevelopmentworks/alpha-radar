# sidecar/ — Python データ取得サイドカー (P0)

yfinance ラッパ。Tauri 2 の `externalBin` サイドカーとして同梱し、Rust が
stdin に JSON リクエストを渡して stdout で OHLCV(JSON/Parquet)を受け取る、
**ステートレス**な取得専用プロセス。キャッシュ・差分判定は Rust 側 (`data/cache.rs`)。

実体(`fetch.py` / `requirements.txt` / PyInstaller ビルド)は **セッション2 (P0)**
で実装する。詳細は [`docs/01-architecture.md`](../docs/01-architecture.md) の
「Python サイドカー起動モデル」と [`docs/04-data-spec.md`](../docs/04-data-spec.md)
の I/O 契約を参照。
