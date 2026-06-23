//! Validation harness (P7) — ported in spirit from `prediction-eval`. Walks
//! historical bars, treats the model's directional state as an entry, measures
//! the forward N-bar outcome, and reports whether there is statistical edge
//! **before** any parameter tuning (P8). See docs/07-testing.md, CLAUDE.md #4.
//!
//! Caveat: per-bar Active samples overlap (autocorrelated), so significance on
//! the broad sample set is optimistic; the headline stats use discrete
//! threshold-cross events.

pub mod tuning;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::ScanConfig;
use crate::indicators::trend::adx_dmi;
use crate::indicators::volatility::choppiness;
use crate::models::{AssetClass, Candle};
use crate::proximity::{proximity_series, SignalState};
use crate::regime::{regime_series, Regime};

/// Evaluation parameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EvalConfig {
    /// Forward holding horizon in bars.
    pub horizon_bars: usize,
    /// Fraction of each symbol's tail reserved as out-of-sample.
    pub oos_fraction: f64,
    /// Bars to skip at the start (indicator warm-up).
    pub min_history: usize,
    /// Proximity ≥ this is the "high" bucket.
    pub prox_high: f64,
    /// Proximity < this is the "low" bucket.
    pub prox_low: f64,
}

impl Default for EvalConfig {
    fn default() -> Self {
        Self {
            horizon_bars: 10,
            oos_fraction: 0.3,
            min_history: 60,
            prox_high: 70.0,
            prox_low: 40.0,
        }
    }
}

/// One historical entry sample.
#[derive(Debug, Clone, Copy)]
struct Sample {
    direction: i8,
    ret: f64, // signed forward return, percent
    mfe: f64, // max favorable excursion, percent
    mae: f64, // max adverse excursion, percent (negative)
    proximity: f64,
    state: SignalState,
    regime: Option<Regime>,
    asset_class: AssetClass,
    is_cross: bool,
    is_oos: bool,
}

/// Aggregated statistics for a set of samples.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub n: usize,
    pub hit_rate: f64,
    pub binomial_p: f64,
    pub avg_return: f64,
    pub expectancy: f64,
    pub profit_factor: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
    pub avg_mfe: f64,
    pub avg_mae: f64,
}

/// Degeneracy guardrails (docs/07 "退化チェック").
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Degeneracy {
    pub buy_fraction: f64,
    pub signals_per_symbol: f64,
    pub proximity_saturation: f64,
}

/// The full evaluation report (docs/07 output).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalReport {
    pub horizon_bars: usize,
    pub n_symbols: usize,
    pub overall: Stats,
    pub in_sample: Stats,
    pub out_of_sample: Stats,
    pub by_regime: Vec<(String, Stats)>,
    pub by_asset_class: Vec<(String, Stats)>,
    pub by_state: Vec<(String, Stats)>,
    pub proximity_lift: Vec<(String, Stats)>,
    pub degeneracy: Degeneracy,
}

/// Replace non-finite floats so the report serializes to JSON cleanly.
fn finite(x: f64) -> f64 {
    if x.is_finite() {
        x
    } else {
        0.0
    }
}

/// Standard normal CDF via the Abramowitz–Stegun erf approximation (7.1.26).
fn normal_cdf(z: f64) -> f64 {
    let sign = if z < 0.0 { -1.0 } else { 1.0 };
    let x = z.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-x * x).exp();
    0.5 * (1.0 + sign * y)
}

/// Two-sided binomial p-value vs p = 0.5 (normal approx + continuity correction).
fn binomial_two_sided_p(hits: usize, n: usize) -> f64 {
    if n == 0 {
        return 1.0;
    }
    let sd = (n as f64 * 0.25).sqrt();
    if sd == 0.0 {
        return 1.0;
    }
    let z = ((hits as f64 - n as f64 * 0.5).abs() - 0.5).max(0.0) / sd;
    (2.0 * (1.0 - normal_cdf(z))).clamp(0.0, 1.0)
}

