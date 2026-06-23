//! Regime-weighted confluence (docs/03 §3, ADR-07).

use crate::config::RegimeWeightTable;
use crate::regime::Regime;
use crate::scoring::composite::CategoryScores;

/// Combine category sub-scores with regime weights.
///
/// In trends the mean-reversion category is **sign-flipped** before weighting:
/// an oversold reading in an up-trend is a continuation (buy), not a fade. The
/// flip combined with the (negative) trend-regime mean-reversion weight damps
/// counter-trend reversion signals (docs/03 §3). Returns `None` only when no
/// category is available for the bar.
pub fn weighted_composite(
    cats: &CategoryScores,
    regime: Regime,
    w: &RegimeWeightTable,
) -> Option<f64> {
    let mr = if regime.is_trend() {
        cats.mean_reversion.map(|v| -v)
    } else {
        cats.mean_reversion
    };

    let mut raw = 0.0;
    let mut any = false;
    if let Some(t) = cats.trend {
        raw += w.trend_w(regime) * t;
        any = true;
    }
    if let Some(m) = cats.momentum {
        raw += w.momentum_w(regime) * m;
        any = true;
    }
    if let Some(r) = mr {
        raw += w.mean_reversion_w(regime) * r;
        any = true;
    }

    any.then_some(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RegimeWeightTable;

    fn cats(t: f64, m: f64, mr: f64) -> CategoryScores {
        CategoryScores {
            trend: Some(t),
            momentum: Some(m),
            mean_reversion: Some(mr),
        }
    }

    #[test]
    fn mean_reversion_sign_flips_in_trend() {
        let w = RegimeWeightTable::default();
        // A positive (buy) reversion reading in an up-trend: weight is −0.20 and
        // the value is flipped, so its contribution is +0.20*value (continuation).
        let c = cats(0.0, 0.0, 0.5);
        let up = weighted_composite(&c, Regime::TrendUp, &w).unwrap();
        assert!((up - (-0.20 * -0.5)).abs() < 1e-12); // = +0.10
                                                      // In range it is taken straight with the positive range weight.
        let rng = weighted_composite(&c, Regime::Range, &w).unwrap();
        assert!((rng - (0.55 * 0.5)).abs() < 1e-12); // = +0.275
    }

    #[test]
    fn none_categories_excluded() {
        let w = RegimeWeightTable::default();
        let c = CategoryScores {
            trend: Some(1.0),
            momentum: None,
            mean_reversion: None,
        };
        let s = weighted_composite(&c, Regime::TrendUp, &w).unwrap();
        assert!((s - 0.40).abs() < 1e-12); // only the trend term
        let empty = CategoryScores {
            trend: None,
            momentum: None,
            mean_reversion: None,
        };
        assert_eq!(weighted_composite(&empty, Regime::Range, &w), None);
    }
}
