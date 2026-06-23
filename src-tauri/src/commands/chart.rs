//! Chart-data assembly (P6). Every series is computed in Rust from the cached
//! candles, so the chart and the ranking score are guaranteed to agree
//! (docs/05, ADR-06). Shaped for direct consumption by lightweight-charts v5.

use crate::config::ScanConfig;
use crate::data::cache::Cache;
use crate::error::{AppError, AppResult};
use crate::indicators::momentum::{macd, squeeze_momentum};
use crate::indicators::trend::{adx_dmi, ema_ribbon, ichimoku, supertrend};
use crate::indicators::volatility::choppiness;
use crate::models::{ChartData, ChartMarker, HistBar, Tf, TfSummary, TimeValue};
use crate::regime::regime_series;
use crate::scoring::composite::single_tf_score;

/// Pair a value series with its timestamps, dropping warm-up (`None`) points.
fn tv(ts: &[i64], series: &[Option<f64>]) -> Vec<TimeValue> {
    ts.iter()
        .zip(series.iter())
        .filter_map(|(&time, v)| v.map(|value| TimeValue { time, value }))
        .collect()
}

struct FourColor {
    up_strong: &'static str,
    up_weak: &'static str,
    dn_strong: &'static str,
    dn_weak: &'static str,
}

// MACD histogram (teal/red, strong = momentum building in its direction).
const MACD_COLORS: FourColor = FourColor {
    up_strong: "#26a69a",
    up_weak: "#80cbc4",
    dn_strong: "#ef5350",
    dn_weak: "#ef9a9a",
};

// Squeeze Momentum (LazyBear palette).
const SQZ_COLORS: FourColor = FourColor {
    up_strong: "#00e676",
    up_weak: "#00897b",
    dn_strong: "#ff5252",
    dn_weak: "#b71c1c",
};

/// 4-color histogram: bright when momentum strengthens in its sign, dim when
/// it weakens (standard MACD / Squeeze coloring).
fn four_color_hist(ts: &[i64], series: &[Option<f64>], c: &FourColor) -> Vec<HistBar> {
    let mut out = Vec::new();
    let mut prev: Option<f64> = None;
    for (i, v) in series.iter().enumerate() {
        let Some(val) = *v else {
            prev = None;
            continue;
        };
        let color = if val >= 0.0 {
            if prev.map_or(true, |p| val >= p) {
                c.up_strong
            } else {
                c.up_weak
            }
        } else if prev.map_or(true, |p| val <= p) {
            c.dn_strong
        } else {
            c.dn_weak
        };
        out.push(HistBar {
            time: ts[i],
            value: val,
            color: color.to_string(),
        });
        prev = Some(val);
    }
    out
}

/// BUY/SELL markers where the (single-TF) score crosses the thresholds.
fn markers_from_score(
    ts: &[i64],
    close: &[f64],
    score: &[Option<f64>],
    buy: f64,
    sell: f64,
) -> Vec<ChartMarker> {
    let mut out = Vec::new();
    for i in 1..score.len() {
        let (Some(s), Some(p)) = (score[i], score[i - 1]) else {
            continue;
        };
        if p < buy && s >= buy {
            out.push(ChartMarker {
                time: ts[i],
                position: "belowBar".into(),
                color: "#26a69a".into(),
                shape: "arrowUp".into(),
                text: format!("BUY @ {:.2}", close[i]),
            });
        } else if p > sell && s <= sell {
            out.push(ChartMarker {
                time: ts[i],
                position: "aboveBar".into(),
                color: "#ef5350".into(),
                shape: "arrowDown".into(),
                text: format!("SELL @ {:.2}", close[i]),
            });
        }
    }
    out
}

/// Conviction velocity from the tail of the score series.
fn velocity_label(score: &[Option<f64>]) -> String {
    let recent: Vec<f64> = score.iter().rev().flatten().copied().take(4).collect();
    if recent.len() < 2 {
        return "Flat".into();
    }
    let delta = recent[0].abs() - recent[recent.len() - 1].abs();
    if delta > 1.0 {
        "Accelerating".into()
    } else if delta < -1.0 {
        "Decelerating".into()
    } else {
        "Flat".into()
    }
}

