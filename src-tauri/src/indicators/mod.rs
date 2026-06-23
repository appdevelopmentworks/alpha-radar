//! Indicator computation engine (P1).
//!
//! All math here is **pure and deterministic** (no I/O, no globals). Every
//! series-producing function returns `Vec<Option<f64>>` aligned 1:1 with the
//! input bars, so warm-up / insufficient-history positions are an explicit
//! `None` (excluded from downstream category means in `scoring`) rather than a
//! silent `NaN` (docs/02-indicators.md, ADR-06).
//!
//! ## Wilder / seeding convention
//!
//! RSI and ATR use Wilder's running average seeded with the simple moving
//! average of the first `period` samples, recursing with `α = 1/period`. EMA
//! seeds the same way with `α = 2/(period+1)`. Golden fixtures are pinned to
//! TA-Lib (`SMA` / `EMA` / `RSI` / `ATR` / `LINEARREG`), the reference named
//! first in ADR-13. SMA/EMA/RSI/LINEARREG are seed-identical across TA-Lib and
//! TradingView; ATR's warm-up seed differs between the two (see [`atr`]) but
//! converges well before any signal-relevant bar.
//!
//! Session 1 implements the base primitives below; trend/momentum/
//! mean_reversion/volatility build on them in later P1 sessions.

pub mod mean_reversion;
pub mod momentum;
pub mod normalize;
pub mod trend;
pub mod volatility;

/// Simple moving average over `period`.
///
/// `out[i] = mean(values[i+1-period ..= i])` for `i >= period-1`; earlier
/// positions (and any call with `period == 0` or `period > len`) are `None`.
pub fn sma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = values.len();
    let mut out = vec![None; n];
    if period == 0 || period > n {
        return out;
    }
    for i in (period - 1)..n {
        let window = &values[i + 1 - period..=i];
        out[i] = Some(window.iter().sum::<f64>() / period as f64);
    }
    out
}

/// Exponential moving average, seeded with the SMA of the first `period`
/// values, then recursed with `α = 2/(period+1)` (`adjust = false`).
///
/// `out[period-1]` is the SMA seed; `out[i] = α·values[i] + (1-α)·out[i-1]`
/// thereafter. Matches TradingView `ta.ema` and pandas-ta `ema(talib=False)`.
pub fn ema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let alpha = 2.0 / (period as f64 + 1.0);
    seeded_recursive_average(values, period, alpha)
}

/// Wilder's RSI over `period` close-to-close changes.
///
/// First value lands at bar `period` (it consumes `period` changes); earlier
/// bars are `None`. Gains/losses are averaged with Wilder's RMA (SMA-seeded,
/// `α = 1/period`). A perfectly flat window yields a neutral `50.0`.
pub fn rsi(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = values.len();
    let mut out = vec![None; n];
    if period == 0 || n < period + 1 {
        return out;
    }

    // Gains / losses aligned to bar index; index 0 has no change.
    let mut gains = vec![0.0; n];
    let mut losses = vec![0.0; n];
    for i in 1..n {
        let delta = values[i] - values[i - 1];
        if delta > 0.0 {
            gains[i] = delta;
        } else {
            losses[i] = -delta;
        }
    }

    // Seed = SMA of the first `period` changes (bars 1..=period); the seeded
    // average lands at bar `period`.
    let alpha = 1.0 / period as f64;
    let mut avg_gain = gains[1..=period].iter().sum::<f64>() / period as f64;
    let mut avg_loss = losses[1..=period].iter().sum::<f64>() / period as f64;
    out[period] = Some(rsi_from(avg_gain, avg_loss));
    for i in (period + 1)..n {
        avg_gain = alpha * gains[i] + (1.0 - alpha) * avg_gain;
        avg_loss = alpha * losses[i] + (1.0 - alpha) * avg_loss;
        out[i] = Some(rsi_from(avg_gain, avg_loss));
    }
    out
}

