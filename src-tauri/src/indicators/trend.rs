//! Trend indicators: ADX/DMI, EMA ribbon, Supertrend, Ichimoku.
//!
//! Raw series are golden-tested (docs/07); the derived sub-scores
//! (`s ∈ [-1, +1]`, + = buy) are unit-tested with crafted cases. Formula
//! coefficients come from docs/02-indicators.md and are named `const`s, not
//! tunable config.

use super::normalize::clamp_unit;
use super::{atr, ema, rolling_max, rolling_min};

// ===================== ADX / DMI =====================

/// Directional movement output: `+DI`, `-DI`, and `ADX`.
pub struct Dmi {
    pub plus_di: Vec<Option<f64>>,
    pub minus_di: Vec<Option<f64>>,
    pub adx: Vec<Option<f64>>,
}

/// ADX / DMI over `period`, matching TA-Lib `PLUS_DI` / `MINUS_DI` / `ADX`.
///
/// `+DI` / `-DI` first land at bar `period`; `ADX` (Wilder-smoothed DX) first
/// lands at bar `2*period-1`.
pub fn adx_dmi(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Dmi {
    let n = close.len();
    let mut plus_di = vec![None; n];
    let mut minus_di = vec![None; n];
    let mut adx = vec![None; n];
    if period == 0 || high.len() != n || low.len() != n || n < 2 * period {
        return Dmi {
            plus_di,
            minus_di,
            adx,
        };
    }

    // Per-bar directional movement and true range (bar 0 has no predecessor).
    let mut tr = vec![0.0; n];
    let mut pdm = vec![0.0; n];
    let mut mdm = vec![0.0; n];
    for i in 1..n {
        let up = high[i] - high[i - 1];
        let dn = low[i - 1] - low[i];
        pdm[i] = if up > dn && up > 0.0 { up } else { 0.0 };
        mdm[i] = if dn > up && dn > 0.0 { dn } else { 0.0 };
        let hl = high[i] - low[i];
        let hc = (high[i] - close[i - 1]).abs();
        let lc = (low[i] - close[i - 1]).abs();
        tr[i] = hl.max(hc).max(lc);
    }

    // Wilder running-sum smoothing of TR/+DM/-DM. TA-Lib seeds the sums with
    // the first `period-1` values (bars 1..period-1), then applies the
    // decay-and-add on every output bar starting at bar `period`.
    let pinv = 1.0 / period as f64;
    let mut sm_tr: f64 = tr[1..period].iter().sum();
    let mut sm_p: f64 = pdm[1..period].iter().sum();
    let mut sm_m: f64 = mdm[1..period].iter().sum();

    let mut dx = vec![0.0; n];
    for i in period..n {
        sm_tr = sm_tr - sm_tr * pinv + tr[i];
        sm_p = sm_p - sm_p * pinv + pdm[i];
        sm_m = sm_m - sm_m * pinv + mdm[i];
        let pdi = if sm_tr != 0.0 {
            100.0 * sm_p / sm_tr
        } else {
            0.0
        };
        let mdi = if sm_tr != 0.0 {
            100.0 * sm_m / sm_tr
        } else {
            0.0
        };
        plus_di[i] = Some(pdi);
        minus_di[i] = Some(mdi);
        let denom = pdi + mdi;
        dx[i] = if denom != 0.0 {
            100.0 * (pdi - mdi).abs() / denom
        } else {
            0.0
        };
    }

    // ADX: seed at 2*period-1 with the mean of the first `period` DX values,
    // then Wilder-smooth.
    let seed_end = 2 * period - 1;
    let mut adx_prev = dx[period..=seed_end].iter().sum::<f64>() / period as f64;
    adx[seed_end] = Some(adx_prev);
    for i in (seed_end + 1)..n {
        adx_prev = (adx_prev * (period as f64 - 1.0) + dx[i]) / period as f64;
        adx[i] = Some(adx_prev);
    }

    Dmi {
        plus_di,
        minus_di,
        adx,
    }
}

/// ADX/DMI sub-score: `sign(+DI − −DI) · clamp(ADX/50, 0, 1)` (docs/02).
/// `None` where ADX is not yet available.
pub fn adx_subscore(dmi: &Dmi) -> Vec<Option<f64>> {
    dmi.adx
        .iter()
        .zip(dmi.plus_di.iter())
        .zip(dmi.minus_di.iter())
        .map(|((adx, pdi), mdi)| match (adx, pdi, mdi) {
            (Some(a), Some(p), Some(m)) => {
                let dir = (p - m).signum();
                Some(clamp_unit(dir * (a / 50.0).clamp(0.0, 1.0)))
            }
            _ => None,
        })
        .collect()
}

// ===================== EMA ribbon =====================

const RIBBON_ORDER_WEIGHT: f64 = 0.6;
const RIBBON_SLOPE_WEIGHT: f64 = 0.4;

/// EMA ribbon (fast / mid / slow), default 20 / 50 / 200.
pub struct EmaRibbon {
    pub fast: Vec<Option<f64>>,
    pub mid: Vec<Option<f64>>,
    pub slow: Vec<Option<f64>>,
}

/// Compute the three ribbon EMAs.
pub fn ema_ribbon(close: &[f64], periods: [usize; 3]) -> EmaRibbon {
    EmaRibbon {
        fast: ema(close, periods[0]),
        mid: ema(close, periods[1]),
        slow: ema(close, periods[2]),
    }
}

/// Ribbon sub-score: `0.6·order + 0.4·slope` (docs/02), clamped.
///
/// `order ∈ [-1,1]` is the averaged pairwise sign of (fast,mid,slow); `slope`
/// is the averaged sign of each EMA's one-bar change. `None` until all three
/// EMAs (and their previous bar) exist.
pub fn ema_ribbon_subscore(r: &EmaRibbon) -> Vec<Option<f64>> {
    let n = r.fast.len();
    let mut out = vec![None; n];
    // Windowed access (bar `i` and `i-1`) across three parallel slices reads
    // more clearly as an index loop than zipped iterators.
    #[allow(clippy::needless_range_loop)]
    for i in 1..n {
        let (Some(f), Some(m), Some(s)) = (r.fast[i], r.mid[i], r.slow[i]) else {
            continue;
        };
        let (Some(fp), Some(mp), Some(sp)) = (r.fast[i - 1], r.mid[i - 1], r.slow[i - 1]) else {
            continue;
        };
        let order = ((f - m).signum() + (m - s).signum() + (f - s).signum()) / 3.0;
        let slope = ((f - fp).signum() + (m - mp).signum() + (s - sp).signum()) / 3.0;
        out[i] = Some(clamp_unit(
            RIBBON_ORDER_WEIGHT * order + RIBBON_SLOPE_WEIGHT * slope,
        ));
    }
    out
}

// ===================== Supertrend =====================

/// Supertrend line and direction (`+1` up / `-1` down).
pub struct Supertrend {
    pub line: Vec<Option<f64>>,
    pub dir: Vec<Option<i8>>,
}

/// Supertrend over ATR `period` and `mult` (defaults 10 / 3.0).
///
/// Uses the Wilder ATR ([`atr`]); the band-flip recursion starts at the first
/// bar where ATR exists and is initialized to an up-trend.
pub fn supertrend(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    period: usize,
    mult: f64,
) -> Supertrend {
    let n = close.len();
    let mut line = vec![None; n];
    let mut dir = vec![None; n];
    let atrv = atr(high, low, close, period);
    let Some(start) = atrv.iter().position(Option::is_some) else {
        return Supertrend { line, dir };
    };

    let hl2 = |i: usize| (high[i] + low[i]) / 2.0;
    let mut fu_prev = hl2(start) + mult * atrv[start].unwrap();
    let mut fl_prev = hl2(start) - mult * atrv[start].unwrap();
    let mut d_prev: i8 = 1; // initialize up-trend
    line[start] = Some(fl_prev);
    dir[start] = Some(d_prev);

    for i in (start + 1)..n {
        let a = atrv[i].unwrap();
        let bu = hl2(i) + mult * a;
        let bl = hl2(i) - mult * a;
        let fu = if bu < fu_prev || close[i - 1] > fu_prev {
            bu
        } else {
            fu_prev
        };
        let fl = if bl > fl_prev || close[i - 1] < fl_prev {
            bl
        } else {
            fl_prev
        };
        let d = if d_prev == 1 {
            if close[i] < fl {
                -1
            } else {
                1
            }
        } else if close[i] > fu {
            1
        } else {
            -1
        };
        line[i] = Some(if d == 1 { fl } else { fu });
        dir[i] = Some(d);
        fu_prev = fu;
        fl_prev = fl;
        d_prev = d;
    }

    Supertrend { line, dir }
}

/// Supertrend sub-score: `dir · clamp(|close − line| / (mult·ATR), 0, 1)`
/// (docs/02). Needs ATR, so it reuses `period`/`mult`.
pub fn supertrend_subscore(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    st: &Supertrend,
    period: usize,
    mult: f64,
) -> Vec<Option<f64>> {
    let atrv = atr(high, low, close, period);
    let n = close.len();
    let mut out = vec![None; n];
    for i in 0..n {
        let (Some(line), Some(d), Some(a)) = (st.line[i], st.dir[i], atrv[i]) else {
            continue;
        };
        let scale = mult * a;
        let strength = if scale > 0.0 {
            ((close[i] - line).abs() / scale).clamp(0.0, 1.0)
        } else {
            0.0
        };
        out[i] = Some(clamp_unit(d as f64 * strength));
    }
    out
}

// ===================== Ichimoku =====================

const ICHI_PRICE_CLOUD_WEIGHT: f64 = 0.4;
const ICHI_TENKAN_KIJUN_WEIGHT: f64 = 0.3;
const ICHI_CHIKOU_WEIGHT: f64 = 0.2;
const ICHI_TWIST_WEIGHT: f64 = 0.1;

/// Ichimoku lines. `senkou_a` / `senkou_b` are **aligned to the current bar**
/// (i.e. already projected forward by `displacement`): the value at bar `i` is
/// the span that was plotted `displacement` bars ago.
pub struct Ichimoku {
    pub tenkan: Vec<Option<f64>>,
    pub kijun: Vec<Option<f64>>,
    pub senkou_a: Vec<Option<f64>>,
    pub senkou_b: Vec<Option<f64>>,
}

/// Ichimoku (defaults tenkan 9 / kijun 26 / senkou-B 52 / displacement 26).
pub fn ichimoku(
    high: &[f64],
    low: &[f64],
    tenkan_p: usize,
    kijun_p: usize,
    senkou_b_p: usize,
    displacement: usize,
) -> Ichimoku {
    let n = high.len();
    let midline = |hp: usize| -> Vec<Option<f64>> {
        let hh = rolling_max(high, hp);
        let ll = rolling_min(low, hp);
        (0..n)
            .map(|i| match (hh[i], ll[i]) {
                (Some(h), Some(l)) => Some((h + l) / 2.0),
                _ => None,
            })
            .collect()
    };

    let tenkan = midline(tenkan_p);
    let kijun = midline(kijun_p);
    let raw_b = midline(senkou_b_p);
    let raw_a: Vec<Option<f64>> = (0..n)
        .map(|i| match (tenkan[i], kijun[i]) {
            (Some(t), Some(k)) => Some((t + k) / 2.0),
            _ => None,
        })
        .collect();

    // Project the spans forward `displacement` bars so they align to "now".
    let project = |raw: &[Option<f64>]| -> Vec<Option<f64>> {
        (0..n)
            .map(|i| {
                if i >= displacement {
                    raw[i - displacement]
                } else {
                    None
                }
            })
            .collect()
    };

    Ichimoku {
        tenkan,
        kijun,
        senkou_a: project(&raw_a),
        senkou_b: project(&raw_b),
    }
}

/// Ichimoku sub-score (docs/02): weighted sum of price-vs-cloud (0.4),
/// tenkan-vs-kijun (0.3), chikou-vs-price-`displacement`-ago (0.2), and cloud
/// twist (0.1). `close` and `displacement` are needed for the chikou term.
pub fn ichimoku_subscore(close: &[f64], ich: &Ichimoku, displacement: usize) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    for i in 0..n {
        let (Some(tk), Some(kj), Some(sa), Some(sb)) = (
            ich.tenkan[i],
            ich.kijun[i],
            ich.senkou_a[i],
            ich.senkou_b[i],
        ) else {
            continue;
        };
        let (cloud_top, cloud_bot) = (sa.max(sb), sa.min(sb));
        let price_cloud = if close[i] > cloud_top {
            1.0
        } else if close[i] < cloud_bot {
            -1.0
        } else {
            0.0
        };
        let tk_kj = (tk - kj).signum();
        let twist = (sa - sb).signum();
        let chikou = if i >= displacement {
            (close[i] - close[i - displacement]).signum()
        } else {
            0.0
        };
        out[i] = Some(clamp_unit(
            ICHI_PRICE_CLOUD_WEIGHT * price_cloud
                + ICHI_TENKAN_KIJUN_WEIGHT * tk_kj
                + ICHI_CHIKOU_WEIGHT * chikou
                + ICHI_TWIST_WEIGHT * twist,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ribbon_subscore_perfect_order_uptrend() {
        // Strictly increasing closes → perfect order and positive slope → +1.
        let close: Vec<f64> = (0..260).map(|i| 100.0 + i as f64).collect();
        let r = ema_ribbon(&close, [20, 50, 200]);
        let s = ema_ribbon_subscore(&r);
        let last = s.last().unwrap().unwrap();
        assert!((last - 1.0).abs() < 1e-9, "got {last}");
    }

    #[test]
    fn ribbon_subscore_none_before_slow_ema() {
        let close: Vec<f64> = (0..100).map(|i| 100.0 + i as f64).collect();
        let r = ema_ribbon(&close, [20, 50, 200]);
        let s = ema_ribbon_subscore(&r);
        assert!(s.iter().all(Option::is_none)); // slow EMA(200) never warms up
    }

    #[test]
    fn supertrend_uptrend_line_below_price() {
        let close: Vec<f64> = (0..60).map(|i| 100.0 + i as f64).collect();
        let high: Vec<f64> = close.iter().map(|c| c + 0.5).collect();
        let low: Vec<f64> = close.iter().map(|c| c - 0.5).collect();
        let st = supertrend(&high, &low, &close, 10, 3.0);
        let i = 59;
        assert_eq!(st.dir[i], Some(1));
        assert!(st.line[i].unwrap() < close[i]);
    }
}