fn mtf_summary(cache: &Cache, symbol: &str, cfg: &ScanConfig) -> AppResult<Vec<TfSummary>> {
    let mut out = Vec::new();
    for tf in Tf::ALL {
        let candles = cache.load_candles(symbol, tf)?;
        let (regime, velocity) = if candles.is_empty() {
            (None, "Flat".to_string())
        } else {
            let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
            let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
            let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
            let dmi = adx_dmi(&high, &low, &close, cfg.indicators.adx_period);
            let chop = choppiness(&high, &low, &close, cfg.indicators.choppiness_period);
            let regime = regime_series(&dmi, &chop, &cfg.regime)
                .last()
                .copied()
                .flatten();
            let velocity = velocity_label(&single_tf_score(&candles, cfg));
            (regime, velocity)
        };
        out.push(TfSummary {
            tf: tf.interval().into(),
            regime,
            velocity,
        });
    }
    Ok(out)
}

/// Build the full multi-pane chart payload for `symbol` × `tf`.
pub fn build_chart_data(
    cache: &Cache,
    symbol: &str,
    tf: Tf,
    cfg: &ScanConfig,
) -> AppResult<ChartData> {
    let candles = cache.load_candles(symbol, tf)?;
    if candles.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "no cached data for {symbol} {}",
            tf.interval()
        )));
    }
    let ts: Vec<i64> = candles.iter().map(|c| c.ts).collect();
    let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
    let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
    let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
    let p = &cfg.indicators;

    let ribbon = ema_ribbon(&close, p.ema_ribbon);
    let st = supertrend(&high, &low, &close, p.supertrend_atr, p.supertrend_mult);
    let ich = ichimoku(
        &high,
        &low,
        p.ichimoku[0],
        p.ichimoku[1],
        p.ichimoku[2],
        p.ichimoku_displacement,
    );
    let m = macd(&close, p.macd_fast, p.macd_slow, p.macd_signal);
    let sq = squeeze_momentum(
        &high,
        &low,
        &close,
        p.squeeze_length,
        p.squeeze_mult_bb,
        p.squeeze_mult_kc,
    );
    let score_series = single_tf_score(&candles, cfg);

    Ok(ChartData {
        ema20: tv(&ts, &ribbon.fast),
        ema50: tv(&ts, &ribbon.mid),
        ema200: tv(&ts, &ribbon.slow),
        supertrend: tv(&ts, &st.line),
        tenkan: tv(&ts, &ich.tenkan),
        kijun: tv(&ts, &ich.kijun),
        senkou_a: tv(&ts, &ich.senkou_a),
        senkou_b: tv(&ts, &ich.senkou_b),
        macd: tv(&ts, &m.macd),
        macd_signal: tv(&ts, &m.signal),
        macd_hist: four_color_hist(&ts, &m.hist, &MACD_COLORS),
        sqz_val: four_color_hist(&ts, &sq.val, &SQZ_COLORS),
        score: tv(&ts, &score_series),
        buy_threshold: cfg.buy_threshold,
        sell_threshold: cfg.sell_threshold,
        markers: markers_from_score(
            &ts,
            &close,
            &score_series,
            cfg.buy_threshold,
            cfg.sell_threshold,
        ),
        mtf_summary: mtf_summary(cache, symbol, cfg)?,
        ohlc: candles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Candle;

    #[test]
    fn chart_data_matches_single_tf_score() {
        let cfg = ScanConfig::default();
        let mut cache = Cache::open_in_memory().unwrap();
        let candles: Vec<Candle> = (0..260)
            .map(|i| {
                let c = 100.0 + i as f64;
                Candle::ohlcv(i as i64 * 86_400, c, c + 1.0, c - 1.0, c, 1_000_000.0)
            })
            .collect();
        cache.upsert_candles("UP", Tf::Daily, &candles).unwrap();

        let cd = build_chart_data(&cache, "UP", Tf::Daily, &cfg).unwrap();
        // The chart's score series is exactly single_tf_score (no recomputation drift).
        let direct = single_tf_score(&candles, &cfg);
        let direct_last = direct.iter().rev().flatten().next().copied().unwrap();
        assert_eq!(cd.score.last().unwrap().value, direct_last);
        assert!(!cd.ohlc.is_empty());
        assert_eq!(cd.buy_threshold, cfg.buy_threshold);
    }
}
