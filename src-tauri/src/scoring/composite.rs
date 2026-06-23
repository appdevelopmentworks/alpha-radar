//! Single-timeframe composite scoring (docs/03 §2, §4).
//!
//! Assembles indicator sub-scores into category means, applies regime weights
//! (`scoring::weights`) and the quality gate, and normalizes to `[-100, +100]`.

use crate::config::ScanConfig;
use crate::indicators::mean_reversion::{
    connors_rsi2, connors_rsi2_subscore, ma_zscore, ma_zscore_subscore, percent_b,
    percent_b_subscore, williams_r, williams_r_subscore,
};
use crate::indicators::momentum::{
    macd, macd_subscore, rsi_subscore, squeeze_momentum, squeeze_subscore, tsi, tsi_subscore,
};
use crate::indicators::rsi;
use crate::indicators::trend::{
    adx_dmi, adx_subscore, ema_ribbon, ema_ribbon_subscore, ichimoku, ichimoku_subscore,
    supertrend, supertrend_subscore,
};
use crate::indicators::volatility::{bollinger, choppiness};
use crate::models::Candle;
use crate::regime::regime_series;
use crate::scoring::weights::weighted_composite;

/// Category sub-scores for a single bar (`None` = no indicator in the category
/// is available yet).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CategoryScores {
    pub trend: Option<f64>,
    pub momentum: Option<f64>,
    pub mean_reversion: Option<f64>,
}

fn ohlc(candles: &[Candle]) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    (
        candles.iter().map(|c| c.high).collect(),
        candles.iter().map(|c| c.low).collect(),
        candles.iter().map(|c| c.close).collect(),
    )
}

/// Mean of the present (`Some`) sub-scores at bar `i`, in fixed slice order
/// (deterministic).
fn category_mean(subs: &[&[Option<f64>]], i: usize) -> Option<f64> {
    let mut sum = 0.0;
    let mut count = 0u32;
    for s in subs {
        if let Some(v) = s[i] {
            sum += v;
            count += 1;
        }
    }
    (count > 0).then(|| sum / count as f64)
}

/// Per-bar category sub-scores (docs/03 §2). RSI joins the mean-reversion
/// category so the regime sign-flip yields its trend-vs-range interpretation.
pub fn category_scores(candles: &[Candle], cfg: &ScanConfig) -> Vec<CategoryScores> {
    let (high, low, close) = ohlc(candles);
    let p = &cfg.indicators;

    // trend
    let adx_s = adx_subscore(&adx_dmi(&high, &low, &close, p.adx_period));
    let ribbon_s = ema_ribbon_subscore(&ema_ribbon(&close, p.ema_ribbon));
    let st = supertrend(&high, &low, &close, p.supertrend_atr, p.supertrend_mult);
    let st_s = supertrend_subscore(
        &high,
        &low,
        &close,
        &st,
        p.supertrend_atr,
        p.supertrend_mult,
    );
    let ich = ichimoku(
        &high,
        &low,
        p.ichimoku[0],
        p.ichimoku[1],
        p.ichimoku[2],
        p.ichimoku_displacement,
    );
    let ich_s = ichimoku_subscore(&close, &ich, p.ichimoku_displacement);
    let trend_subs: [&[Option<f64>]; 4] = [&adx_s, &ribbon_s, &st_s, &ich_s];

    // momentum
    let m = macd(&close, p.macd_fast, p.macd_slow, p.macd_signal);
    let macd_s = macd_subscore(&high, &low, &close, &m, p.atr_period, p.macd_k);
    let sq = squeeze_momentum(
        &high,
        &low,
        &close,
        p.squeeze_length,
        p.squeeze_mult_bb,
        p.squeeze_mult_kc,
    );
    let sq_s = squeeze_subscore(&high, &low, &close, &sq, p.atr_period, p.squeeze_k);
    let tsi_s = tsi_subscore(&tsi(&close, p.tsi_long, p.tsi_short));
    let mom_subs: [&[Option<f64>]; 3] = [&macd_s, &sq_s, &tsi_s];

    // mean reversion (RSI in range orientation; sign-flip handles regime)
    let cr = connors_rsi2(&close, p.connors_rsi);
    let cr_s = connors_rsi2_subscore(&close, &cr, p.connors_ma);
    let pb_s = percent_b_subscore(&percent_b(
        &close,
        &bollinger(&close, p.bb_period, p.bb_mult),
    ));
    let wr_s = williams_r_subscore(&williams_r(&high, &low, &close, p.williams_period));
    let z_s = ma_zscore_subscore(&ma_zscore(&close, p.zscore_sma, p.zscore_std));
    let rsi_s = rsi_subscore(&rsi(&close, p.rsi_period));
    let mr_subs: [&[Option<f64>]; 5] = [&cr_s, &pb_s, &wr_s, &z_s, &rsi_s];

    (0..candles.len())
        .map(|i| CategoryScores {
            trend: category_mean(&trend_subs, i),
            momentum: category_mean(&mom_subs, i),
            mean_reversion: category_mean(&mr_subs, i),
        })
        .collect()
}

/// Quality gate `0..1` applied to the direction score: a squeeze (low
/// volatility, unclear direction) damps it by `cfg.squeeze_gate`. `1.0` when
/// not squeezed or unknown (docs/03 §4).
pub fn quality_gate(candles: &[Candle], cfg: &ScanConfig) -> Vec<f64> {
    let (high, low, close) = ohlc(candles);
    let p = &cfg.indicators;
    let sq = squeeze_momentum(
        &high,
        &low,
        &close,
        p.squeeze_length,
        p.squeeze_mult_bb,
        p.squeeze_mult_kc,
    );
    sq.sqz_on
        .iter()
        .map(|o| match o {
            Some(true) => cfg.squeeze_gate,
            _ => 1.0,
        })
        .collect()
}

/// Single-timeframe direction score series, `clamp(round(S_raw·gate·100), ±100)`
/// (docs/03 §4). `None` until a regime and at least one category exist.
pub fn single_tf_score(candles: &[Candle], cfg: &ScanConfig) -> Vec<Option<f64>> {
    let (high, low, close) = ohlc(candles);
    let dmi = adx_dmi(&high, &low, &close, cfg.indicators.adx_period);
    let chop = choppiness(&high, &low, &close, cfg.indicators.choppiness_period);
    let regimes = regime_series(&dmi, &chop, &cfg.regime);
    let cats = category_scores(candles, cfg);
    let gate = quality_gate(candles, cfg);

    (0..candles.len())
        .map(|i| {
            let regime = regimes[i]?;
            let raw = weighted_composite(&cats[i], regime, &cfg.weights)?;
            Some((raw * gate[i] * 100.0).round().clamp(-100.0, 100.0))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ScanConfig;

    fn candle(ts: i64, o: f64, h: f64, l: f64, c: f64) -> Candle {
        Candle::ohlcv(ts, o, h, l, c, 1_000_000.0)
    }

    /// A clean monotonic up-trend should produce a positive late-bar score.
    #[test]
    fn strong_uptrend_scores_positive() {
        let cfg = ScanConfig::default();
        let candles: Vec<Candle> = (0..260)
            .map(|i| {
                let c = 100.0 + i as f64;
                candle(i as i64 * 86_400, c - 0.5, c + 0.5, c - 1.0, c)
            })
            .collect();
        let score = single_tf_score(&candles, &cfg);
        let last = score.last().copied().flatten().expect("score present");
        assert!(last > 0.0, "expected positive score, got {last}");
    }
}
