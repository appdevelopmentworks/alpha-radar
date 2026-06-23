//! Entry-imminence (proximity) engine — the second orthogonal axis (docs/03 §6,
//! ADR-08). "How close is this name to a swing entry", `0..100`, read off the
//! **daily** timeframe and independent of the direction score's magnitude.
//!
//! Pipeline: four proximity components (each `[0,1]`) → max aggregation →
//! state machine (Primed / Triggered / Active) with freshness decay →
//! `actionability` for ranking. Constants are config-driven (P8 tunes them).

use serde::{Deserialize, Serialize};

use crate::config::{ProximityConfig, ScanConfig};
use crate::indicators::momentum::squeeze_momentum;
use crate::indicators::{atr, ema, rsi, sma};
use crate::models::Candle;
use crate::scoring::composite::single_tf_score;

/// Candidate trade direction for a bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Buy,
    Sell,
    None,
}

impl Direction {
    /// +1 buy, −1 sell, 0 none.
    pub fn sign(self) -> i8 {
        match self {
            Direction::Buy => 1,
            Direction::Sell => -1,
            Direction::None => 0,
        }
    }
}

/// Imminence state machine (docs/03 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalState {
    PrimedBuy,
    TriggeredBuy,
    ActiveBuy,
    Neutral,
    PrimedSell,
    TriggeredSell,
    ActiveSell,
}

impl SignalState {
    fn primed(d: Direction) -> Self {
        match d {
            Direction::Buy => SignalState::PrimedBuy,
            Direction::Sell => SignalState::PrimedSell,
            Direction::None => SignalState::Neutral,
        }
    }
    fn triggered(d: Direction) -> Self {
        match d {
            Direction::Buy => SignalState::TriggeredBuy,
            Direction::Sell => SignalState::TriggeredSell,
            Direction::None => SignalState::Neutral,
        }
    }
    fn active(d: Direction) -> Self {
        match d {
            Direction::Buy => SignalState::ActiveBuy,
            Direction::Sell => SignalState::ActiveSell,
            Direction::None => SignalState::Neutral,
        }
    }

    /// The trade direction implied by this state.
    pub fn direction(self) -> Direction {
        match self {
            SignalState::PrimedBuy | SignalState::TriggeredBuy | SignalState::ActiveBuy => {
                Direction::Buy
            }
            SignalState::PrimedSell | SignalState::TriggeredSell | SignalState::ActiveSell => {
                Direction::Sell
            }
            SignalState::Neutral => Direction::None,
        }
    }
}

/// The four proximity components for a bar, each in `[0,1]` (0 when N/A).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProximityComponents {
    pub p_thresh: f64,
    pub p_sqz: f64,
    pub p_cr: f64,
    pub p_pull: f64,
}

impl ProximityComponents {
    /// Aggregate by max (any single near-trigger lifts proximity). Non-finite
    /// values are treated as 0 (docs/03 §7 NaN guard).
    pub fn aggregate_max(&self) -> f64 {
        [self.p_thresh, self.p_sqz, self.p_cr, self.p_pull]
            .into_iter()
            .filter(|x| x.is_finite())
            .fold(0.0, f64::max)
    }
}

/// Per-bar proximity result.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProximityPoint {
    pub state: SignalState,
    pub proximity_score: f64,
    pub bars_since_trigger: Option<u32>,
    pub components: ProximityComponents,
}

/// Candidate direction from the (daily) score sign.
fn score_direction(score: Option<f64>) -> Direction {
    match score {
        Some(s) if s > 0.0 => Direction::Buy,
        Some(s) if s < 0.0 => Direction::Sell,
        _ => Direction::None,
    }
}

