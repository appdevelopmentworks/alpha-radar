//! Configuration for indicator / regime / score parameters.
//!
//! Project guardrail: no parameter is hard-coded at a call site — defaults live
//! here and flow in via config (CLAUDE.md "No magic numbers"). Defaults follow
//! docs/02-indicators.md and docs/03-scoring.md. All values are tunable in P8.

use serde::{Deserialize, Serialize};

use crate::regime::{Regime, RegimeThresholds};

/// Periods / multipliers for every indicator. Defaults per docs/02.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IndicatorParams {
    // base
    pub rsi_period: usize,
    pub atr_period: usize,
    pub sma_period: usize,
    pub linreg_period: usize,
    // trend
    pub adx_period: usize,
    pub ema_ribbon: [usize; 3],
    pub supertrend_atr: usize,
    pub supertrend_mult: f64,
    pub ichimoku: [usize; 3],
    pub ichimoku_displacement: usize,
    /// Q-Trend (chart display layer, ADR-15): ratcheting trend line over the
    /// `qtrend_period` close range, band width `qtrend_mult × ATR(qtrend_atr)`.
    /// Field-level serde defaults keep configs saved before these fields
    /// deserializable (`load_active_config` would silently fall back to the
    /// Standard preset otherwise).
    #[serde(default = "default_qtrend_period")]
    pub qtrend_period: usize,
    #[serde(default = "default_qtrend_atr")]
    pub qtrend_atr: usize,
    #[serde(default = "default_qtrend_mult")]
    pub qtrend_mult: f64,
    /// Precursor: fire when close is within this fraction of ATR of the
    /// pending flip threshold while moving toward it (ADR-15).
    #[serde(default = "default_qtrend_precursor_atr")]
    pub qtrend_precursor_atr: f64,
    /// ST-flip display layer (ADR-16): its own Supertrend params so the marker
    /// timing approximates ATS (its status line reads "2 10" ⇒ mult 2 ×
    /// ATR 10) without touching the scoring/ADR-14 Supertrend (10 / 3.0).
    #[serde(default = "default_stflip_atr")]
    pub stflip_atr: usize,
    #[serde(default = "default_stflip_mult")]
    pub stflip_mult: f64,
    // momentum
    pub macd_fast: usize,
    pub macd_slow: usize,
    pub macd_signal: usize,
    pub macd_k: f64,
    pub squeeze_length: usize,
    pub squeeze_mult_bb: f64,
    pub squeeze_mult_kc: f64,
    pub squeeze_k: f64,
    pub tsi_long: usize,
    pub tsi_short: usize,
    // mean reversion
    pub connors_rsi: usize,
    pub connors_ma: usize,
    pub bb_period: usize,
    pub bb_mult: f64,
    pub williams_period: usize,
    pub zscore_sma: usize,
    pub zscore_std: usize,
    // volatility / gates
    pub keltner_ema: usize,
    pub keltner_atr: usize,
    pub keltner_mult: f64,
    pub choppiness_period: usize,
}

impl Default for IndicatorParams {
    fn default() -> Self {
        Self {
            rsi_period: 14,
            atr_period: 14,
            sma_period: 20,
            linreg_period: 20,
            adx_period: 14,
            ema_ribbon: [20, 50, 200],
            supertrend_atr: 10,
            supertrend_mult: 3.0,
            ichimoku: [9, 26, 52],
            ichimoku_displacement: 26,
            qtrend_period: default_qtrend_period(),
            qtrend_atr: default_qtrend_atr(),
            qtrend_mult: default_qtrend_mult(),
            qtrend_precursor_atr: default_qtrend_precursor_atr(),
            stflip_atr: default_stflip_atr(),
            stflip_mult: default_stflip_mult(),
            macd_fast: 12,
            macd_slow: 26,
            macd_signal: 9,
            macd_k: 0.5,
            squeeze_length: 20,
            squeeze_mult_bb: 2.0,
            squeeze_mult_kc: 1.5,
            squeeze_k: 1.0,
            tsi_long: 25,
            tsi_short: 13,
            connors_rsi: 2,
            connors_ma: 200,
            bb_period: 20,
            bb_mult: 2.0,
            williams_period: 14,
            zscore_sma: 20,
            zscore_std: 100,
            keltner_ema: 20,
            keltner_atr: 20,
            keltner_mult: 2.0,
            choppiness_period: 14,
        }
    }
}

