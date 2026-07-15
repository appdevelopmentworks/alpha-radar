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

// ===================== Q-Trend =====================

/// Q-Trend output (TradingView "Q-Trend" by tarasenko_, Type A — ADR-15).
///
/// A ratcheting trend line `m` seeded at the midpoint of the `p`-bar close
/// range; each bar, if `close > m + eps` (`eps = mult · ATR(atr_p)` of the
/// PREVIOUS bar) the line steps up to `m + eps`, if `close < m - eps` it steps
/// down. BUY/SELL fire only on direction flips. Chart display layer only —
/// not a scoring sub-score.
pub struct QTrend {
    /// The ratcheting trend line `m`.
    pub line: Vec<Option<f64>>,
    /// Held direction: `+1` after a buy signal, `-1` after a sell; `None`
    /// until the first signal.
    pub dir: Vec<Option<i8>>,
    /// `Some(+1)` on a BUY flip bar, `Some(-1)` on a SELL flip bar.
    pub flip: Vec<Option<i8>>,
    /// STRONG qualifier at flip bars (open sat in the bottom/top octile of the
    /// `p`-bar range within the last 5 bars); `false` elsewhere.
    pub strong: Vec<bool>,
    /// Distance from close to the NEXT bar's flip threshold, in ATR units
    /// (`eps_next = mult · ATR[i]`, fully known at this bar's close — no
    /// lookahead). Drives the precursor detection.
    pub dist_flip_atr: Vec<Option<f64>>,
}

/// Q-Trend over the `p`-bar close range with `eps = mult · ATR(atr_p)[1]`
/// (defaults 200 / 14 / 1.0 — `qtrend_*` in [`crate::config::IndicatorParams`]).
///
/// Pine sequencing is preserved: flip tests run against the PREVIOUS bar's
/// final `m` and the previous bar's ATR; the ratchet applies afterwards.
/// Warm-up: `m` seeds at the first full `p`-window bar as the range midpoint
/// (Pine re-seeds through bar `p`; ≤1-bar divergence, self-corrects as the
/// ratchet converges to price — same warm-up class as the ATR seed note).
pub fn qtrend(
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    p: usize,
    atr_p: usize,
    mult: f64,
) -> QTrend {
    let n = close.len();
    let mut out = QTrend {
        line: vec![None; n],
        dir: vec![None; n],
        flip: vec![None; n],
        strong: vec![false; n],
        dist_flip_atr: vec![None; n],
    };
    if p == 0 || p > n || open.len() != n || high.len() != n || low.len() != n {
        return out;
    }

    let atrv = atr(high, low, close, atr_p);
    let hh = rolling_max(close, p);
    let ll = rolling_min(close, p);
    let start = p - 1; // first bar with a full p-window

    // Octile flags (open in the bottom/top 1/8 of the p-range) power STRONG.
    let mut sb = vec![false; n];
    let mut ss = vec![false; n];

    let mut m = 0.0_f64;
    let mut ls: i8 = 0; // last signal: 0 = unset (Pine ls = "")
    for i in start..n {
        let (Some(h), Some(l)) = (hh[i], ll[i]) else {
            continue;
        };
        let d = h - l;
        sb[i] = open[i] < l + d / 8.0 && open[i] >= l;
        ss[i] = open[i] > h - d / 8.0 && open[i] <= h;

        // Pine carries the previous FINAL m; the first bar seeds the midline.
        let m_carried = if i == start { (h + l) / 2.0 } else { m };

        // eps uses the PREVIOUS bar's ATR (Pine: mult * atr(atr_p)[1]).
        let a_prev = if i > 0 { atrv[i - 1] } else { None };
        let Some(a_prev) = a_prev else {
            // Only reachable when p - 1 <= atr_p (tiny test params): carry the
            // line without signals until ATR exists.
            m = m_carried;
            out.line[i] = Some(m);
            continue;
        };
        let eps = mult * a_prev;

        // Type A: "crossover(src, m+eps) OR src > m+eps" — the OR-term
        // subsumes the cross. Mutually exclusive since eps >= 0.
        let change_up = close[i] > m_carried + eps;
        let change_down = close[i] < m_carried - eps;

        // Ratchet AFTER the tests, from the carried m.
        m = if change_up {
            m_carried + eps
        } else if change_down {
            m_carried - eps
        } else {
            m_carried
        };

        let prev_ls = ls;
        if change_up {
            ls = 1;
        } else if change_down {
            ls = -1;
        }
        if change_up && prev_ls != 1 {
            out.flip[i] = Some(1);
            out.strong[i] = sb[i.saturating_sub(4).max(start)..=i].iter().any(|&x| x);
        } else if change_down && prev_ls != -1 {
            out.flip[i] = Some(-1);
            out.strong[i] = ss[i.saturating_sub(4).max(start)..=i].iter().any(|&x| x);
        }

        out.line[i] = Some(m);
        if ls != 0 {
            out.dir[i] = Some(ls);
            // Distance to the pending flip threshold using eps_next = mult·ATR[i]
            // (tomorrow's eps, known today).
            if let Some(a_now) = atrv[i] {
                if a_now > 0.0 {
                    let dist = if ls == -1 {
                        ((m + mult * a_now) - close[i]) / a_now
                    } else {
                        (close[i] - (m - mult * a_now)) / a_now
                    };
                    out.dist_flip_atr[i] = Some(dist);
                }
            }
        }
    }

    out
}