/// `p_thresh`: approach to the buy/sell threshold from the trigger side, scaled
/// by an approach-velocity bonus (docs/03 §6).
pub(crate) fn p_thresh_at(
    score: &[Option<f64>],
    i: usize,
    dir: Direction,
    cfg: &ProximityConfig,
    buy: f64,
    sell: f64,
) -> f64 {
    let Some(s) = score[i] else {
        return 0.0;
    };
    let base = match dir {
        Direction::Buy if buy > cfg.approach_floor => {
            ((s - cfg.approach_floor) / (buy - cfg.approach_floor)).clamp(0.0, 1.0)
        }
        Direction::Sell if -sell > cfg.approach_floor => {
            ((-s - cfg.approach_floor) / (-sell - cfg.approach_floor)).clamp(0.0, 1.0)
        }
        _ => 0.0,
    };
    if base == 0.0 {
        return 0.0;
    }

    let velocity = if i >= cfg.velocity_bars {
        match (score[i], score[i - cfg.velocity_bars]) {
            (Some(a), Some(b)) => a - b,
            _ => 0.0,
        }
    } else {
        0.0
    };
    let directional = match dir {
        Direction::Buy => velocity,
        Direction::Sell => -velocity,
        Direction::None => 0.0,
    };
    let bonus =
        1.0 + (directional / cfg.velocity_scale).clamp(0.0, 1.0) * (cfg.velocity_max_bonus - 1.0);
    (base * bonus).clamp(0.0, 1.0)
}

/// `p_sqz`: squeeze build-up (ramps with bars in squeeze) and release spike
/// (docs/03 §6).
pub(crate) fn p_sqz_at(
    sqz_on: &[Option<bool>],
    bars_in_sqz: &[u32],
    i: usize,
    typical_len: f64,
) -> f64 {
    match sqz_on[i] {
        Some(true) => (bars_in_sqz[i] as f64 / typical_len).clamp(0.0, 1.0),
        // Just released: previous bar squeezed, this bar not.
        _ if i > 0 && sqz_on[i - 1] == Some(true) && sqz_on[i] == Some(false) => 1.0,
        _ => 0.0,
    }
}

/// `p_cr`: Connors RSI(2) trigger proximity, gated by the 200-MA filter and the
/// candidate direction (docs/03 §6).
pub(crate) fn p_cr_at(
    close: &[f64],
    rsi2: &[Option<f64>],
    sma_filter: &[Option<f64>],
    i: usize,
    dir: Direction,
    cfg: &ProximityConfig,
) -> f64 {
    let Some(r) = rsi2[i] else {
        return 0.0;
    };
    match (dir, sma_filter[i]) {
        (Direction::Buy, Some(ma)) if close[i] > ma => {
            ((cfg.cr_buy_zone - r) / cfg.cr_buy_zone).clamp(0.0, 1.0)
        }
        (Direction::Sell, Some(ma)) if close[i] < ma => {
            ((r - cfg.cr_sell_zone) / (100.0 - cfg.cr_sell_zone)).clamp(0.0, 1.0)
        }
        _ => 0.0,
    }
}

/// `p_pull`: closeness to the key MA (EMA fast) in ATR units (docs/03 §6).
pub(crate) fn p_pull_at(
    close: &[f64],
    ema_fast: &[Option<f64>],
    atrv: &[Option<f64>],
    i: usize,
    max_dist_atr: f64,
) -> f64 {
    let (Some(e), Some(a)) = (ema_fast[i], atrv[i]) else {
        return 0.0;
    };
    if a <= 0.0 || max_dist_atr <= 0.0 {
        return 0.0;
    }
    (1.0 - (close[i] - e).abs() / a / max_dist_atr).clamp(0.0, 1.0)
}

/// State machine + freshness decay over the daily score series and per-bar
/// components (docs/03 §6). Pure and deterministic.
pub fn classify(
    score: &[Option<f64>],
    components: &[ProximityComponents],
    cfg: &ScanConfig,
) -> Vec<ProximityPoint> {
    let n = score.len();
    let pc = &cfg.proximity;
    let (buy, sell) = (cfg.buy_threshold, cfg.sell_threshold);

    // Most recent threshold cross at or before each bar.
    let mut last: Option<(usize, Direction)> = None;
    let mut crosses = vec![None; n];
    for i in 0..n {
        if i > 0 {
            if let (Some(s), Some(ps)) = (score[i], score[i - 1]) {
                if ps < buy && s >= buy {
                    last = Some((i, Direction::Buy));
                } else if ps > sell && s <= sell {
                    last = Some((i, Direction::Sell));
                }
            }
        }
        crosses[i] = last;
    }

    (0..n)
        .map(|i| {
            let comps = components[i];
            let base = (comps.aggregate_max() * 100.0).round();
            let dir = score_direction(score[i]);
            let bars_since = crosses[i].map(|(ci, _)| (i - ci) as u32);

            // docs/03 §6: a fresh cross ⇒ Triggered; no cross but a forming
            // setup ⇒ Primed; a stale cross ⇒ Active (decayed); else Neutral.
            let (state, proximity_score) = match crosses[i] {
                Some((ci, cdir)) if (i - ci) as u32 <= pc.fresh_bars_n => {
                    (SignalState::triggered(cdir), base.max(pc.triggered_floor))
                }
                None if dir != Direction::None && comps.aggregate_max() >= pc.primed_floor => {
                    (SignalState::primed(dir), base)
                }
                Some((ci, cdir)) => {
                    let b = (i - ci) as u32;
                    let decayed = (base * pc.active_decay.powi(b as i32)).round();
                    (SignalState::active(cdir), decayed)
                }
                None => (SignalState::Neutral, base),
            };

            ProximityPoint {
                state,
                proximity_score,
                bars_since_trigger: bars_since,
                components: comps,
            }
        })
        .collect()
}

