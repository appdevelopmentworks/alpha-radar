//! Volatility / filter indicators: Bollinger Bands (+width), Keltner Channel,
//! Choppiness Index. These feed gates / risk / proximity, not the direction
//! score, so they expose raw series (or 0..1 quality gates) rather than
//! `[-1,+1]` sub-scores (docs/02-indicators.md).

use super::{atr, ema, rolling_max, rolling_min, rolling_std_pop, sma, true_range};

// ===================== Bollinger Bands =====================

/// Bollinger Bands: `middle = SMA(period)`, `upper/lower = middle ± mult·σ`
/// (population σ). Shared by %B (mean-reversion) and BB width.
pub struct BollingerBands {
    pub upper: Vec<Option<f64>>,
    pub middle: Vec<Option<f64>>,
    pub lower: Vec<Option<f64>>,
}

/// Bollinger Bands over `period` with `mult` standard deviations
/// (defaults 20 / 2.0), matching TA-Lib `BBANDS` (population σ).
pub fn bollinger(close: &[f64], period: usize, mult: f64) -> BollingerBands {
    let middle = sma(close, period);
    let sd = rolling_std_pop(close, period);
    let n = close.len();
    let mut upper = vec![None; n];
    let mut lower = vec![None; n];
    for i in 0..n {
        if let (Some(m), Some(s)) = (middle[i], sd[i]) {
            upper[i] = Some(m + mult * s);
            lower[i] = Some(m - mult * s);
        }
    }
    BollingerBands {
        upper,
        middle,
        lower,
    }
}

/// Bollinger band width `(upper − lower) / middle` — the squeeze/expansion
/// measure used as a quality gate (docs/02).
pub fn bb_width(bb: &BollingerBands) -> Vec<Option<f64>> {
    (0..bb.middle.len())
        .map(|i| match (bb.upper[i], bb.lower[i], bb.middle[i]) {
            (Some(u), Some(l), Some(m)) if m != 0.0 => Some((u - l) / m),
            _ => None,
        })
        .collect()
}

// ===================== Keltner Channel =====================

/// Keltner Channel: `middle = EMA(ema_period)`, bands `± mult·ATR(atr_period)`.
pub struct KeltnerChannel {
    pub upper: Vec<Option<f64>>,
    pub middle: Vec<Option<f64>>,
    pub lower: Vec<Option<f64>>,
}

/// Keltner Channel (defaults EMA 20 / ATR 20 / mult 2.0).
pub fn keltner(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    ema_period: usize,
    atr_period: usize,
    mult: f64,
) -> KeltnerChannel {
    let middle = ema(close, ema_period);
    let a = atr(high, low, close, atr_period);
    let n = close.len();
    let mut upper = vec![None; n];
    let mut lower = vec![None; n];
    for i in 0..n {
        if let (Some(m), Some(av)) = (middle[i], a[i]) {
            upper[i] = Some(m + mult * av);
            lower[i] = Some(m - mult * av);
        }
    }
    KeltnerChannel {
        upper,
        middle,
        lower,
    }
}

// ===================== Choppiness Index =====================

/// Choppiness Index over `period` (default 14):
/// `100 · log10(Σ TR / (maxHigh − minLow)) / log10(period)`. High = range,
/// low = trend. First value at bar `period-1`.
pub fn choppiness(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    if period < 2 || high.len() != n || low.len() != n || n < period {
        return out;
    }
    let tr = true_range(high, low, close);
    let hh = rolling_max(high, period);
    let ll = rolling_min(low, period);
    let log_n = (period as f64).log10();
    for i in (period - 1)..n {
        let sum_tr: f64 = tr[i + 1 - period..=i].iter().sum();
        if let (Some(h), Some(l)) = (hh[i], ll[i]) {
            let range = h - l;
            if range > 0.0 && sum_tr > 0.0 {
                out[i] = Some(100.0 * (sum_tr / range).log10() / log_n);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bollinger_ordering() {
        let close: Vec<f64> = (0..40).map(|i| 100.0 + (i as f64 * 0.5).sin()).collect();
        let bb = bollinger(&close, 20, 2.0);
        for i in 19..close.len() {
            let (u, m, l) = (
                bb.upper[i].unwrap(),
                bb.middle[i].unwrap(),
                bb.lower[i].unwrap(),
            );
            assert!(u >= m && m >= l);
        }
    }

    #[test]
    fn choppiness_in_range() {
        let close: Vec<f64> = (0..60).map(|i| 100.0 + (i as f64 * 0.3).sin()).collect();
        let high: Vec<f64> = close.iter().map(|c| c + 0.5).collect();
        let low: Vec<f64> = close.iter().map(|c| c - 0.5).collect();
        let chop = choppiness(&high, &low, &close, 14);
        for v in chop.into_iter().flatten() {
            assert!((0.0..=100.0).contains(&v), "chop out of range: {v}");
        }
    }
}
