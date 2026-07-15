#!/usr/bin/env python3
"""Alpha Radar yfinance data-fetch sidecar (P0).

Stateless, fetch-only process. Reads one JSON request from stdin and writes one
JSON response to stdout, per the docs/04 I/O contract. Failures are returned as
structured `errors` (never raised), so one bad symbol does not abort the batch.
Caching / differential update lives on the Rust side (`data/cache.rs`).

Run (dev):  uv run --project sidecar python sidecar/fetch.py
"""

from __future__ import annotations

import json
import math
import sys
import time
from typing import Any


def _err_response(reason: str) -> dict[str, Any]:
    return {"results": [], "errors": [{"symbol": "*", "interval": "*", "reason": reason}]}


def _finite(x, default: float = 0.0) -> float:
    """Coerce to a JSON-safe finite float. yfinance can yield NaN/Inf (missing
    values, current in-progress bar); the default `json` module would emit the
    literal `NaN`/`Infinity`, which is invalid JSON the Rust side rejects."""
    try:
        v = float(x)
    except (TypeError, ValueError):
        return default
    return v if math.isfinite(v) else default


def _to_unix(ts) -> int:
    """Normalize a pandas Timestamp to UTC Unix seconds."""
    if ts.tzinfo is None:
        ts = ts.tz_localize("UTC")
    else:
        ts = ts.tz_convert("UTC")
    return int(ts.timestamp())


def _fetch_one(yf, pd, item: dict, auto_adjust: bool, retries: int = 3):
    """Fetch one (symbol, interval). Returns (candles, None) or (None, reason)."""
    symbol, interval = item["symbol"], item["interval"]
    start, end = item.get("start"), item.get("end")
    last_err = None
    for attempt in range(retries):
        try:
            kwargs: dict[str, Any] = {"interval": interval, "auto_adjust": auto_adjust}
            if start is None:
                kwargs["period"] = "max"
            else:
                kwargs["start"] = pd.to_datetime(start, unit="s", utc=True)
                if end is not None:
                    kwargs["end"] = pd.to_datetime(end, unit="s", utc=True)
            df = yf.Ticker(symbol).history(**kwargs)
            if df is None or df.empty:
                return None, "no data / delisted"
            # Drop bars with a missing OHLC value: they are unusable for the
            # indicators and would otherwise serialize as invalid-JSON `NaN`.
            ohlc_cols = [c for c in ("Open", "High", "Low", "Close") if c in df.columns]
            df = df.dropna(subset=ohlc_cols)
            if df.empty:
                return None, "no data / delisted"
            candles = []
            has_adj = "Adj Close" in df.columns
            for idx, row in df.iterrows():
                close = _finite(row["Close"])
                candles.append(
                    {
                        "ts": _to_unix(idx),
                        "open": _finite(row["Open"]),
                        "high": _finite(row["High"]),
                        "low": _finite(row["Low"]),
                        "close": close,
                        "volume": _finite(row["Volume"]),
                        # auto_adjust=True already adjusts Close; mirror it.
                        "adj_close": _finite(row["Adj Close"], close) if has_adj else close,
                    }
                )
            return candles, None
        except Exception as e:  # noqa: BLE001 - structured error, never propagate
            last_err = str(e)
            time.sleep(2**attempt)  # exponential backoff
    return None, last_err or "fetch failed"


def main() -> None:
    try:
        req = json.loads(sys.stdin.read())
    except Exception as e:  # noqa: BLE001
        json.dump(_err_response(f"bad request json: {e}"), sys.stdout)
        return

    auto_adjust = bool(req.get("auto_adjust", True))
    items = req.get("requests", [])

    try:
        import pandas as pd
        import yfinance as yf
    except Exception as e:  # noqa: BLE001
        json.dump(_err_response(f"import failed: {e}"), sys.stdout)
        return

    results, errors = [], []
    for i, item in enumerate(items):
        candles, reason = _fetch_one(yf, pd, item, auto_adjust)
        if reason is not None:
            errors.append(
                {"symbol": item["symbol"], "interval": item["interval"], "reason": reason}
            )
        else:
            results.append(
                {"symbol": item["symbol"], "interval": item["interval"], "candles": candles}
            )
        if i + 1 < len(items):
            time.sleep(0.3)  # throttle between symbols (rate-limit courtesy)

    # NOTE: output="parquet" (large universes) is a planned optimization; this
    # build always returns inline JSON.
    # `allow_nan=False` is a safety net: the Rust parser rejects `NaN`/`Infinity`
    # (not valid JSON), so rather than emit an unparseable stream we surface a
    # structured error. Candles are already coerced finite above, so this should
    # not trigger in practice.
    out = {"results": results, "errors": errors}
    try:
        payload = json.dumps(out, allow_nan=False)
    except ValueError:
        payload = json.dumps(_err_response("non-finite value in fetched data"))
    sys.stdout.write(payload)


if __name__ == "__main__":
    main()
