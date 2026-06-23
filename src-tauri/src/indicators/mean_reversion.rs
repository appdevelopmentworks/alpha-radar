//! Mean-reversion indicators: Connors RSI(2) + 200-MA filter, Bollinger %B,
//! Williams %R, MA-deviation z-score.
//!
//! Sign convention here is **+ = buy, − = sell** (docs/02). Regime-dependent
//! sign-flip / zeroing happens later in `scoring` (ADR-07), never here. Raw
//! series are golden-tested; sub-scores are unit-tested.

use super::normalize::clamp_unit;
use super::volatility::{bollinger, BollingerBands};
use super::{rolling_max, rolling_min, rolling_std_pop, rsi, sma};

// ===================== Connors RSI(2) =====================

/// Connors RSI(2): the raw `RSI(close, period)` (default period 2). The
/// 200-MA trend filter is applied in the sub-score, not here.
pub fn connors_rsi2(close: &[f64], period: usize) -> Vec<Option<f64>> {
    rsi(close, period)
}

/// Connors RSI(2) sub-score: `clamp((50 − RSI2)/50)` when price is above the
/// `ma_period` SMA (uptrend filter), else `0` (docs/02). `None` until the
/// filter MA exists.
pub fn connors_rsi2_subscore(
    close: &[f64],
    rsi2: &[Option<f64>],
    ma_period: usize,
) -> Vec<Option<f64>> {
    let filter_ma = sma(close, ma_period);
    rsi2.iter()
        .enumerate()
        .map(|(i, r)| match (r, filter_ma[i]) {
            (Some(rv), Some(ma)) => {
                if close[i] > ma {
                    Some(clamp_unit((50.0 - rv) / 50.0))
                } else {
                    Some(0.0)
                }
            }
            _ => None,
        })
        .collect()
}

// ===================== Bollinger %B =====================

/// Bollinger %B `(close − lower) / (upper − lower)` over the given bands
/// (default BB 20 / 2.0 via [`bollinger`]).
pub fn percent_b(close: &[f64], bb: &BollingerBands) -> Vec<Option<f64>> {
    (0..close.len())
        .map(|i| match (bb.upper[i], bb.lower[i]) {
            (Some(u), Some(l)) if u != l => Some((close[i] - l) / (u - l)),
            _ => None,
        })
        .collect()
}

/// Convenience: %B from `close` with the default Bollinger config.
pub fn percent_b_default(close: &[f64], period: usize, mult: f64) -> Vec<Option<f64>> {
    percent_b(close, &bollinger(close, period, mult))
}

/// %B sub-score: `clamp((0.5 − %B)·2)` — %B < 0 is a strong buy, > 1 a strong
/// sell (docs/02).
pub fn percent_b_subscore(pb: &[Option<f64>]) -> Vec<Option<f64>> {
    pb.iter()
        .map(|b| b.map(|v| clamp_unit((0.5 - v) * 2.0)))
        .collect()
}

// ===================== Williams %R =====================

/// Williams %R over `period` (default 14): `−100·(maxHigh − close)/(maxHigh −
/// minLow)`, ranging `[-100, 0]`. Matches TA-Lib `WILLR`.
pub fn williams_r(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    let hh = rolling_max(high, period);
    let ll = rolling_min(low, period);
    (0..close.len())
        .map(|i| match (hh[i], ll[i]) {
            (Some(h), Some(l)) if h != l => Some(-100.0 * (h - close[i]) / (h - l)),
            _ => None,
        })
        .collect()
}

/// Williams %R sub-score: maps `%R ∈ [-100, 0]` to `[+1, -1]` so deeply
/// oversold (`-100`) is `+1` (buy) and overbought (`0`) is `-1` (sell):
/// `clamp(−(%R + 50)/50)`.
///
/// (docs/02 left the exact sign to be verified; the negated form gives the
/// correct buy/sell orientation.)
pub fn williams_r_subscore(wr: &[Option<f64>]) -> Vec<Option<f64>> {
    wr.iter()
        .map(|r| r.map(|v| clamp_unit(-(v + 50.0) / 50.0)))
        .collect()
}

// ===================== MA-deviation z-score =====================

/// MA-deviation z-score: `(close − SMA(sma_period)) / σ(close − SMA, std_period)`
/// (population σ). Defaults SMA 20 / std window 100. First value at bar
/// `(sma_period-1) + (std_period-1)`.
pub fn ma_zscore(close: &[f64], sma_period: usize, std_period: usize) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    let mid = sma(close, sma_period);
    let dev: Vec<Option<f64>> = (0..n).map(|i| mid[i].map(|m| close[i] - m)).collect();
    let Some(start) = dev.iter().position(Option::is_some) else {
        return out;
    };
    let valid: Vec<f64> = dev[start..].iter().map(|x| x.unwrap()).collect();
    for (k, s) in rolling_std_pop(&valid, std_period).into_iter().enumerate() {
        if let Some(sv) = s {
            if sv > 0.0 {
                out[start + k] = Some(valid[k] / sv);
            }
        }
    }
    out
}

/// z-score sub-score: `clamp(−z/2)` — positive deviation (stretched up) reads
/// as sell, negative as buy (docs/02).
pub fn ma_zscore_subscore(z: &[Option<f64>]) -> Vec<Option<f64>> {
    z.iter().map(|v| v.map(|x| clamp_unit(-x / 2.0))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_b_subscore_orientation() {
        // %B = 0 (at lower band) → +1 buy; %B = 1 (upper) → -1 sell.
        let pb = [Some(0.0), Some(1.0), Some(0.5)];
        let s = percent_b_subscore(&pb);
        assert_eq!(s[0], Some(1.0));
        assert_eq!(s[1], Some(-1.0));
        assert_eq!(s[2], Some(0.0));
    }

    #[test]
    fn williams_r_subscore_orientation() {
        // -100 (oversold) → +1; 0 (overbought) → -1; -50 → 0.
        let wr = [Some(-100.0), Some(0.0), Some(-50.0)];
        let s = williams_r_subscore(&wr);
        assert_eq!(s[0], Some(1.0));
        assert_eq!(s[1], Some(-1.0));
        assert_eq!(s[2], Some(0.0));
    }

    #[test]
    fn connors_filter_zeroes_in_downtrend() {
        // Flat-then-below-MA: when close <= SMA200 the score is forced to 0.
        let mut close: Vec<f64> = (0..210).map(|i| 100.0 + i as f64).collect();
        // Drop the last bar far below its SMA200 with a low RSI2.
        *close.last_mut().unwrap() = 1.0;
        let r = connors_rsi2(&close, 2);
        let s = connors_rsi2_subscore(&close, &r, 200);
        assert_eq!(*s.last().unwrap(), Some(0.0));
    }
}
