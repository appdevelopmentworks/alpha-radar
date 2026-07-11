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

/// BUY/SELL markers: at most one per Supertrend leg, placed on the first bar
/// whose composite score confirms the leg's direction at threshold strength
/// (score ≥ buy in an up leg / ≤ sell in a down leg). The trailing-stop leg is
/// the direction axis and the score is the entry trigger — a raw threshold
/// cross re-fires on every score wobble (cluttered, counter-trend markers),
/// while a raw flip has no edge; the leg×score confluence keeps the flip-
/// anchored timing with the score's precision (ADR-14, docs/00 FR-8). The
/// event rule lives in `scoring::marker_events` so the radar's hit-rate column
/// is computed from exactly these markers.
fn confluence_markers(
    ts: &[i64],
    close: &[f64],
    score: &[Option<f64>],
    st_dir: &[Option<i8>],
    buy: f64,
    sell: f64,
) -> Vec<ChartMarker> {
    crate::scoring::marker_events(score, st_dir, buy, sell)
        .into_iter()
        .map(|(i, d)| {
            if d > 0 {
                ChartMarker {
                    time: ts[i],
                    position: "belowBar".into(),
                    color: "#26a69a".into(),
                    shape: "arrowUp".into(),
                    text: format!("BUY @ {:.2}", close[i]),
                }
            } else {
                ChartMarker {
                    time: ts[i],
                    position: "aboveBar".into(),
                    color: "#ef5350".into(),
                    shape: "arrowDown".into(),
                    text: format!("SELL @ {:.2}", close[i]),
                }
            }
        })
        .collect()
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
        markers: confluence_markers(
            &ts,
            &close,
            &score_series,
            &st.dir,
            cfg.buy_threshold,
            cfg.sell_threshold,
        ),
        mtf_summary: mtf_summary(cache, symbol, cfg)?,
        initial_bars: cfg.chart_bars,
        ohlc: candles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Candle;

    /// Crafted-case harness for `confluence_markers`: bar timestamps are the
    /// indices, closes are 100.0, thresholds ±40.
    fn markers(score: &[Option<f64>], dir: &[Option<i8>]) -> Vec<ChartMarker> {
        let n = score.len();
        let ts: Vec<i64> = (0..n as i64).collect();
        let close = vec![100.0; n];
        confluence_markers(&ts, &close, score, dir, 40.0, -40.0)
    }

    #[test]
    fn one_marker_per_leg_at_first_confirmation() {
        // Up leg from bar 0; score reaches the buy threshold at bar 2 and stays
        // above — exactly one BUY at bar 2, no re-fire on later bars.
        let score = [Some(10.0), Some(30.0), Some(45.0), Some(50.0), Some(60.0)];
        let dir = [Some(1); 5];
        let m = markers(&score, &dir);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].time, 2);
        assert_eq!(m[0].shape, "arrowUp");
    }

    #[test]
    fn marker_at_flip_bar_when_score_already_confirms() {
        // Score is already past the buy threshold when the leg flips up at
        // bar 2 → the marker lands on the flip bar itself.
        let score = [Some(50.0); 5];
        let dir = [Some(-1), Some(-1), Some(1), Some(1), Some(1)];
        let m = markers(&score, &dir);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].time, 2);
    }

    #[test]
    fn unconfirmed_leg_yields_no_marker_and_next_leg_rearms() {
        // Up leg (bars 0-2) never reaches +40 → silent. Down leg (bars 3-5)
        // confirms at bar 4 → one SELL.
        let score = [
            Some(10.0),
            Some(20.0),
            Some(30.0),
            Some(-20.0),
            Some(-45.0),
            Some(-60.0),
        ];
        let dir = [Some(1), Some(1), Some(1), Some(-1), Some(-1), Some(-1)];
        let m = markers(&score, &dir);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].time, 4);
        assert_eq!(m[0].shape, "arrowDown");
        assert_eq!(m[0].position, "aboveBar");
    }

    #[test]
    fn score_wobble_across_threshold_does_not_refire_within_leg() {
        // The old rule fired on every re-cross; the leg rule must not.
        let score = [Some(45.0), Some(30.0), Some(45.0), Some(30.0), Some(45.0)];
        let dir = [Some(1); 5];
        let m = markers(&score, &dir);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].time, 0);
    }

    #[test]
    fn warmup_nones_are_skipped() {
        let score = [None, None, Some(50.0)];
        let dir = [None, Some(1), Some(1)];
        let m = markers(&score, &dir);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].time, 2);
    }

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