/// Full proximity series for a symbol's daily candles (docs/03 §6).
pub fn proximity_series(daily: &[Candle], cfg: &ScanConfig) -> Vec<ProximityPoint> {
    let n = daily.len();
    let high: Vec<f64> = daily.iter().map(|c| c.high).collect();
    let low: Vec<f64> = daily.iter().map(|c| c.low).collect();
    let close: Vec<f64> = daily.iter().map(|c| c.close).collect();
    let ip = &cfg.indicators;
    let pc = &cfg.proximity;

    let score = single_tf_score(daily, cfg);
    let sq = squeeze_momentum(
        &high,
        &low,
        &close,
        ip.squeeze_length,
        ip.squeeze_mult_bb,
        ip.squeeze_mult_kc,
    );
    let rsi2 = rsi(&close, ip.connors_rsi);
    let sma_filter = sma(&close, ip.connors_ma);
    let ema_fast = ema(&close, ip.ema_ribbon[0]);
    let atrv = atr(&high, &low, &close, ip.atr_period);

    let mut bars_in_sqz = vec![0u32; n];
    for i in 0..n {
        if sq.sqz_on[i] == Some(true) {
            bars_in_sqz[i] = if i > 0 { bars_in_sqz[i - 1] + 1 } else { 1 };
        }
    }

    let components: Vec<ProximityComponents> = (0..n)
        .map(|i| {
            let dir = score_direction(score[i]);
            ProximityComponents {
                p_thresh: p_thresh_at(&score, i, dir, pc, cfg.buy_threshold, cfg.sell_threshold),
                p_sqz: p_sqz_at(&sq.sqz_on, &bars_in_sqz, i, pc.typical_squeeze_len),
                p_cr: p_cr_at(&close, &rsi2, &sma_filter, i, dir, pc),
                p_pull: p_pull_at(&close, &ema_fast, &atrv, i, pc.pull_max_dist_atr),
            }
        })
        .collect();

    classify(&score, &components, cfg)
}

/// Latest-bar proximity for a symbol (`None` if it has no bars).
pub fn latest_proximity(daily: &[Candle], cfg: &ScanConfig) -> Option<ProximityPoint> {
    proximity_series(daily, cfg).into_iter().last()
}