/// Q-Trend defaults match the TradingView script's stock settings
/// (close / 200 / ATR 14 / mult 1.0, Type A) so marker timing lines up with
/// the reference chart.
fn default_qtrend_period() -> usize {
    200
}
fn default_qtrend_atr() -> usize {
    14
}
fn default_qtrend_mult() -> f64 {
    1.0
}
/// Precursor default: within half an ATR of the pending flip threshold.
fn default_qtrend_precursor_atr() -> f64 {
    0.5
}
/// ST-flip display defaults approximate ATS's visible inputs ("2 10"):
/// a tighter band than the scoring Supertrend, flipping earlier and oftener.
fn default_stflip_atr() -> usize {
    10
}
fn default_stflip_mult() -> f64 {
    2.0
}

/// Regime-dependent category weights (docs/03 §3). Columns are indexed by
/// [`Regime::index`]: TrendUp, TrendDown, Range, Transition. Negative
/// mean-reversion weights in trends combine with the sign-flip in
/// `scoring::weights` to damp counter-trend reversion (ADR-07).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RegimeWeightTable {
    pub trend: [f64; 4],
    pub momentum: [f64; 4],
    pub mean_reversion: [f64; 4],
}

impl Default for RegimeWeightTable {
    fn default() -> Self {
        Self {
            trend: [0.40, 0.40, 0.10, 0.25],
            momentum: [0.40, 0.40, 0.20, 0.30],
            mean_reversion: [-0.20, -0.20, 0.55, 0.30],
        }
    }
}

impl RegimeWeightTable {
    pub fn trend_w(&self, r: Regime) -> f64 {
        self.trend[r.index()]
    }
    pub fn momentum_w(&self, r: Regime) -> f64 {
        self.momentum[r.index()]
    }
    pub fn mean_reversion_w(&self, r: Regime) -> f64 {
        self.mean_reversion[r.index()]
    }
}

/// Multi-timeframe combination config (docs/03 §5, ADR-09). `alpha` is
/// [daily, weekly, monthly]. Weekly is a strong gate; monthly is a soft
/// modifier (never a hard gate) and can be disabled.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MtfConfig {
    pub alpha: [f64; 3],
    pub weekly_gate_aligned: f64,
    pub weekly_gate_neutral: f64,
    pub weekly_gate_opposed: f64,
    pub monthly_mod_aligned: f64,
    pub monthly_mod_opposed: f64,
    pub monthly_enabled: bool,
}

impl Default for MtfConfig {
    fn default() -> Self {
        Self {
            alpha: [0.55, 0.30, 0.15],
            weekly_gate_aligned: 1.0,
            weekly_gate_neutral: 0.8,
            weekly_gate_opposed: 0.4,
            monthly_mod_aligned: 1.1,
            monthly_mod_opposed: 0.85,
            monthly_enabled: true,
        }
    }
}

/// Entry-imminence (proximity) parameters (docs/03 §6). All tunable in P8.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProximityConfig {
    /// Lower bound from which `p_thresh` starts measuring approach.
    pub approach_floor: f64,
    /// Lookback (bars) for the `p_thresh` velocity bonus.
    pub velocity_bars: usize,
    /// Score-point change over `velocity_bars` that earns the full bonus.
    pub velocity_scale: f64,
    /// Max `p_thresh` velocity multiplier (1.0 = none).
    pub velocity_max_bonus: f64,
    /// Typical squeeze length (bars) for the `p_sqz` ramp.
    pub typical_squeeze_len: f64,
    /// Connors RSI(2) buy trigger zone (price > MA filter).
    pub cr_buy_zone: f64,
    /// Connors RSI(2) sell trigger zone (price < MA filter).
    pub cr_sell_zone: f64,
    /// Max distance to the key MA (in ATR) for `p_pull`.
    pub pull_max_dist_atr: f64,
    /// Bars within which a threshold cross counts as "fresh" (Triggered).
    pub fresh_bars_n: u32,
    /// Per-bar decay applied to an Active (late) signal's proximity.
    pub active_decay: f64,
    /// Min aggregated proximity to call a no-trigger setup "Primed".
    pub primed_floor: f64,
    /// Proximity floor forced on a freshly Triggered signal.
    pub triggered_floor: f64,
}

impl Default for ProximityConfig {
    fn default() -> Self {
        Self {
            approach_floor: 0.0,
            velocity_bars: 3,
            velocity_scale: 20.0,
            velocity_max_bonus: 1.3,
            typical_squeeze_len: 20.0,
            cr_buy_zone: 10.0,
            cr_sell_zone: 90.0,
            pull_max_dist_atr: 2.0,
            fresh_bars_n: 2,
            active_decay: 0.95,
            primed_floor: 0.5,
            triggered_floor: 90.0,
        }
    }
}

