//! Momentum indicators: MACD, Squeeze Momentum (LazyBear), TSI.
//!
//! Raw series are golden-tested against TA-Lib (MACD) or an explicit reference
//! mirrored in `tools/golden/gen_golden.py` (Squeeze, TSI). Sub-scores follow
//! docs/02-indicators.md.

use super::normalize::{clamp_unit, tanh_compress};
use super::{atr, ema, linreg, rolling_max, rolling_min, sma, true_range};

/// Run `ema(p2)` over the warmed-up portion of `ema(values, p1)`, realigned to
/// the original indices. First value at bar `(p1-1)+(p2-1)`.
fn ema_then_ema(values: &[f64], p1: usize, p2: usize) -> Vec<Option<f64>> {
    let first = ema(values, p1);
    let mut out = vec![None; values.len()];
    let Some(start) = first.iter().position(Option::is_some) else {
        return out;
    };
    let valid: Vec<f64> = first[start..].iter().map(|x| x.unwrap()).collect();
    for (k, v) in ema(&valid, p2).into_iter().enumerate() {
        out[start + k] = v;
    }
    out
}

// ===================== MACD =====================

/// MACD line, signal, and histogram.
pub struct Macd {
    pub macd: Vec<Option<f64>>,
    pub signal: Vec<Option<f64>>,
    pub hist: Vec<Option<f64>>,
}

/// MACD (defaults fast 12 / slow 26 / signal 9), matching TA-Lib.
///
/// The MACD line (`emaFast − emaSlow`) lands at bar `slow-1`; the signal EMA of
/// the MACD line (and therefore the histogram) lands at bar
/// `(slow-1)+(signal-1)`.
pub fn macd(close: &[f64], fast: usize, slow: usize, signal_period: usize) -> Macd {
    let n = close.len();
    let ef = ema(close, fast);
    let es = ema(close, slow);
    let macd_line: Vec<Option<f64>> = (0..n)
        .map(|i| match (ef[i], es[i]) {
            (Some(a), Some(b)) => Some(a - b),
            _ => None,
        })
        .collect();

    let mut signal = vec![None; n];
    let mut hist = vec![None; n];
    if let Some(start) = macd_line.iter().position(Option::is_some) {
        let valid: Vec<f64> = macd_line[start..].iter().map(|x| x.unwrap()).collect();
        for (k, s) in ema(&valid, signal_period).into_iter().enumerate() {
            if let Some(sv) = s {
                signal[start + k] = Some(sv);
                hist[start + k] = Some(macd_line[start + k].unwrap() - sv);
            }
        }
    }

    Macd {
        macd: macd_line,
        signal,
        hist,
    }
}

/// MACD sub-score: `tanh(hist / (k·ATR))` (docs/02, `k` default 0.5).
pub fn macd_subscore(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    m: &Macd,
    atr_period: usize,
    k: f64,
) -> Vec<Option<f64>> {
    let atrv = atr(high, low, close, atr_period);
    m.hist
        .iter()
        .zip(atrv.iter())
        .map(|(h, a)| match (h, a) {
            (Some(h), Some(a)) => Some(tanh_compress(*h, k * a)),
            _ => None,
        })
        .collect()
}

// ===================== Squeeze Momentum (LazyBear) =====================

/// Squeeze Momentum output: linreg momentum `val` plus the squeeze on/off flags.
pub struct SqueezeMomentum {
    pub val: Vec<Option<f64>>,
    pub sqz_on: Vec<Option<bool>>,
    pub sqz_off: Vec<Option<bool>>,
}