fn stats(samples: &[&Sample]) -> Stats {
    let n = samples.len();
    if n == 0 {
        return Stats {
            n: 0,
            hit_rate: 0.0,
            binomial_p: 1.0,
            avg_return: 0.0,
            expectancy: 0.0,
            profit_factor: 0.0,
            avg_win: 0.0,
            avg_loss: 0.0,
            avg_mfe: 0.0,
            avg_mae: 0.0,
        };
    }
    let nf = n as f64;
    let hits = samples.iter().filter(|s| s.ret > 0.0).count();
    let wins: Vec<f64> = samples
        .iter()
        .filter(|s| s.ret > 0.0)
        .map(|s| s.ret)
        .collect();
    let losses: Vec<f64> = samples
        .iter()
        .filter(|s| s.ret < 0.0)
        .map(|s| -s.ret)
        .collect();
    let sum_w: f64 = wins.iter().sum();
    let sum_l: f64 = losses.iter().sum();
    let win_rate = wins.len() as f64 / nf;
    let loss_rate = losses.len() as f64 / nf;
    let avg_win = if wins.is_empty() {
        0.0
    } else {
        sum_w / wins.len() as f64
    };
    let avg_loss = if losses.is_empty() {
        0.0
    } else {
        sum_l / losses.len() as f64
    };
    let profit_factor = if sum_l > 0.0 {
        (sum_w / sum_l).min(999.0)
    } else if sum_w > 0.0 {
        999.0
    } else {
        0.0
    };
    Stats {
        n,
        hit_rate: hits as f64 / nf,
        binomial_p: binomial_two_sided_p(hits, n),
        avg_return: finite(samples.iter().map(|s| s.ret).sum::<f64>() / nf),
        expectancy: finite(win_rate * avg_win - loss_rate * avg_loss),
        profit_factor: finite(profit_factor),
        avg_win: finite(avg_win),
        avg_loss: finite(avg_loss),
        avg_mfe: finite(samples.iter().map(|s| s.mfe).sum::<f64>() / nf),
        avg_mae: finite(samples.iter().map(|s| s.mae).sum::<f64>() / nf),
    }
}

fn collect_samples(
    candles: &[Candle],
    asset: AssetClass,
    cfg: &ScanConfig,
    ecfg: &EvalConfig,
    out: &mut Vec<Sample>,
) {
    let n = candles.len();
    if n < ecfg.min_history + ecfg.horizon_bars + 1 {
        return;
    }
    let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
    let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
    let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

    let prox = proximity_series(candles, cfg);
    let dmi = adx_dmi(&high, &low, &close, cfg.indicators.adx_period);
    let chop = choppiness(&high, &low, &close, cfg.indicators.choppiness_period);
    let regimes = regime_series(&dmi, &chop, &cfg.regime);
    let oos_start = (n as f64 * (1.0 - ecfg.oos_fraction)) as usize;

    let h = ecfg.horizon_bars;
    for i in ecfg.min_history..(n - h) {
        let dir = prox[i].state.direction().sign();
        if dir == 0 {
            continue;
        }
        let entry = close[i];
        if entry <= 0.0 {
            continue;
        }
        let df = dir as f64;
        let ret = (close[i + h] - entry) / entry * df * 100.0;
        let mut mfe = f64::MIN;
        let mut mae = f64::MAX;
        for k in 1..=h {
            let fav = if dir > 0 {
                (high[i + k] - entry) / entry
            } else {
                (entry - low[i + k]) / entry
            } * 100.0;
            let adv = if dir > 0 {
                (low[i + k] - entry) / entry
            } else {
                (entry - high[i + k]) / entry
            } * 100.0;
            mfe = mfe.max(fav);
            mae = mae.min(adv);
        }
        out.push(Sample {
            direction: dir,
            ret,
            mfe,
            mae,
            proximity: prox[i].proximity_score,
            state: prox[i].state,
            regime: regimes[i],
            asset_class: asset,
            is_cross: prox[i].bars_since_trigger == Some(0),
            is_oos: i >= oos_start,
        });
    }
}

fn grouped<K: Ord, F: Fn(&Sample) -> Option<K>>(
    samples: &[Sample],
    label: impl Fn(&K) -> String,
    key: F,
) -> Vec<(String, Stats)> {
    let mut groups: BTreeMap<K, Vec<&Sample>> = BTreeMap::new();
    for s in samples {
        if let Some(k) = key(s) {
            groups.entry(k).or_default().push(s);
        }
    }
    groups
        .into_iter()
        .map(|(k, v)| (label(&k), stats(&v)))
        .collect()
}

fn state_tier(state: SignalState) -> Option<&'static str> {
    match state {
        SignalState::TriggeredBuy | SignalState::TriggeredSell => Some("Triggered"),
        SignalState::ActiveBuy | SignalState::ActiveSell => Some("Active"),
        SignalState::PrimedBuy | SignalState::PrimedSell => Some("Primed"),
        SignalState::Neutral => None,
    }
}

