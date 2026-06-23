"""Generate golden-value fixtures for the Rust indicator unit tests.

Reference = TA-Lib (ADR-13, docs/07-testing.md). Deterministic and reproducible:
re-running this script regenerates byte-identical files.

Outputs (both committed):
  tests/fixtures/sample_basic_1d.csv          synthetic OHLCV input (seed 42)
  tests/fixtures/sample_basic_1d.golden.json  TA-Lib reference series

The CSV is synthetic (a seeded geometric random walk) — sufficient to pin the
numeric agreement between Rust and TA-Lib. Visual reconciliation against
TradingView on real symbols is a separate, later step (ADR-13).

Run:  uv run --project tools/golden python tools/golden/gen_golden.py
"""

from __future__ import annotations

import json
import math
import random
from pathlib import Path

import numpy as np
import pandas as pd
import talib

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[1]  # tools/golden -> repo root
FIX_DIR = REPO_ROOT / "tests" / "fixtures"
CSV_PATH = FIX_DIR / "sample_basic_1d.csv"
GOLDEN_PATH = FIX_DIR / "sample_basic_1d.golden.json"

N_BARS = 220
SEED = 42
BASE_TS = 1_577_836_800  # 2020-01-01T00:00:00Z
DAY = 86_400

# Periods exercised by the golden tests (match config.rs defaults).
PERIODS = {"sma": 20, "ema": [20, 50], "rsi": 14, "atr": 14, "linreg": 20}


def generate_csv() -> pd.DataFrame:
    """Deterministic synthetic OHLCV. Guarantees high >= max(open, close) and
    low <= min(open, close) even after rounding to 4 decimals."""
    rng = random.Random(SEED)
    rows = []
    prev_close = 100.0
    for i in range(N_BARS):
        o = prev_close
        c = o * math.exp(rng.gauss(0.0004, 0.015))
        hi = max(o, c) * (1.0 + rng.uniform(0.0008, 0.012))
        lo = min(o, c) * (1.0 - rng.uniform(0.0008, 0.012))
        rows.append(
            {
                "ts": BASE_TS + i * DAY,
                "open": round(o, 4),
                "high": round(hi, 4),
                "low": round(lo, 4),
                "close": round(c, 4),
                "volume": rng.randint(1_000_000, 5_000_000),
            }
        )
        prev_close = c
    df = pd.DataFrame(rows, columns=["ts", "open", "high", "low", "close", "volume"])
    FIX_DIR.mkdir(parents=True, exist_ok=True)
    df.to_csv(CSV_PATH, index=False, lineterminator="\n")
    return df


def to_json_list(arr: np.ndarray) -> list[float | None]:
    """TA-Lib emits NaN during warm-up; serialize those as JSON null so the Rust
    side can compare them to `None`."""
    return [None if math.isnan(x) else float(x) for x in arr]


def clean(xs) -> list:
    """Serialize a Python/NumPy sequence that may contain None/NaN to JSON,
    preserving ints (used for direction flags)."""
    out = []
    for x in xs:
        if x is None or (isinstance(x, float) and math.isnan(x)):
            out.append(None)
        elif isinstance(x, (bool, int)) and not isinstance(x, float):
            out.append(int(x))
        else:
            out.append(float(x))
    return out


def supertrend_ref(high, low, close, period, mult):
    """Reference Supertrend mirroring src-tauri indicators::trend::supertrend,
    reusing talib.ATR (== the Rust `atr` primitive)."""
    n = len(close)
    atrv = talib.ATR(high, low, close, period)
    line: list = [None] * n
    direction: list = [None] * n
    start = next((i for i in range(n) if not math.isnan(atrv[i])), None)
    if start is None:
        return line, direction
    hl2 = (high + low) / 2.0
    fu_prev = hl2[start] + mult * atrv[start]
    fl_prev = hl2[start] - mult * atrv[start]
    d_prev = 1
    line[start], direction[start] = fl_prev, 1
    for i in range(start + 1, n):
        a = atrv[i]
        bu = hl2[i] + mult * a
        bl = hl2[i] - mult * a
        fu = bu if (bu < fu_prev or close[i - 1] > fu_prev) else fu_prev
        fl = bl if (bl > fl_prev or close[i - 1] < fl_prev) else fl_prev
        if d_prev == 1:
            d = -1 if close[i] < fl else 1
        else:
            d = 1 if close[i] > fu else -1
        line[i] = fl if d == 1 else fu
        direction[i] = d
        fu_prev, fl_prev, d_prev = fu, fl, d
    return line, direction


def ichimoku_ref(high, low, tenkan_p, kijun_p, senkou_b_p, disp):
    """Reference Ichimoku mirroring src-tauri indicators::trend::ichimoku.
    Spans are projected forward `disp` bars (aligned to the current bar)."""
    hs, ls = pd.Series(high), pd.Series(low)

    def midline(p):
        return (hs.rolling(p).max() + ls.rolling(p).min()) / 2.0

    tenkan, kijun = midline(tenkan_p), midline(kijun_p)
    raw_a = (tenkan + kijun) / 2.0
    raw_b = midline(senkou_b_p)
    return tenkan, kijun, raw_a.shift(disp), raw_b.shift(disp)