/// Squeeze Momentum (LazyBear): BB(`length`, `mult_bb`) vs KC(`length`,
/// `mult_kc`, true-range based), momentum = `linreg(close − mid, length)`.
/// Defaults `length=20`, `mult_bb=2.0`, `mult_kc=1.5`.
pub fn squeeze_momentum(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    length: usize,
    mult_bb: f64,
    mult_kc: f64,
) -> SqueezeMomentum {
    let n = close.len();
    let basis = sma(close, length);
    let dev = super::rolling_std_pop(close, length);
    let ma = &basis; // LazyBear uses the same SMA for the KC mid
    let rangema = sma(&true_range(high, low, close), length);

    let mut sqz_on = vec![None; n];
    let mut sqz_off = vec![None; n];
    for i in 0..n {
        let (Some(b), Some(d), Some(m), Some(r)) = (basis[i], dev[i], ma[i], rangema[i]) else {
            continue;
        };
        let (upper_bb, lower_bb) = (b + mult_bb * d, b - mult_bb * d);
        let (upper_kc, lower_kc) = (m + mult_kc * r, m - mult_kc * r);
        sqz_on[i] = Some(lower_bb > lower_kc && upper_bb < upper_kc);
        sqz_off[i] = Some(lower_bb < lower_kc && upper_bb > upper_kc);
    }

    // momentum source = close − avg( avg(highest(high), lowest(low)), sma(close) )
    let hh = rolling_max(high, length);
    let ll = rolling_min(low, length);
    let source: Vec<Option<f64>> = (0..n)
        .map(|i| match (hh[i], ll[i], basis[i]) {
            (Some(h), Some(l), Some(s)) => Some(close[i] - ((h + l) / 2.0 + s) / 2.0),
            _ => None,
        })
        .collect();

    let mut val = vec![None; n];
    if let Some(start) = source.iter().position(Option::is_some) {
        let valid: Vec<f64> = source[start..].iter().map(|x| x.unwrap()).collect();
        for (k, v) in linreg(&valid, length).into_iter().enumerate() {
            val[start + k] = v;
        }
    }

    SqueezeMomentum {
        val,
        sqz_on,
        sqz_off,
    }
}

/// Squeeze sub-score: `sign(val)·clamp(|val|/(k·ATR), 0, 1)` (docs/02).
pub fn squeeze_subscore(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    sq: &SqueezeMomentum,
    atr_period: usize,
    k: f64,
) -> Vec<Option<f64>> {
    let atrv = atr(high, low, close, atr_period);
    sq.val
        .iter()
        .zip(atrv.iter())
        .map(|(v, a)| match (v, a) {
            (Some(v), Some(a)) => {
                let scale = k * a;
                let strength = if scale > 0.0 {
                    (v.abs() / scale).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                Some(clamp_unit(v.signum() * strength))
            }
            _ => None,
        })
        .collect()
}

// ===================== TSI =====================

/// True Strength Index (defaults long 25 / short 13).
///
/// `TSI = 100 · EMA_short(EMA_long(Δclose)) / EMA_short(EMA_long(|Δclose|))`.
/// First value at bar `(long-1)+(short-1)+1`.
pub fn tsi(close: &[f64], long: usize, short: usize) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    if n < 2 {
        return out;
    }
    let m: Vec<f64> = (1..n).map(|i| close[i] - close[i - 1]).collect();
    let abs_m: Vec<f64> = m.iter().map(|x| x.abs()).collect();
    let ds_m = ema_then_ema(&m, long, short);
    let ds_abs = ema_then_ema(&abs_m, long, short);
    for k in 0..m.len() {
        if let (Some(a), Some(b)) = (ds_m[k], ds_abs[k]) {
            // m[k] corresponds to bar k+1.
            out[k + 1] = Some(if b != 0.0 { 100.0 * a / b } else { 0.0 });
        }
    }
    out
}

/// TSI sub-score: `clamp(TSI/50, -1, +1)` (docs/02 base form).
pub fn tsi_subscore(tsi_vals: &[Option<f64>]) -> Vec<Option<f64>> {
    tsi_vals
        .iter()
        .map(|t| t.map(|v| clamp_unit(v / 50.0)))
        .collect()
}

/// RSI sub-score in **range orientation**: `clamp((50 − RSI)/50)` — overbought
/// is sell (−), oversold is buy (+). The scoring layer assigns this to the
/// mean-reversion category, so the regime sign-flip (ADR-07) automatically
/// turns it into trend-continuation in trends (docs/02 "トレンド時の解釈は
/// scoring 側で切替").
pub fn rsi_subscore(rsi_vals: &[Option<f64>]) -> Vec<Option<f64>> {
    rsi_vals
        .iter()
        .map(|r| r.map(|v| clamp_unit((50.0 - v) / 50.0)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macd_warmup_alignment() {
        let close: Vec<f64> = (0..60).map(|i| 100.0 + (i as f64 * 0.3).sin()).collect();
        let m = macd(&close, 12, 26, 9);
        assert!(m.macd[24].is_none() && m.macd[25].is_some()); // slow-1 = 25
        assert!(m.signal[32].is_none() && m.signal[33].is_some()); // +signal-1 = 33
        assert!(m.hist[32].is_none() && m.hist[33].is_some());
    }

    #[test]
    fn tsi_is_bounded_and_signed() {
        // Monotone up → TSI → +100 territory (all momentum positive).
        let up: Vec<f64> = (0..120).map(|i| 100.0 + i as f64).collect();
        let t = tsi(&up, 25, 13);
        let last = t.last().unwrap().unwrap();
        assert!((last - 100.0).abs() < 1e-6, "got {last}");
    }
}