/// Wilder's Average True Range over `period`, matching TA-Lib `ATR`.
///
/// True range is `max(high-low, |high-prev_close|, |low-prev_close|)` for bars
/// `i >= 1`; bar 0 has no previous close and is excluded. ATR is seeded at bar
/// `period` with the SMA of `tr[1..=period]`, then smoothed with Wilder's
/// recursion (`α = 1/period`); the first value lands at bar `period`.
///
/// Note: TradingView's `ta.atr` instead includes `tr[0] = high[0]-low[0]` and
/// emits its first value one bar earlier. The two differ only during warm-up
/// and converge before any signal-relevant bar (docs/02 "ケルトナーの ATR 定義"
/// 流儀差); we pin to TA-Lib per ADR-13.
pub fn atr(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    if period == 0 || high.len() != n || low.len() != n || n < period + 1 {
        return out;
    }

    // True range; tr[0] is undefined (no previous close) and unused.
    let mut tr = vec![0.0; n];
    for i in 1..n {
        let hl = high[i] - low[i];
        let hc = (high[i] - close[i - 1]).abs();
        let lc = (low[i] - close[i - 1]).abs();
        tr[i] = hl.max(hc).max(lc);
    }

    // Seed at bar `period` with the SMA of tr[1..=period]; then Wilder recursion.
    let alpha = 1.0 / period as f64;
    let seed = tr[1..=period].iter().sum::<f64>() / period as f64;
    out[period] = Some(seed);
    let mut prev = seed;
    for i in (period + 1)..n {
        prev = alpha * tr[i] + (1.0 - alpha) * prev;
        out[i] = Some(prev);
    }
    out
}

/// Linear-regression endpoint over a rolling window of `period` values.
///
/// For each window ending at bar `i`, fit `y = a + b·x` with `x = 0..period-1`
/// (oldest→newest) by ordinary least squares and return the fitted value at the
/// most recent point, `a + b·(period-1)`. This is the value used by Squeeze
/// Momentum and matches TradingView `ta.linreg(src, period, 0)` and pandas-ta
/// `linreg` (the predicted endpoint is independent of the x-axis labeling).
pub fn linreg(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = values.len();
    let mut out = vec![None; n];
    if period == 0 || period > n {
        return out;
    }

    let p = period as f64;
    let x_sum = (period - 1) as f64 * p / 2.0; // Σ 0..period-1
    let x2_sum = (p - 1.0) * p * (2.0 * p - 1.0) / 6.0; // Σ k², k=0..period-1
    let denom = p * x2_sum - x_sum * x_sum;
    if denom == 0.0 {
        return out; // period == 1: slope undefined
    }

    for i in (period - 1)..n {
        let window = &values[i + 1 - period..=i];
        let y_sum: f64 = window.iter().sum();
        let xy_sum: f64 = window.iter().enumerate().map(|(k, &v)| k as f64 * v).sum();
        let slope = (p * xy_sum - x_sum * y_sum) / denom;
        let intercept = (y_sum - slope * x_sum) / p;
        out[i] = Some(intercept + slope * (p - 1.0));
    }
    out
}

/// RSI from Wilder averages: `100 · gain / (gain + loss)`. Neutral `50` for a
/// flat window (both averages zero) to avoid `0/0`.
fn rsi_from(avg_gain: f64, avg_loss: f64) -> f64 {
    let denom = avg_gain + avg_loss;
    if denom == 0.0 {
        return 50.0;
    }
    100.0 * avg_gain / denom
}

/// Shared seed-then-recurse helper: `out[period-1] = SMA(first period)`, then
/// `out[i] = α·values[i] + (1-α)·out[i-1]`. Used by [`ema`] (Wilder RSI/ATR
/// inline their own seeding because their windows start at bar 1).
fn seeded_recursive_average(values: &[f64], period: usize, alpha: f64) -> Vec<Option<f64>> {
    let n = values.len();
    let mut out = vec![None; n];
    if period == 0 || period > n {
        return out;
    }
    let seed = values[..period].iter().sum::<f64>() / period as f64;
    let mut prev = seed;
    out[period - 1] = Some(seed);
    for i in period..n {
        prev = alpha * values[i] + (1.0 - alpha) * prev;
        out[i] = Some(prev);
    }
    out
}

