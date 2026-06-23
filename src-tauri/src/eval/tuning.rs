//! Walk-forward tuning (P8). Evaluates hypothesis-driven config candidates,
//! **selects on in-sample expectancy, then confirms out-of-sample** so the
//! choice is never made by peeking at the OOS data (docs/06 P8, docs/07 OOS
//! discipline). Candidates are a small, interpretable set — not a blind grid,
//! which would overfit the overlapping samples.

use serde::{Deserialize, Serialize};

use crate::config::ScanConfig;
use crate::eval::{evaluate, EvalConfig};
use crate::models::{AssetClass, Candle};

/// A named configuration to evaluate.
#[derive(Debug, Clone)]
pub struct TuneCandidate {
    pub label: String,
    pub config: ScanConfig,
}

/// IS/OOS outcome for one candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TuneResult {
    pub label: String,
    pub is_expectancy: f64,
    pub is_pf: f64,
    pub is_n: usize,
    pub oos_expectancy: f64,
    pub oos_pf: f64,
    pub oos_n: usize,
}

/// Tuning report. `best` is selected by in-sample expectancy; compare its
/// `oos_expectancy` to `baseline` to judge the improvement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TuneReport {
    pub objective: String,
    pub candidates: Vec<TuneResult>, // sorted by in-sample expectancy (desc)
    pub baseline: TuneResult,
    pub best: TuneResult,
}

fn with(label: &str, base: ScanConfig, f: impl FnOnce(&mut ScanConfig)) -> TuneCandidate {
    let mut config = base;
    f(&mut config);
    TuneCandidate {
        label: label.to_string(),
        config,
    }
}

/// Hypothesis-driven candidates, motivated by the P7 finding that the short
/// side (TrendDown) had negative edge while TrendUp longs were strong.
pub fn default_candidates(base: ScanConfig) -> Vec<TuneCandidate> {
    vec![
        with("baseline", base, |_| {}),
        // Make shorts require more conviction (asymmetric thresholds).
        with("long_bias", base, |c| c.sell_threshold = -55.0),
        // Fewer but higher-conviction signals both sides.
        with("higher_thresholds", base, |c| {
            c.buy_threshold = 50.0;
            c.sell_threshold = -50.0;
        }),
        // Damp counter-trend harder via the weekly gate.
        with("strong_weekly_gate", base, |c| {
            c.mtf.weekly_gate_opposed = 0.25;
        }),
        with("long_bias_and_weekly", base, |c| {
            c.sell_threshold = -55.0;
            c.mtf.weekly_gate_opposed = 0.25;
        }),
        // Distrust direction harder inside a squeeze.
        with("tight_quality_gate", base, |c| c.squeeze_gate = 0.6),
        // Lean further into trend continuation (deeper mean-reversion damp).
        with("trend_continuation", base, |c| {
            c.weights.mean_reversion[0] = -0.30; // TrendUp
            c.weights.mean_reversion[1] = -0.30; // TrendDown
        }),
    ]
}

/// Run walk-forward tuning over the universe.
pub fn tune(
    universe: &[(AssetClass, Vec<Candle>)],
    candidates: &[TuneCandidate],
    ecfg: &EvalConfig,
) -> TuneReport {
    let mut results: Vec<TuneResult> = candidates
        .iter()
        .map(|cand| {
            let rep = evaluate(universe, &cand.config, ecfg);
            TuneResult {
                label: cand.label.clone(),
                is_expectancy: rep.in_sample.expectancy,
                is_pf: rep.in_sample.profit_factor,
                is_n: rep.in_sample.n,
                oos_expectancy: rep.out_of_sample.expectancy,
                oos_pf: rep.out_of_sample.profit_factor,
                oos_n: rep.out_of_sample.n,
            }
        })
        .collect();

    let baseline = results
        .iter()
        .find(|r| r.label == "baseline")
        .or(results.first())
        .cloned()
        .expect("at least one candidate");

    // Selection happens on IS only (walk-forward discipline).
    results.sort_by(|a, b| {
        b.is_expectancy
            .partial_cmp(&a.is_expectancy)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let best = results.first().cloned().expect("at least one candidate");

    TuneReport {
        objective: "in-sample expectancy (confirmed out-of-sample)".to_string(),
        candidates: results,
        baseline,
        best,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_are_distinct_and_include_baseline() {
        let cands = default_candidates(ScanConfig::default());
        assert!(cands.iter().any(|c| c.label == "baseline"));
        // long_bias actually changes the sell threshold.
        let lb = cands.iter().find(|c| c.label == "long_bias").unwrap();
        assert_eq!(lb.config.sell_threshold, -55.0);
        assert_eq!(ScanConfig::default().sell_threshold, -40.0);
    }

    #[test]
    fn tune_selects_on_in_sample_and_runs() {
        let candles: Vec<Candle> = (0..400)
            .map(|i| {
                let c = 100.0 + i as f64;
                Candle::ohlcv(i as i64 * 86_400, c, c + 1.0, c - 1.0, c, 1_000_000.0)
            })
            .collect();
        let universe = vec![(AssetClass::Equity, candles)];
        let cands = default_candidates(ScanConfig::default());
        let report = tune(&universe, &cands, &EvalConfig::default());
        assert_eq!(report.candidates.len(), cands.len());
        // sorted descending by IS expectancy.
        for w in report.candidates.windows(2) {
            assert!(w[0].is_expectancy >= w[1].is_expectancy);
        }
        assert!(serde_json::to_string(&report).is_ok());
    }
}
