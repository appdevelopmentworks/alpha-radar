//! Direction scoring: regime-weighted confluence (single TF) + MTF composite
//! (α weighting + weekly gate + monthly soft modifier). Implemented in P2.
//! Pipeline order per docs/03-scoring.md, ADR-07/09.

pub mod composite;
pub mod mtf;
pub mod weights;

use serde::{Deserialize, Serialize};

use crate::config::ScanConfig;
use crate::indicators::trend::adx_dmi;
use crate::indicators::volatility::choppiness;
use crate::models::Candle;
use crate::regime::{regime_series, Regime};

/// Direction score for a symbol across timeframes. `score_final` is `None` only
/// when the daily timeframe has no score (insufficient history).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DirectionScore {
    pub score_daily: Option<f64>,
    pub score_weekly: Option<f64>,
    pub score_monthly: Option<f64>,
    pub score_final: Option<f64>,
    pub regime_daily: Option<Regime>,
    pub regime_weekly: Option<Regime>,
    pub regime_monthly: Option<Regime>,
}

/// Latest-bar single-TF score (the most recent bar; `None` if it has none).
fn latest_score(candles: &[Candle], cfg: &ScanConfig) -> Option<f64> {
    composite::single_tf_score(candles, cfg)
        .last()
        .copied()
        .flatten()
}

/// Latest-bar regime for a timeframe.
fn latest_regime(candles: &[Candle], cfg: &ScanConfig) -> Option<Regime> {
    let (high, low, close) = (
        candles.iter().map(|c| c.high).collect::<Vec<_>>(),
        candles.iter().map(|c| c.low).collect::<Vec<_>>(),
        candles.iter().map(|c| c.close).collect::<Vec<_>>(),
    );
    let dmi = adx_dmi(&high, &low, &close, cfg.indicators.adx_period);
    let chop = choppiness(&high, &low, &close, cfg.indicators.choppiness_period);
    regime_series(&dmi, &chop, &cfg.regime)
        .last()
        .copied()
        .flatten()
}

/// Compute the full multi-timeframe direction score for a symbol. `weekly` /
/// `monthly` are optional — a symbol missing them degrades via the MTF gates
/// (docs/03 §5, ADR-09).
pub fn direction_score(
    daily: &[Candle],
    weekly: Option<&[Candle]>,
    monthly: Option<&[Candle]>,
    cfg: &ScanConfig,
) -> DirectionScore {
    let score_daily = latest_score(daily, cfg);
    let score_weekly = weekly.and_then(|w| latest_score(w, cfg));
    let score_monthly = monthly.and_then(|m| latest_score(m, cfg));
    let regime_daily = latest_regime(daily, cfg);
    let regime_weekly = weekly.and_then(|w| latest_regime(w, cfg));
    let regime_monthly = monthly.and_then(|m| latest_regime(m, cfg));

    let score_final = score_daily.map(|d| {
        mtf::mtf_combine(
            d,
            score_weekly,
            score_monthly,
            regime_weekly,
            regime_monthly,
            &cfg.mtf,
        )
    });

    DirectionScore {
        score_daily,
        score_weekly,
        score_monthly,
        score_final,
        regime_daily,
        regime_weekly,
        regime_monthly,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Linear trend candles (slope > 0 up, < 0 down) with a constant intrabar
    /// range — enough to drive ADX into a clear trend regime.
    fn trend_candles(n: usize, start: f64, slope: f64) -> Vec<Candle> {
        (0..n)
            .map(|i| {
                let c = start + slope * i as f64;
                Candle::ohlcv(i as i64 * 86_400, c, c + 1.0, c - 1.0, c, 1_000_000.0)
            })
            .collect()
    }

    /// Oscillating candles around a level → low ADX → Range regime.
    fn range_candles(n: usize) -> Vec<Candle> {
        (0..n)
            .map(|i| {
                let c = 100.0 + (i as f64 * 0.6).sin() * 2.0;
                Candle::ohlcv(i as i64 * 86_400, c, c + 1.0, c - 1.0, c, 1_000_000.0)
            })
            .collect()
    }

    #[test]
    fn opposed_weekly_attenuates_vs_aligned() {
        let cfg = ScanConfig::default();
        let daily = trend_candles(260, 100.0, 1.0); // up
        let weekly_up = trend_candles(260, 100.0, 1.0); // aligned
        let weekly_down = trend_candles(260, 400.0, -1.0); // opposed

        let aligned = direction_score(&daily, Some(&weekly_up), None, &cfg)
            .score_final
            .unwrap();
        let opposed = direction_score(&daily, Some(&weekly_down), None, &cfg)
            .score_final
            .unwrap();

        assert!(aligned > 0.0, "aligned should stay positive: {aligned}");
        assert!(
            opposed < aligned,
            "opposed weekly must attenuate: opposed={opposed}, aligned={aligned}"
        );
    }

    #[test]
    fn daily_only_degrades_to_neutral_weekly_gate() {
        let cfg = ScanConfig::default();
        let daily = trend_candles(260, 100.0, 1.0);
        let ds = direction_score(&daily, None, None, &cfg);
        // Daily-only blend == daily score; neutral weekly gate (0.8) applies.
        let expected = ds.score_daily.unwrap() * cfg.mtf.weekly_gate_neutral;
        assert!((ds.score_final.unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn oscillating_series_is_range_regime() {
        let cfg = ScanConfig::default();
        let candles = range_candles(260);
        assert_eq!(latest_regime(&candles, &cfg), Some(Regime::Range));
    }
}