/// Top-level scan configuration. Snapshotted per scan run for reproducibility
/// (ADR-10).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScanConfig {
    pub indicators: IndicatorParams,
    pub regime: RegimeThresholds,
    pub weights: RegimeWeightTable,
    pub mtf: MtfConfig,
    pub proximity: ProximityConfig,
    /// Buy/sell thresholds on `Score_final` (docs/00 §5, default ±40).
    pub buy_threshold: f64,
    pub sell_threshold: f64,
    /// Quality-gate factor applied to the direction score while a squeeze is on
    /// (low-volatility, direction unclear). 1.0 = no reduction.
    pub squeeze_gate: f64,
    /// Minimum daily bars required to score a symbol; fewer ⇒ `RowError`
    /// (docs/03 §7).
    pub min_bars: usize,
    /// ATR multiple for the suggested stop distance.
    pub stop_atr_mult: f64,
    /// Display-only: how many of the most recent bars the chart shows on open
    /// (a swing view fits ~100–120 bars). Not a computation input — the chart
    /// still receives the full series and only zooms the initial visible range.
    /// `#[serde(default)]` keeps configs saved before this field deserializable.
    #[serde(default = "default_chart_bars")]
    pub chart_bars: usize,
    /// Forward window (daily bars) for the radar's marker hit-rate column: a
    /// marker "hits" when the signed close-to-close return this many bars
    /// later is positive. Matches the swing holding horizon (eval default).
    #[serde(default = "default_marker_horizon_bars")]
    pub marker_horizon_bars: usize,
}

/// Default initial chart window: ~half a year of daily bars — enough recent
/// swing structure with a little context (docs/05 chart display).
fn default_chart_bars() -> usize {
    120
}

/// Default marker hit-rate horizon: 10 daily bars ≈ 2 weeks, the same swing
/// horizon as `EvalConfig::horizon_bars` (docs/07).
fn default_marker_horizon_bars() -> usize {
    10
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            indicators: IndicatorParams::default(),
            regime: RegimeThresholds::default(),
            weights: RegimeWeightTable::default(),
            mtf: MtfConfig::default(),
            proximity: ProximityConfig::default(),
            buy_threshold: 40.0,
            sell_threshold: -40.0,
            squeeze_gate: 0.8,
            min_bars: 60,
            stop_atr_mult: 2.0,
            chart_bars: default_chart_bars(),
            marker_horizon_bars: default_marker_horizon_bars(),
        }
    }
}

/// Threshold/gate preset (docs/00 §5, ADR-10). `Standard` is the P8
/// walk-forward result; the raw `ScanConfig::default()` stays the un-tuned
/// baseline (the tuning candidates vary from it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Conservative,
    Standard,
    Aggressive,
}

impl ScanConfig {
    /// Build a configuration for a preset (starts from the raw default and
    /// applies the preset's threshold/gate deltas).
    pub fn preset(p: Preset) -> Self {
        let mut c = Self::default();
        match p {
            // P8 winner: shorts need more conviction (sell −55). Walk-forward
            // OOS expectancy +0.17% → +0.67%, PF 1.08 → 1.36 vs baseline.
            Preset::Standard => {
                c.sell_threshold = -55.0;
            }
            // Fewer, higher-conviction signals.
            Preset::Conservative => {
                c.buy_threshold = 55.0;
                c.sell_threshold = -70.0;
                c.squeeze_gate = 0.6;
            }
            // More signals (still mildly long-biased).
            Preset::Aggressive => {
                c.buy_threshold = 30.0;
                c.sell_threshold = -45.0;
            }
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_preset_is_long_biased_default_is_raw() {
        // Raw default = un-tuned baseline; Standard = P8-tuned long bias.
        assert_eq!(ScanConfig::default().sell_threshold, -40.0);
        assert_eq!(ScanConfig::preset(Preset::Standard).sell_threshold, -55.0);
        assert_eq!(ScanConfig::preset(Preset::Standard).buy_threshold, 40.0);
        // Presets keep buy/sell sanity (buy > 0 > sell).
        for p in [Preset::Conservative, Preset::Standard, Preset::Aggressive] {
            let c = ScanConfig::preset(p);
            assert!(c.buy_threshold > 0.0 && c.sell_threshold < 0.0);
        }
    }
}