def true_range_np(high, low, close):
    """True range with tr[0] = high[0]-low[0] (mirrors Rust `true_range`)."""
    n = len(close)
    tr = np.empty(n)
    tr[0] = high[0] - low[0]
    for i in range(1, n):
        tr[i] = max(
            high[i] - low[i],
            abs(high[i] - close[i - 1]),
            abs(low[i] - close[i - 1]),
        )
    return tr


def squeeze_ref(high, low, close, length, mult_bb, mult_kc):
    """Reference Squeeze Momentum (LazyBear) mirroring
    src-tauri indicators::momentum::squeeze_momentum."""
    n = len(close)
    cs = pd.Series(close)
    basis = cs.rolling(length).mean()
    dev = cs.rolling(length).std(ddof=0)  # population std
    upper_bb, lower_bb = basis + mult_bb * dev, basis - mult_bb * dev
    rangema = pd.Series(true_range_np(high, low, close)).rolling(length).mean()
    upper_kc, lower_kc = basis + mult_kc * rangema, basis - mult_kc * rangema

    on, off = [], []
    for i in range(n):
        if math.isnan(dev[i]) or math.isnan(rangema[i]):
            on.append(None)
            off.append(None)
        else:
            on.append(int(lower_bb[i] > lower_kc[i] and upper_bb[i] < upper_kc[i]))
            off.append(int(lower_bb[i] < lower_kc[i] and upper_bb[i] > upper_kc[i]))

    hh = pd.Series(high).rolling(length).max()
    ll = pd.Series(low).rolling(length).min()
    source = cs - ((hh + ll) / 2.0 + basis) / 2.0
    val = np.full(n, np.nan)
    first = source.first_valid_index()
    if first is not None:
        val[first:] = talib.LINEARREG(source.iloc[first:].to_numpy(), length)
    return val, on, off


def ema_then_ema_np(values, p1, p2):
    """talib.EMA(p2) over the warmed-up talib.EMA(p1) (mirrors Rust ema_then_ema)."""
    e1 = talib.EMA(values, p1)
    out = np.full(len(values), np.nan)
    start = next((i for i in range(len(e1)) if not math.isnan(e1[i])), None)
    if start is not None:
        out[start:] = talib.EMA(e1[start:], p2)
    return out


def tsi_ref(close, long, short):
    """Reference TSI mirroring src-tauri indicators::momentum::tsi."""
    m = np.diff(close)  # m[k] = close[k+1]-close[k]
    dm = ema_then_ema_np(m, long, short)
    dam = ema_then_ema_np(np.abs(m), long, short)
    out = np.full(len(close), np.nan)
    for k in range(len(m)):
        if not math.isnan(dm[k]) and not math.isnan(dam[k]):
            out[k + 1] = 100.0 * dm[k] / dam[k] if dam[k] != 0 else 0.0
    return out


def ma_zscore_ref(close, sma_period, std_period):
    """Reference MA z-score mirroring
    src-tauri indicators::mean_reversion::ma_zscore."""
    n = len(close)
    mid = pd.Series(close).rolling(sma_period).mean().to_numpy()
    dev = close - mid
    out = np.full(n, np.nan)
    start = next((i for i in range(n) if not math.isnan(dev[i])), None)
    if start is None:
        return out
    valid = dev[start:]
    sd = pd.Series(valid).rolling(std_period).std(ddof=0).to_numpy()  # population
    for k in range(len(valid)):
        if not math.isnan(sd[k]) and sd[k] > 0:
            out[start + k] = valid[k] / sd[k]
    return out


def choppiness_ref(high, low, close, period):
    """Reference Choppiness Index mirroring
    src-tauri indicators::volatility::choppiness."""
    n = len(close)
    tr = true_range_np(high, low, close)
    hh = pd.Series(high).rolling(period).max().to_numpy()
    ll = pd.Series(low).rolling(period).min().to_numpy()
    sum_tr = pd.Series(tr).rolling(period).sum().to_numpy()
    out = np.full(n, np.nan)
    log_n = math.log10(period)
    for i in range(n):
        if not (math.isnan(sum_tr[i]) or math.isnan(hh[i]) or math.isnan(ll[i])):
            rng = hh[i] - ll[i]
            if rng > 0 and sum_tr[i] > 0:
                out[i] = 100.0 * math.log10(sum_tr[i] / rng) / log_n
    return out


