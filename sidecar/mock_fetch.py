#!/usr/bin/env python3
"""Offline mock of the Alpha Radar sidecar (stdlib only, no network).

Emits deterministic synthetic OHLCV for round-trip protocol tests. Symbols
starting with "BAD" return a structured error to exercise the error path.

Run:  uv run --python 3.12 python sidecar/mock_fetch.py
"""

from __future__ import annotations

import json
import math
import random
import sys

_DAY = {"1d": 86_400, "1wk": 604_800, "1mo": 2_592_000}
_BASE_TS = 1_577_836_800  # 2020-01-01T00:00:00Z


def _synth(symbol: str, interval: str, n: int = 220) -> list[dict]:
    rng = random.Random(hash((symbol, interval)) & 0xFFFFFFFF)
    step = _DAY.get(interval, 86_400)
    out, price = [], 100.0
    for i in range(n):
        o = price
        c = o * math.exp(rng.gauss(0.0004, 0.015))
        hi = max(o, c) * (1.0 + rng.uniform(0.001, 0.01))
        lo = min(o, c) * (1.0 - rng.uniform(0.001, 0.01))
        out.append(
            {
                "ts": _BASE_TS + i * step,
                "open": round(o, 4),
                "high": round(hi, 4),
                "low": round(lo, 4),
                "close": round(c, 4),
                "volume": rng.randint(1_000_000, 5_000_000),
                "adj_close": round(c, 4),
            }
        )
        price = c
    return out


def main() -> None:
    req = json.loads(sys.stdin.read())
    results, errors = [], []
    for item in req.get("requests", []):
        sym, iv = item["symbol"], item["interval"]
        if sym.upper().startswith("BAD"):
            errors.append({"symbol": sym, "interval": iv, "reason": "mock: no data"})
        else:
            results.append({"symbol": sym, "interval": iv, "candles": _synth(sym, iv)})
    json.dump({"results": results, "errors": errors}, sys.stdout)


if __name__ == "__main__":
    main()
