//! Golden-value tests for the base indicator primitives (P1).
//!
//! Reference values are TA-Lib output captured in
//! `tests/fixtures/sample_basic_1d.golden.json`. Regenerate both fixtures with:
//!   `uv run --project tools/golden python tools/golden/gen_golden.py`
//! See docs/07-testing.md and ADR-13. An indicator is not "done" without a
//! green golden test here.

use std::path::PathBuf;

use alpha_radar_lib::indicators::mean_reversion::{
    connors_rsi2, ma_zscore, percent_b_default, williams_r,
};
use alpha_radar_lib::indicators::momentum::{macd, squeeze_momentum, tsi};
use alpha_radar_lib::indicators::trend::{adx_dmi, ichimoku, qtrend, supertrend};
use alpha_radar_lib::indicators::volatility::{bb_width, bollinger, choppiness, keltner};
use alpha_radar_lib::indicators::{atr, ema, linreg, rsi, sma};
use serde::Deserialize;

/// Tolerance for f64 agreement with TA-Lib. The algorithms are identical, so
/// differences are pure floating-point summation order (well under this).
const TOL: f64 = 1e-6;

#[derive(Deserialize)]
struct Golden {
    sma_20: Vec<Option<f64>>,
    ema_20: Vec<Option<f64>>,
    ema_50: Vec<Option<f64>>,
    rsi_14: Vec<Option<f64>>,
    atr_14: Vec<Option<f64>>,
    linreg_20: Vec<Option<f64>>,
    // trend
    ema_200: Vec<Option<f64>>,
    plus_di_14: Vec<Option<f64>>,
    minus_di_14: Vec<Option<f64>>,
    adx_14: Vec<Option<f64>>,
    st_line: Vec<Option<f64>>,
    st_dir: Vec<Option<i64>>,
    // Q-Trend: p=50 primary (many flips on the 220-bar fixture), p=200
    // secondary (production default; pins the seed convention).
    qt_line_50: Vec<Option<f64>>,
    qt_dir_50: Vec<Option<i64>>,
    qt_flip_50: Vec<Option<i64>>,
    qt_strong_50: Vec<i64>,
    qt_dist_50: Vec<Option<f64>>,
    qt_line_200: Vec<Option<f64>>,
    qt_dir_200: Vec<Option<i64>>,
    qt_flip_200: Vec<Option<i64>>,
    ichi_tenkan: Vec<Option<f64>>,
    ichi_kijun: Vec<Option<f64>>,
    ichi_senkou_a: Vec<Option<f64>>,
    ichi_senkou_b: Vec<Option<f64>>,
    // momentum
    macd_line: Vec<Option<f64>>,
    macd_signal: Vec<Option<f64>>,
    macd_hist: Vec<Option<f64>>,
    sqz_val: Vec<Option<f64>>,
    sqz_on: Vec<Option<i64>>,
    sqz_off: Vec<Option<i64>>,
    tsi_25_13: Vec<Option<f64>>,
    // mean reversion
    rsi_2: Vec<Option<f64>>,
    pct_b_20: Vec<Option<f64>>,
    willr_14: Vec<Option<f64>>,
    ma_zscore: Vec<Option<f64>>,
    // volatility
    bb_upper_20: Vec<Option<f64>>,
    bb_middle_20: Vec<Option<f64>>,
    bb_lower_20: Vec<Option<f64>>,
    bb_width_20: Vec<Option<f64>>,
    kc_upper_20: Vec<Option<f64>>,
    kc_middle_20: Vec<Option<f64>>,
    kc_lower_20: Vec<Option<f64>>,
    chop_14: Vec<Option<f64>>,
}

struct Ohlcv {
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures")
}

fn load_csv() -> Ohlcv {
    let path = fixtures_dir().join("sample_basic_1d.csv");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let (mut open, mut high, mut low, mut close) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    // Header: ts,open,high,low,close,volume
    for line in text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let cols: Vec<&str> = line.split(',').collect();
        open.push(cols[1].parse().expect("parse open"));
        high.push(cols[2].parse().expect("parse high"));
        low.push(cols[3].parse().expect("parse low"));
        close.push(cols[4].parse().expect("parse close"));
    }
    Ohlcv {
        open,
        high,
        low,
        close,
    }
}

fn load_golden() -> Golden {
    let path = fixtures_dir().join("sample_basic_1d.golden.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).expect("parse golden json")
}

/// Assert two `Option`-series match: `None` (warm-up) positions must line up
/// exactly, and concrete values must agree within [`TOL`].
fn assert_series_close(got: &[Option<f64>], want: &[Option<f64>], name: &str) {
    assert_eq!(got.len(), want.len(), "{name}: length mismatch");
    for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
        match (g, w) {
            (None, None) => {}
            (Some(a), Some(b)) => assert!(
                (a - b).abs() <= TOL,
                "{name}[{i}]: got {a}, want {b} (|Δ|={})",
                (a - b).abs()
            ),
            (Some(a), None) => panic!("{name}[{i}]: got Some({a}), want None (warm-up mismatch)"),
            (None, Some(b)) => panic!("{name}[{i}]: got None, want Some({b}) (warm-up mismatch)"),
        }
    }
}

/// Compare an `i8` direction series to integer golden values.
fn assert_dir_eq(got: &[Option<i8>], want: &[Option<i64>], name: &str) {
    assert_eq!(got.len(), want.len(), "{name}: length mismatch");
    for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
        assert_eq!(g.map(i64::from), *w, "{name}[{i}]");
    }
}

/// Compare a `bool` flag series to 0/1 integer golden values.
fn assert_flag_eq(got: &[Option<bool>], want: &[Option<i64>], name: &str) {
    assert_eq!(got.len(), want.len(), "{name}: length mismatch");
    for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
        assert_eq!(g.map(i64::from), *w, "{name}[{i}]");
    }
}