def main() -> None:
    generate_csv()
    # Recompute from the written CSV so TA-Lib operates on the exact rounded
    # values the Rust test will parse.
    df = pd.read_csv(CSV_PATH)
    close = df["close"].to_numpy(dtype="float64")
    high = df["high"].to_numpy(dtype="float64")
    low = df["low"].to_numpy(dtype="float64")

    # Reference series for indicators TA-Lib does not provide (mirror the Rust
    # algorithm exactly; Supertrend reuses talib.ATR == the Rust `atr`).
    st_line, st_dir = supertrend_ref(high, low, close, 10, 3.0)
    ichi_t, ichi_k, ichi_a, ichi_b = ichimoku_ref(high, low, 9, 26, 52, 26)
    # MACD composed from the SMA-seeded talib.EMA primitive (= TradingView /
    # the Rust `ema`). talib.MACD() seeds its signal EMA differently (a TA-Lib
    # internal quirk), so we build the signal as EMA9 of the warmed-up macd line.
    macd_line = talib.EMA(close, 12) - talib.EMA(close, 26)
    macd_signal = np.full(len(close), np.nan)
    _ml_start = next(
        (i for i in range(len(macd_line)) if not math.isnan(macd_line[i])), None
    )
    if _ml_start is not None:
        macd_signal[_ml_start:] = talib.EMA(macd_line[_ml_start:], 9)
    macd_hist = macd_line - macd_signal
    sqz_val, sqz_on, sqz_off = squeeze_ref(high, low, close, 20, 2.0, 1.5)
    tsi_vals = tsi_ref(close, 25, 13)
    bb_up, bb_mid, bb_low = talib.BBANDS(close, 20, 2.0, 2.0)
    pct_b = (close - bb_low) / (bb_up - bb_low)
    bb_width = (bb_up - bb_low) / bb_mid
    ma_z = ma_zscore_ref(close, 20, 100)
    kc_mid = talib.EMA(close, 20)
    kc_atr = talib.ATR(high, low, close, 20)
    kc_up, kc_low = kc_mid + 2.0 * kc_atr, kc_mid - 2.0 * kc_atr
    chop = choppiness_ref(high, low, close, 14)

    golden = {
        "meta": {
            "source": "TA-Lib",
            "talib_version": talib.__version__,
            "bars": int(len(close)),
            "seed": SEED,
            "periods": PERIODS,
            "note": "Synthetic deterministic OHLCV. Numeric reference per ADR-13.",
        },
        # --- base primitives ---
        "sma_20": to_json_list(talib.SMA(close, 20)),
        "ema_20": to_json_list(talib.EMA(close, 20)),
        "ema_50": to_json_list(talib.EMA(close, 50)),
        "rsi_14": to_json_list(talib.RSI(close, 14)),
        "atr_14": to_json_list(talib.ATR(high, low, close, 14)),
        "linreg_20": to_json_list(talib.LINEARREG(close, 20)),
        # --- trend ---
        "ema_200": to_json_list(talib.EMA(close, 200)),
        "plus_di_14": to_json_list(talib.PLUS_DI(high, low, close, 14)),
        "minus_di_14": to_json_list(talib.MINUS_DI(high, low, close, 14)),
        "adx_14": to_json_list(talib.ADX(high, low, close, 14)),
        "st_line": clean(st_line),
        "st_dir": clean(st_dir),
        "ichi_tenkan": clean(ichi_t.to_numpy()),
        "ichi_kijun": clean(ichi_k.to_numpy()),
        "ichi_senkou_a": clean(ichi_a.to_numpy()),
        "ichi_senkou_b": clean(ichi_b.to_numpy()),
        # --- momentum ---
        "macd_line": to_json_list(macd_line),
        "macd_signal": to_json_list(macd_signal),
        "macd_hist": to_json_list(macd_hist),
        "sqz_val": to_json_list(sqz_val),
        "sqz_on": clean(sqz_on),
        "sqz_off": clean(sqz_off),
        "tsi_25_13": to_json_list(tsi_vals),
        # --- mean reversion ---
        "rsi_2": to_json_list(talib.RSI(close, 2)),
        "pct_b_20": to_json_list(pct_b),
        "willr_14": to_json_list(talib.WILLR(high, low, close, 14)),
        "ma_zscore": to_json_list(ma_z),
        # --- volatility ---
        "bb_upper_20": to_json_list(bb_up),
        "bb_middle_20": to_json_list(bb_mid),
        "bb_lower_20": to_json_list(bb_low),
        "bb_width_20": to_json_list(bb_width),
        "kc_upper_20": to_json_list(kc_up),
        "kc_middle_20": to_json_list(kc_mid),
        "kc_lower_20": to_json_list(kc_low),
        "chop_14": to_json_list(chop),
    }

    with open(GOLDEN_PATH, "w", newline="\n", encoding="utf-8") as f:
        json.dump(golden, f, indent=2)
        f.write("\n")

    print(f"wrote {CSV_PATH.relative_to(REPO_ROOT)} ({len(close)} bars)")
    print(f"wrote {GOLDEN_PATH.relative_to(REPO_ROOT)} (TA-Lib {talib.__version__})")


if __name__ == "__main__":
    main()