/// Ranking score: timing (proximity) × conviction (|direction score|)
/// (docs/03 §6). `score_final` is the MTF `Score_final`.
pub fn actionability(proximity_score: f64, score_final: f64) -> f64 {
    proximity_score * (0.5 + 0.5 * score_final.abs() / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ScanConfig;

    fn comps(p_thresh: f64) -> ProximityComponents {
        ProximityComponents {
            p_thresh,
            p_sqz: 0.0,
            p_cr: 0.0,
            p_pull: 0.0,
        }
    }

    #[test]
    fn threshold_cross_triggers_then_goes_active() {
        let cfg = ScanConfig::default(); // buy 40, fresh 2, decay 0.95, trig floor 90
                                         // Score rises across the buy threshold at i=3, then drifts up.
        let score: Vec<Option<f64>> = vec![
            Some(20.0),
            Some(30.0),
            Some(38.0),
            Some(45.0), // cross at i=3
            Some(46.0),
            Some(47.0),
            Some(48.0),
            Some(49.0),
        ];
        let components: Vec<ProximityComponents> = score.iter().map(|_| comps(0.8)).collect();
        let pts = classify(&score, &components, &cfg);

        assert_eq!(pts[3].state, SignalState::TriggeredBuy);
        assert!(pts[3].proximity_score >= 90.0);
        assert_eq!(pts[4].state, SignalState::TriggeredBuy); // within fresh_bars_n=2
        assert_eq!(pts[5].state, SignalState::TriggeredBuy);
        // i=6 → 3 bars since trigger > 2 → Active, decayed below the base 80.
        assert_eq!(pts[6].state, SignalState::ActiveBuy);
        assert!(pts[6].proximity_score < 80.0 && pts[6].proximity_score > 0.0);
        assert_eq!(pts[6].bars_since_trigger, Some(3));
    }

    #[test]
    fn forming_setup_without_cross_is_primed() {
        let cfg = ScanConfig::default();
        // Below threshold (no cross) but a strong component and a buy lean.
        let score: Vec<Option<f64>> = vec![Some(10.0), Some(15.0), Some(20.0)];
        let components: Vec<ProximityComponents> = score.iter().map(|_| comps(0.7)).collect();
        let pts = classify(&score, &components, &cfg);
        assert_eq!(pts[2].state, SignalState::PrimedBuy);
        assert_eq!(pts[2].proximity_score, 70.0);
    }

    #[test]
    fn quiet_series_is_neutral() {
        let cfg = ScanConfig::default();
        let score: Vec<Option<f64>> = vec![Some(5.0), Some(6.0), Some(5.0)];
        let components: Vec<ProximityComponents> = score.iter().map(|_| comps(0.1)).collect();
        let pts = classify(&score, &components, &cfg);
        assert_eq!(pts[2].state, SignalState::Neutral);
    }

    #[test]
    fn squeeze_ramps_then_spikes_on_release() {
        let len = 20.0;
        // 5 bars squeezed, then released on the 6th.
        let sqz_on = vec![
            Some(true),
            Some(true),
            Some(true),
            Some(true),
            Some(true),
            Some(false),
        ];
        let bars_in_sqz = vec![1, 2, 3, 4, 5, 0];
        assert!((p_sqz_at(&sqz_on, &bars_in_sqz, 0, len) - 0.05).abs() < 1e-12);
        assert!((p_sqz_at(&sqz_on, &bars_in_sqz, 4, len) - 0.25).abs() < 1e-12);
        assert_eq!(p_sqz_at(&sqz_on, &bars_in_sqz, 5, len), 1.0); // release spike
    }

    #[test]
    fn pullback_near_ma_scores_high() {
        let close = [100.0];
        let ema_fast = [Some(99.5)];
        let atrv = [Some(2.0)];
        // dist = 0.5/2 = 0.25 ATR; with max 2.0 ATR → 1 - 0.25/2 = 0.875.
        let p = p_pull_at(&close, &ema_fast, &atrv, 0, 2.0);
        assert!((p - 0.875).abs() < 1e-12);
    }

    #[test]
    fn actionability_blends_timing_and_conviction() {
        // Full conviction doubles the timing weight vs zero conviction.
        assert!((actionability(80.0, 100.0) - 80.0).abs() < 1e-12);
        assert!((actionability(80.0, 0.0) - 40.0).abs() < 1e-12);
    }

    #[test]
    fn proximity_series_end_to_end_on_uptrend() {
        let cfg = ScanConfig::default();
        // A strong, long up-trend crosses the buy threshold and stays bid.
        let candles: Vec<Candle> = (0..260)
            .map(|i| {
                let c = 100.0 + i as f64;
                Candle::ohlcv(i as i64 * 86_400, c, c + 1.0, c - 1.0, c, 1_000_000.0)
            })
            .collect();
        let pt = latest_proximity(&candles, &cfg).expect("has bars");
        assert!((0.0..=100.0).contains(&pt.proximity_score));
        // The score crossed the buy threshold earlier, so a trigger is on record.
        assert!(pt.bars_since_trigger.is_some());
        assert!(matches!(
            pt.state,
            SignalState::TriggeredBuy | SignalState::ActiveBuy | SignalState::PrimedBuy
        ));
    }
}