// --- shared rolling helpers (used across the indicator submodules) ---

/// Rolling maximum over `period` (highest value in `values[i+1-period..=i]`).
/// First value at bar `period-1`.
pub(crate) fn rolling_max(values: &[f64], period: usize) -> Vec<Option<f64>> {
    rolling_extreme(values, period, f64::max)
}

/// Rolling minimum over `period`. First value at bar `period-1`.
pub(crate) fn rolling_min(values: &[f64], period: usize) -> Vec<Option<f64>> {
    rolling_extreme(values, period, f64::min)
}

fn rolling_extreme(values: &[f64], period: usize, pick: fn(f64, f64) -> f64) -> Vec<Option<f64>> {
    let n = values.len();
    let mut out = vec![None; n];
    if period == 0 || period > n {
        return out;
    }
    for i in (period - 1)..n {
        let window = &values[i + 1 - period..=i];
        out[i] = Some(window.iter().copied().fold(window[0], pick));
    }
    out
}

/// Rolling population standard deviation (divisor `N`) over `period`. Matches
/// TA-Lib `STDDEV`/`BBANDS` and Pine `ta.stdev`. First value at bar `period-1`.
pub(crate) fn rolling_std_pop(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = values.len();
    let mut out = vec![None; n];
    if period == 0 || period > n {
        return out;
    }
    let p = period as f64;
    for i in (period - 1)..n {
        let window = &values[i + 1 - period..=i];
        let mean = window.iter().sum::<f64>() / p;
        let var = window.iter().map(|&v| (v - mean) * (v - mean)).sum::<f64>() / p;
        out[i] = Some(var.sqrt());
    }
    out
}

/// True range with `tr[0] = high[0]-low[0]` (Pine `ta.tr(true)` convention),
/// `tr[i>=1] = max(high-low, |high-prev_close|, |low-prev_close|)`. Used by
/// Keltner / Squeeze / Choppiness (ATR itself excludes bar 0 to match TA-Lib).
pub(crate) fn true_range(high: &[f64], low: &[f64], close: &[f64]) -> Vec<f64> {
    let n = close.len();
    let mut tr = vec![0.0; n];
    if n == 0 {
        return tr;
    }
    tr[0] = high[0] - low[0];
    for i in 1..n {
        let hl = high[i] - low[i];
        let hc = (high[i] - close[i - 1]).abs();
        let lc = (low[i] - close[i - 1]).abs();
        tr[i] = hl.max(hc).max(lc);
    }
    tr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sma_basic() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        let got = sma(&v, 3);
        assert_eq!(got[0], None);
        assert_eq!(got[1], None);
        assert_eq!(got[2], Some(2.0));
        assert_eq!(got[3], Some(3.0));
        assert_eq!(got[4], Some(4.0));
    }

    #[test]
    fn ema_seed_is_sma() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        let got = ema(&v, 3);
        assert_eq!(got[0], None);
        assert_eq!(got[1], None);
        assert_eq!(got[2], Some(2.0)); // SMA(1,2,3) seed
                                       // α = 0.5: e[3] = 0.5*4 + 0.5*2 = 3.0; e[4] = 0.5*5 + 0.5*3 = 4.0
        assert_eq!(got[3], Some(3.0));
        assert_eq!(got[4], Some(4.0));
    }

    #[test]
    fn linreg_on_perfect_line_is_identity() {
        // y = 2x + 1 → endpoint equals the last value exactly.
        let v: Vec<f64> = (0..10).map(|x| 2.0 * x as f64 + 1.0).collect();
        let got = linreg(&v, 5);
        for i in 4..10 {
            assert!((got[i].unwrap() - v[i]).abs() < 1e-9);
        }
    }

    #[test]
    fn too_short_is_all_none() {
        let v = [1.0, 2.0];
        assert!(sma(&v, 5).iter().all(|x| x.is_none()));
        assert!(rsi(&v, 14).iter().all(|x| x.is_none()));
    }
}