/// Precursor events: while waiting for the opposite flip, the FIRST bar where
/// `0 < dist_flip_atr <= precursor_atr` and the close moved toward the
/// threshold fires `(bar, pending_dir)`. One shot per leg (re-armed by the
/// next flip). Gap-through flips (no precursor) and precursors never followed
/// by a flip (false positives) are allowed by design — rule J in the marker
/// experiment measures both.
pub fn qtrend_precursors(close: &[f64], qt: &QTrend, precursor_atr: f64) -> Vec<(usize, i8)> {
    let mut out = Vec::new();
    let mut armed = true;
    for i in 1..close.len() {
        if qt.flip[i].is_some() {
            armed = true; // flip bar re-arms; never fires itself
            continue;
        }
        let (Some(d), Some(dist)) = (qt.dir[i], qt.dist_flip_atr[i]) else {
            continue;
        };
        if !armed {
            continue;
        }
        let pending = -d;
        let toward = if pending > 0 {
            close[i] > close[i - 1]
        } else {
            close[i] < close[i - 1]
        };
        if dist > 0.0 && dist <= precursor_atr && toward {
            out.push((i, pending));
            armed = false;
        }
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

    // ---- Q-Trend ----

    /// Flat 100 → jump to 200 at bar 10 → drop to 100 at bar 20. The jumps
    /// dwarf any eps, so flips are guaranteed at exactly those bars.
    fn qt_step_series() -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let close: Vec<f64> = (0..30)
            .map(|i| if (10..20).contains(&i) { 200.0 } else { 100.0 })
            .collect();
        let open = close.clone();
        let high: Vec<f64> = close.iter().map(|c| c + 1.0).collect();
        let low: Vec<f64> = close.iter().map(|c| c - 1.0).collect();
        (open, high, low, close)
    }

    #[test]
    fn qtrend_none_before_full_window_and_short_input() {
        let (open, high, low, close) = qt_step_series();
        let qt = qtrend(&open, &high, &low, &close, 5, 3, 1.0);
        assert!(qt.line[..4].iter().all(Option::is_none));
        assert!(qt.dir[..4].iter().all(Option::is_none));
        // Input shorter than p → all-None, no panic.
        let qt2 = qtrend(&open[..3], &high[..3], &low[..3], &close[..3], 5, 3, 1.0);
        assert!(qt2.line.iter().all(Option::is_none));
        assert!(qt2.flip.iter().all(Option::is_none));
    }

    #[test]
    fn qtrend_flips_only_on_direction_change() {
        let (open, high, low, close) = qt_step_series();
        let qt = qtrend(&open, &high, &low, &close, 5, 3, 1.0);
        let flips: Vec<(usize, i8)> = qt
            .flip
            .iter()
            .enumerate()
            .filter_map(|(i, f)| f.map(|d| (i, d)))
            .collect();
        assert_eq!(flips, vec![(10, 1), (20, -1)]);
        // Direction holds between flips; None until the first signal.
        assert!(qt.dir[..10].iter().all(Option::is_none));
        assert!(qt.dir[10..20].iter().all(|d| *d == Some(1)));
        assert!(qt.dir[20..].iter().all(|d| *d == Some(-1)));
        // The ratchet converges toward price during the up leg.
        assert!(qt.line[19].unwrap() > qt.line[10].unwrap());
    }

    #[test]
    fn qtrend_ramp_fires_buy_once() {
        let close: Vec<f64> = (0..40).map(|i| 100.0 + 3.0 * i as f64).collect();
        let open = close.clone();
        let high: Vec<f64> = close.iter().map(|c| c + 1.0).collect();
        let low: Vec<f64> = close.iter().map(|c| c - 1.0).collect();
        let qt = qtrend(&open, &high, &low, &close, 5, 3, 1.0);
        let buys = qt.flip.iter().filter(|f| **f == Some(1)).count();
        let sells = qt.flip.iter().filter(|f| **f == Some(-1)).count();
        assert_eq!(buys, 1, "a monotonic ramp flips to buy exactly once");
        assert_eq!(sells, 0);
    }

    #[test]
    fn qtrend_strong_requires_open_in_octile() {
        let (mut open, high, low, close) = qt_step_series();
        let qt = qtrend(&open, &high, &low, &close, 5, 3, 1.0);
        // open == close (200 at the flip bar) sits at the range TOP → not STRONG.
        assert_eq!(qt.flip[10], Some(1));
        assert!(!qt.strong[10]);
        // An open at the very bottom of the 5-bar range (100..200) is in the
        // bottom octile → STRONG buy.
        open[10] = 100.0;
        let qt2 = qtrend(&open, &high, &low, &close, 5, 3, 1.0);
        assert_eq!(qt2.flip[10], Some(1));
        assert!(qt2.strong[10]);
    }

    /// Precursor logic is tested against a hand-built QTrend (it only reads
    /// dir/flip/dist_flip_atr), isolating the arming rules from the ratchet.
    #[test]
    fn qtrend_precursor_fires_once_and_rearms_after_flip() {
        let n = 10;
        let mut qt = QTrend {
            line: vec![None; n],
            dir: vec![Some(-1); n], // downtrend → pending BUY
            flip: vec![None; n],
            strong: vec![false; n],
            dist_flip_atr: vec![Some(2.0); n],
        };
        // close rises toward the threshold from bar 3; dist enters range at 3.
        let close: Vec<f64> = vec![10.0, 9.0, 8.0, 9.0, 9.5, 9.8, 10.5, 10.0, 10.2, 10.4];
        qt.dist_flip_atr[3] = Some(0.4);
        qt.dist_flip_atr[4] = Some(0.3); // still qualifying — must NOT re-fire
        qt.flip[6] = Some(1); // flip re-arms
        for i in 6..n {
            qt.dir[i] = Some(1); // now uptrend → pending SELL
        }
        qt.dist_flip_atr[8] = Some(0.2); // qualifying dist but close RISES (away)
        qt.dist_flip_atr[9] = Some(0.2); // close rises again → never fires

        let pre = qtrend_precursors(&close, &qt, 0.5);
        assert_eq!(pre, vec![(3, 1)], "one shot per leg; wrong-direction momentum ignored");
    }

    #[test]
    fn qtrend_precursor_requires_positive_dist_and_momentum() {
        let n = 6;
        let qt = QTrend {
            line: vec![None; n],
            dir: vec![Some(-1); n],
            flip: vec![None; n],
            strong: vec![false; n],
            // dist <= 0 (already beyond threshold without a flip) never fires.
            dist_flip_atr: vec![Some(-0.1); n],
        };
        let close: Vec<f64> = (0..n).map(|i| 10.0 + i as f64).collect(); // rising
        assert!(qtrend_precursors(&close, &qt, 0.5).is_empty());
    }
}
