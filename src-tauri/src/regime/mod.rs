//! Regime detection — the first stage of the scoring pipeline (docs/03 §1,
//! ADR-07). Mean-reversion and trend signals are opposite by nature, so the
//! regime is detected *before* aggregation and drives the category weights.

use serde::{Deserialize, Serialize};

use crate::indicators::trend::Dmi;

/// Market regime. Index order (`index()`) matches the columns of
/// [`crate::config::RegimeWeightTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Regime {
    TrendUp,
    TrendDown,
    Range,
    Transition,
}

impl Regime {
    /// Column index into the regime weight table (TrendUp, TrendDown, Range,
    /// Transition).
    pub fn index(self) -> usize {
        match self {
            Regime::TrendUp => 0,
            Regime::TrendDown => 1,
            Regime::Range => 2,
            Regime::Transition => 3,
        }
    }

    /// Whether this is a directional trend regime.
    pub fn is_trend(self) -> bool {
        matches!(self, Regime::TrendUp | Regime::TrendDown)
    }

    /// Directional sign: +1 up-trend, −1 down-trend, 0 otherwise.
    pub fn direction(self) -> i8 {
        match self {
            Regime::TrendUp => 1,
            Regime::TrendDown => -1,
            _ => 0,
        }
    }
}

/// Thresholds for [`detect_regime`] (docs/03 §1). ADX splits trend/range;
/// Choppiness breaks ties in the transition band.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RegimeThresholds {
    /// ADX above this ⇒ trend. Default 25.
    pub adx_trend: f64,
    /// ADX below this ⇒ range. Default 20.
    pub adx_range: f64,
    /// Choppiness above this ⇒ range-leaning. Default 61.8.
    pub chop_high: f64,
    /// Choppiness below this ⇒ trend-leaning. Default 38.2.
    pub chop_low: f64,
}

impl Default for RegimeThresholds {
    fn default() -> Self {
        Self {
            adx_trend: 25.0,
            adx_range: 20.0,
            chop_high: 61.8,
            chop_low: 38.2,
        }
    }
}

/// Detect the regime for a single bar (docs/03 §1).
///
/// ADX > `adx_trend` ⇒ trend (direction from ±DI); ADX < `adx_range` ⇒ range;
/// in the transition band, Choppiness tips the decision toward trend (low CHOP)
/// or range (high CHOP), otherwise `Transition`.
pub fn detect_regime(
    adx: f64,
    plus_di: f64,
    minus_di: f64,
    chop: Option<f64>,
    t: &RegimeThresholds,
) -> Regime {
    let directional = |p: f64, m: f64| {
        if p >= m {
            Regime::TrendUp
        } else {
            Regime::TrendDown
        }
    };
    if adx > t.adx_trend {
        directional(plus_di, minus_di)
    } else if adx < t.adx_range {
        Regime::Range
    } else {
        match chop {
            Some(c) if c < t.chop_low => directional(plus_di, minus_di),
            Some(c) if c > t.chop_high => Regime::Range,
            _ => Regime::Transition,
        }
    }
}

/// Per-bar regime series. `None` where ADX/DI are not yet available; `chop`
/// must align 1:1 with `dmi`.
pub fn regime_series(dmi: &Dmi, chop: &[Option<f64>], t: &RegimeThresholds) -> Vec<Option<Regime>> {
    (0..dmi.adx.len())
        .map(|i| match (dmi.adx[i], dmi.plus_di[i], dmi.minus_di[i]) {
            (Some(a), Some(p), Some(m)) => Some(detect_regime(a, p, m, chop[i], t)),
            _ => None,
        })
        .collect()
}

/// Continuous regime strength `clamp((ADX-20)/30, 0, 1)` (docs/03 §1, optional
/// weight interpolation).
pub fn regime_strength(adx: f64) -> f64 {
    ((adx - 20.0) / 30.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn th() -> RegimeThresholds {
        RegimeThresholds::default()
    }

    #[test]
    fn strong_adx_picks_direction() {
        assert_eq!(
            detect_regime(30.0, 25.0, 10.0, None, &th()),
            Regime::TrendUp
        );
        assert_eq!(
            detect_regime(30.0, 10.0, 25.0, None, &th()),
            Regime::TrendDown
        );
    }

    #[test]
    fn low_adx_is_range() {
        assert_eq!(detect_regime(15.0, 20.0, 18.0, None, &th()), Regime::Range);
    }

    #[test]
    fn transition_band_resolved_by_choppiness() {
        // ADX in 20..25: CHOP low ⇒ trend, high ⇒ range, mid ⇒ transition.
        assert_eq!(
            detect_regime(22.0, 20.0, 10.0, Some(30.0), &th()),
            Regime::TrendUp
        );
        assert_eq!(
            detect_regime(22.0, 20.0, 10.0, Some(70.0), &th()),
            Regime::Range
        );
        assert_eq!(
            detect_regime(22.0, 20.0, 10.0, Some(50.0), &th()),
            Regime::Transition
        );
        assert_eq!(
            detect_regime(22.0, 20.0, 10.0, None, &th()),
            Regime::Transition
        );
    }
}