#[test]
fn primitives_match_talib_golden() {
    let data = load_csv();
    let g = load_golden();

    assert_series_close(&sma(&data.close, 20), &g.sma_20, "sma_20");
    assert_series_close(&ema(&data.close, 20), &g.ema_20, "ema_20");
    assert_series_close(&ema(&data.close, 50), &g.ema_50, "ema_50");
    assert_series_close(&rsi(&data.close, 14), &g.rsi_14, "rsi_14");
    assert_series_close(
        &atr(&data.high, &data.low, &data.close, 14),
        &g.atr_14,
        "atr_14",
    );
    assert_series_close(&linreg(&data.close, 20), &g.linreg_20, "linreg_20");
}

#[test]
fn trend_match_talib_golden() {
    let d = load_csv();
    let g = load_golden();

    let dmi = adx_dmi(&d.high, &d.low, &d.close, 14);
    assert_series_close(&dmi.plus_di, &g.plus_di_14, "plus_di_14");
    assert_series_close(&dmi.minus_di, &g.minus_di_14, "minus_di_14");
    assert_series_close(&dmi.adx, &g.adx_14, "adx_14");

    assert_series_close(&ema(&d.close, 200), &g.ema_200, "ema_200");

    let st = supertrend(&d.high, &d.low, &d.close, 10, 3.0);
    assert_series_close(&st.line, &g.st_line, "st_line");
    assert_dir_eq(&st.dir, &g.st_dir, "st_dir");

    let ich = ichimoku(&d.high, &d.low, 9, 26, 52, 26);
    assert_series_close(&ich.tenkan, &g.ichi_tenkan, "ichi_tenkan");
    assert_series_close(&ich.kijun, &g.ichi_kijun, "ichi_kijun");
    assert_series_close(&ich.senkou_a, &g.ichi_senkou_a, "ichi_senkou_a");
    assert_series_close(&ich.senkou_b, &g.ichi_senkou_b, "ichi_senkou_b");
}

#[test]
fn qtrend_matches_golden() {
    let d = load_csv();
    let g = load_golden();

    let qt = qtrend(&d.open, &d.high, &d.low, &d.close, 50, 14, 1.0);
    assert_series_close(&qt.line, &g.qt_line_50, "qt_line_50");
    assert_dir_eq(&qt.dir, &g.qt_dir_50, "qt_dir_50");
    assert_dir_eq(&qt.flip, &g.qt_flip_50, "qt_flip_50");
    assert_series_close(&qt.dist_flip_atr, &g.qt_dist_50, "qt_dist_50");
    for (i, (got, want)) in qt.strong.iter().zip(g.qt_strong_50.iter()).enumerate() {
        assert_eq!(i64::from(*got), *want, "qt_strong_50[{i}]");
    }

    // Production default p=200: pins the seed convention (first flip lands on
    // the seed bar itself in this fixture).
    let qt200 = qtrend(&d.open, &d.high, &d.low, &d.close, 200, 14, 1.0);
    assert_series_close(&qt200.line, &g.qt_line_200, "qt_line_200");
    assert_dir_eq(&qt200.dir, &g.qt_dir_200, "qt_dir_200");
    assert_dir_eq(&qt200.flip, &g.qt_flip_200, "qt_flip_200");
}

#[test]
fn momentum_match_golden() {
    let d = load_csv();
    let g = load_golden();

    let m = macd(&d.close, 12, 26, 9);
    assert_series_close(&m.macd, &g.macd_line, "macd_line");
    assert_series_close(&m.signal, &g.macd_signal, "macd_signal");
    assert_series_close(&m.hist, &g.macd_hist, "macd_hist");

    let sq = squeeze_momentum(&d.high, &d.low, &d.close, 20, 2.0, 1.5);
    assert_series_close(&sq.val, &g.sqz_val, "sqz_val");
    assert_flag_eq(&sq.sqz_on, &g.sqz_on, "sqz_on");
    assert_flag_eq(&sq.sqz_off, &g.sqz_off, "sqz_off");

    assert_series_close(&tsi(&d.close, 25, 13), &g.tsi_25_13, "tsi_25_13");
}

#[test]
fn mean_reversion_and_volatility_match_golden() {
    let d = load_csv();
    let g = load_golden();

    // mean reversion
    assert_series_close(&connors_rsi2(&d.close, 2), &g.rsi_2, "rsi_2");
    assert_series_close(
        &percent_b_default(&d.close, 20, 2.0),
        &g.pct_b_20,
        "pct_b_20",
    );
    assert_series_close(
        &williams_r(&d.high, &d.low, &d.close, 14),
        &g.willr_14,
        "willr_14",
    );
    assert_series_close(&ma_zscore(&d.close, 20, 100), &g.ma_zscore, "ma_zscore");

    // volatility
    let bb = bollinger(&d.close, 20, 2.0);
    assert_series_close(&bb.upper, &g.bb_upper_20, "bb_upper_20");
    assert_series_close(&bb.middle, &g.bb_middle_20, "bb_middle_20");
    assert_series_close(&bb.lower, &g.bb_lower_20, "bb_lower_20");
    assert_series_close(&bb_width(&bb), &g.bb_width_20, "bb_width_20");

    let kc = keltner(&d.high, &d.low, &d.close, 20, 20, 2.0);
    assert_series_close(&kc.upper, &g.kc_upper_20, "kc_upper_20");
    assert_series_close(&kc.middle, &g.kc_middle_20, "kc_middle_20");
    assert_series_close(&kc.lower, &g.kc_lower_20, "kc_lower_20");

    assert_series_close(
        &choppiness(&d.high, &d.low, &d.close, 14),
        &g.chop_14,
        "chop_14",
    );
}