/// Run the full evaluation over a universe of (asset_class, daily candles).
pub fn evaluate(
    universe: &[(AssetClass, Vec<Candle>)],
    cfg: &ScanConfig,
    ecfg: &EvalConfig,
) -> EvalReport {
    let mut samples = Vec::new();
    for (asset, candles) in universe {
        collect_samples(candles, *asset, cfg, ecfg, &mut samples);
    }

    // Headline stats over every directional entry sample. (Discrete threshold
    // crosses are sparse — one per regime change — so they feed the degeneracy
    // signal-frequency metric, not the headline; the overlap caveat applies.)
    let all: Vec<&Sample> = samples.iter().collect();
    let overall = stats(&all);
    let in_sample = stats(&samples.iter().filter(|s| !s.is_oos).collect::<Vec<_>>());
    let out_of_sample = stats(&samples.iter().filter(|s| s.is_oos).collect::<Vec<_>>());

    let by_regime = grouped(&samples, |r: &Regime| format!("{r:?}"), |s| s.regime);
    let by_asset_class = grouped(
        &samples,
        |a: &AssetClass| a.as_str().to_string(),
        |s| Some(s.asset_class),
    );

    // Proximity validity / freshness stratification.
    let by_state = grouped(
        &samples,
        |t: &&'static str| t.to_string(),
        |s| state_tier(s.state),
    );
    let proximity_lift = grouped(
        &samples,
        |b: &u8| match b {
            2 => format!("high (>= {})", ecfg.prox_high),
            1 => "mid".to_string(),
            _ => format!("low (< {})", ecfg.prox_low),
        },
        |s| {
            Some(if s.proximity >= ecfg.prox_high {
                2u8
            } else if s.proximity < ecfg.prox_low {
                0u8
            } else {
                1u8
            })
        },
    );

    let n_symbols = universe.len();
    let buys = samples.iter().filter(|s| s.direction > 0).count();
    let cross_count = samples.iter().filter(|s| s.is_cross).count();
    let degeneracy = Degeneracy {
        buy_fraction: if samples.is_empty() {
            0.0
        } else {
            buys as f64 / samples.len() as f64
        },
        signals_per_symbol: if n_symbols == 0 {
            0.0
        } else {
            cross_count as f64 / n_symbols as f64
        },
        proximity_saturation: if samples.is_empty() {
            0.0
        } else {
            samples.iter().filter(|s| s.proximity >= 90.0).count() as f64 / samples.len() as f64
        },
    };

    EvalReport {
        horizon_bars: ecfg.horizon_bars,
        n_symbols,
        overall,
        in_sample,
        out_of_sample,
        by_regime,
        by_asset_class,
        by_state,
        proximity_lift,
        degeneracy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proximity::SignalState;

    fn sample(ret: f64, dir: i8, prox: f64, cross: bool, oos: bool) -> Sample {
        Sample {
            direction: dir,
            ret,
            mfe: ret.max(0.0),
            mae: ret.min(0.0),
            proximity: prox,
            state: if cross {
                SignalState::TriggeredBuy
            } else {
                SignalState::ActiveBuy
            },
            regime: Some(Regime::TrendUp),
            asset_class: AssetClass::Equity,
            is_cross: cross,
            is_oos: oos,
        }
    }

    #[test]
    fn stats_math() {
        let s = [sample(1.0, 1, 90.0, true, false); 3];
        let mut v = s.to_vec();
        v.push(sample(-1.0, 1, 90.0, true, false));
        let refs: Vec<&Sample> = v.iter().collect();
        let st = stats(&refs);
        assert_eq!(st.n, 4);
        assert!((st.hit_rate - 0.75).abs() < 1e-12);
        assert!((st.expectancy - 0.5).abs() < 1e-12); // 0.75*1 - 0.25*1
        assert!((st.profit_factor - 3.0).abs() < 1e-12); // 3 / 1
    }

    #[test]
    fn binomial_and_normal_cdf() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-9);
        assert!((normal_cdf(1.96) - 0.975).abs() < 2e-3);
        assert!(binomial_two_sided_p(50, 100) > 0.5); // no deviation
        assert!(binomial_two_sided_p(75, 100) < 0.01); // strong deviation
    }

    #[test]
    fn no_inf_or_nan_in_stats() {
        // All wins → profit factor capped, no Inf leaks into JSON.
        let v: Vec<Sample> = (0..5).map(|_| sample(2.0, 1, 80.0, true, false)).collect();
        let refs: Vec<&Sample> = v.iter().collect();
        let st = stats(&refs);
        assert!(st.profit_factor.is_finite());
        assert_eq!(st.profit_factor, 999.0);
        let json = serde_json::to_string(&st).unwrap();
        assert!(!json.contains("inf") && !json.contains("NaN"));
    }

    #[test]
    fn evaluate_runs_on_trend_universe() {
        let cfg = ScanConfig::default();
        let ecfg = EvalConfig::default();
        let candles: Vec<Candle> = (0..400)
            .map(|i| {
                let c = 100.0 + i as f64;
                Candle::ohlcv(i as i64 * 86_400, c, c + 1.0, c - 1.0, c, 1_000_000.0)
            })
            .collect();
        let report = evaluate(&[(AssetClass::Equity, candles)], &cfg, &ecfg);
        assert_eq!(report.n_symbols, 1);
        // A clean up-trend: buy signals should mostly be profitable forward.
        assert!(report.overall.n > 0);
        assert!(report.overall.hit_rate > 0.5);
        assert!(report.degeneracy.buy_fraction > 0.9);
        // Report serializes cleanly (no NaN/Inf).
        assert!(serde_json::to_string(&report).is_ok());
    }
}
