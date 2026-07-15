//! One-off diagnostic (not part of CI): compares chart-marker timing rules on
//! the real local cache DB. Run manually:
//!   ALPHA_RADAR_DB=%APPDATA%/com.aileap.alpharadar/alpha-radar.db \
//!     cargo test --test marker_experiment -- --ignored --nocapture

use alpha_radar_lib::config::{Preset, ScanConfig};
use alpha_radar_lib::data::cache::Cache;
use alpha_radar_lib::indicators::momentum::macd;
use alpha_radar_lib::indicators::trend::{qtrend, qtrend_precursors, supertrend};
use alpha_radar_lib::models::Tf;
use alpha_radar_lib::scoring::composite::single_tf_score;

const WARMUP: usize = 210; // EMA200 + margin, fair start for every rule
const H_SHORT: usize = 5;
const H_LONG: usize = 10;

#[derive(Default)]
struct Agg {
    n: usize,
    hits5: usize,
    hits10: usize,
    sum5: f64,
    sum10: f64,
    win10: f64,
    loss10: f64,
    mfe: f64,
    mae: f64,
}

impl Agg {
    fn add(&mut self, r5: f64, r10: f64, mfe: f64, mae: f64) {
        self.n += 1;
        if r5 > 0.0 {
            self.hits5 += 1;
        }
        if r10 > 0.0 {
            self.hits10 += 1;
        }
        self.sum5 += r5;
        self.sum10 += r10;
        if r10 > 0.0 {
            self.win10 += r10;
        } else {
            self.loss10 += -r10;
        }
        self.mfe += mfe;
        self.mae += mae;
    }

    fn report(&self, label: &str, n_symbols: usize) {
        if self.n == 0 {
            println!("{label:<28} n=0");
            return;
        }
        let nf = self.n as f64;
        let pf = if self.loss10 > 0.0 {
            self.win10 / self.loss10
        } else {
            f64::INFINITY
        };
        println!(
            "{label:<28} n={:<5} per_sym={:<5.1} hit5={:<5.1}% hit10={:<5.1}% ret5={:<6.2}% ret10={:<6.2}% PF10={:<5.2} MFE={:<5.2}% MAE={:<5.2}%",
            self.n,
            nf / n_symbols as f64,
            100.0 * self.hits5 as f64 / nf,
            100.0 * self.hits10 as f64 / nf,
            self.sum5 / nf,
            self.sum10 / nf,
            pf,
            self.mfe / nf,
            self.mae / nf,
        );
    }
}

/// (bar index, direction +1/-1) markers per rule.
fn eval_rule(markers: &[(usize, i8)], high: &[f64], low: &[f64], close: &[f64], agg: &mut Agg) {
    let n = close.len();
    for &(i, dir) in markers {
        if i + H_LONG >= n || close[i] <= 0.0 {
            continue;
        }
        let entry = close[i];
        let df = dir as f64;
        let r5 = (close[i + H_SHORT] - entry) / entry * df * 100.0;
        let r10 = (close[i + H_LONG] - entry) / entry * df * 100.0;
        let mut mfe = f64::MIN;
        let mut mae = f64::MAX;
        for k in 1..=H_LONG {
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
        agg.add(r5, r10, mfe, mae);
    }
}

/// Spot-check the wired-up chart path: marker counts per symbol via the real
/// `build_chart_data` (what the app actually renders).
#[test]
#[ignore = "diagnostic on the local cache DB; set ALPHA_RADAR_DB and run with --ignored --nocapture"]
fn chart_marker_counts() {
    let Ok(db) = std::env::var("ALPHA_RADAR_DB") else {
        eprintln!("ALPHA_RADAR_DB not set; skipping");
        return;
    };
    let cfg = ScanConfig::preset(Preset::Aggressive);
    let conn = rusqlite::Connection::open(&db).unwrap();
    let symbols: Vec<String> = conn
        .prepare("SELECT DISTINCT symbol FROM ohlcv WHERE tf='1d'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    drop(conn);
    let cache = Cache::open(&db).unwrap();
    for sym in &symbols {
        let Ok(cd) = alpha_radar_lib::commands::chart::build_chart_data(
            &cache,
            sym,
            Tf::Daily,
            &cfg,
        ) else {
            continue;
        };
        let bars = cd.ohlc.len();
        // Same hit-rate computation the scan column uses (scoring::marker_*).
        let candles = cache.load_candles(sym, Tf::Daily).unwrap();
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let st = supertrend(
            &high,
            &low,
            &close,
            cfg.indicators.supertrend_atr,
            cfg.indicators.supertrend_mult,
        );
        let score = single_tf_score(&candles, &cfg);
        let events = alpha_radar_lib::scoring::marker_events(
            &score,
            &st.dir,
            cfg.buy_threshold,
            cfg.sell_threshold,
        );
        let (rate, n) =
            alpha_radar_lib::scoring::marker_hit_rate(&close, &events, cfg.marker_horizon_bars);
        println!(
            "{sym:<10} bars={bars:<6} markers={:<4} (1 per {:.0} bars)  hit_rate={} (n={n})",
            cd.markers.len(),
            bars as f64 / cd.markers.len().max(1) as f64,
            rate.map_or("—".into(), |r| format!("{:.1}%", r * 100.0)),
        );
    }
}

#[test]
#[ignore = "diagnostic on the local cache DB; set ALPHA_RADAR_DB and run with --ignored --nocapture"]
fn compare_marker_rules() {
    let Ok(db) = std::env::var("ALPHA_RADAR_DB") else {
        eprintln!("ALPHA_RADAR_DB not set; skipping");
        return;
    };
    let cfg = ScanConfig::preset(Preset::Aggressive); // matches the active config (buy 30 / sell -45)

    let conn = rusqlite::Connection::open(&db).unwrap();
    let mut stmt = conn
        .prepare("SELECT DISTINCT symbol FROM ohlcv WHERE tf='1d'")
        .unwrap();
    let symbols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    drop(stmt);
    drop(conn);

    let cache = Cache::open(&db).unwrap();
    let mut aggs: Vec<(&str, Agg)> = vec![
        ("A score-threshold-cross", Agg::default()),
        ("B st-flip raw", Agg::default()),
        ("C st-flip + score-sign", Agg::default()),
        ("D st-flip + macd-hist-sign", Agg::default()),
        ("E st-flip + both", Agg::default()),
        ("F leg-first score>=thresh", Agg::default()),
        ("F0 leg-first score>=0", Agg::default()),
        ("G aligned crossings", Agg::default()),
        // Q-Trend layer (ADR-15). H = the TradingView marker timing; I = one
        // bar earlier (hindsight upper bound for the user's hypothesis); J =
        // the real-time-detectable precursor approximation of I.
        ("H qtrend-flip", Agg::default()),
        ("I qtrend-flip-minus-1", Agg::default()),
        ("J qtrend-precursor", Agg::default()),
    ];
    let mut n_symbols = 0usize;
    let mut total_bars = 0usize;
    // Precursor lead/precision diagnostics (rule J quality).
    let (mut flips_total, mut flips_with_pre) = (0usize, 0usize);
    let (mut pre_total, mut pre_followed) = (0usize, 0usize);
    let mut lead_sum = 0usize;

    for sym in &symbols {
        let candles = cache.load_candles(sym, Tf::Daily).unwrap();
        if candles.len() < WARMUP + H_LONG + 30 {
            continue;
        }
        n_symbols += 1;
        total_bars += candles.len();
        let open: Vec<f64> = candles.iter().map(|c| c.open).collect();
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let p = &cfg.indicators;

        let score = single_tf_score(&candles, &cfg);
        let st = supertrend(&high, &low, &close, p.supertrend_atr, p.supertrend_mult);
        let m = macd(&close, p.macd_fast, p.macd_slow, p.macd_signal);
        let qt = qtrend(
            &open,
            &high,
            &low,
            &close,
            p.qtrend_period,
            p.qtrend_atr,
            p.qtrend_mult,
        );

        let mut rules: Vec<Vec<(usize, i8)>> = vec![Vec::new(); 11];
        // Leg-scoped state: has the current supertrend leg already produced a
        // marker (F: at threshold strength, F0: at any confirming sign)?
        let mut leg_done_f = false;
        let mut leg_done_f0 = false;
        for i in WARMUP..candles.len() {
            // A: score threshold cross (current logic)
            if let (Some(s), Some(pv)) = (score[i], score[i - 1]) {
                if pv < cfg.buy_threshold && s >= cfg.buy_threshold {
                    rules[0].push((i, 1));
                } else if pv > cfg.sell_threshold && s <= cfg.sell_threshold {
                    rules[0].push((i, -1));
                }
            }
            let (Some(d), Some(dp)) = (st.dir[i], st.dir[i - 1]) else {
                continue;
            };
            let flipped = d != dp;
            if flipped {
                leg_done_f = false;
                leg_done_f0 = false;
                // B..E: markers at the flip bar itself
                rules[1].push((i, d));
                let score_ok = score[i].is_some_and(|s| s * d as f64 >= 0.0);
                let hist_ok = m.hist[i].is_some_and(|h| h * d as f64 >= 0.0);
                if score_ok {
                    rules[2].push((i, d));
                }
                if hist_ok {
                    rules[3].push((i, d));
                }
                if score_ok && hist_ok {
                    rules[4].push((i, d));
                }
            }
            // F/F0: first confirming bar within each leg
            if let Some(s) = score[i] {
                let thresh_ok = if d > 0 {
                    s >= cfg.buy_threshold
                } else {
                    s <= cfg.sell_threshold
                };
                if !leg_done_f && thresh_ok {
                    rules[5].push((i, d));
                    leg_done_f = true;
                }
                if !leg_done_f0 && s * d as f64 > 0.0 {
                    rules[6].push((i, d));
                    leg_done_f0 = true;
                }
                // G: every threshold cross that agrees with the leg direction
                if let Some(pv) = score[i - 1] {
                    let cross = if d > 0 {
                        pv < cfg.buy_threshold && s >= cfg.buy_threshold
                    } else {
                        pv > cfg.sell_threshold && s <= cfg.sell_threshold
                    };
                    if cross {
                        rules[7].push((i, d));
                    }
                }
            }
        }
        // H/I/J: Q-Trend flips, flips shifted 1 bar earlier, and precursors.
        let flips: Vec<(usize, i8)> = qt
            .flip
            .iter()
            .enumerate()
            .filter_map(|(i, f)| f.map(|d| (i, d)))
            .collect();
        for &(i, d) in &flips {
            if i >= WARMUP {
                rules[8].push((i, d));
            }
            if i > WARMUP {
                rules[9].push((i - 1, d));
            }
        }
        let pres = qtrend_precursors(&close, &qt, p.qtrend_precursor_atr);
        for &(i, d) in &pres {
            if i >= WARMUP {
                rules[10].push((i, d));
            }
        }
        // Diagnostics: coverage (flip had a precursor within the prior 3 bars),
        // precision (precursor followed by a flip within 3 bars), mean lead.
        for &(fi, _) in flips.iter().filter(|(i, _)| *i >= WARMUP) {
            flips_total += 1;
            if pres.iter().any(|&(pi, _)| pi < fi && fi - pi <= 3) {
                flips_with_pre += 1;
            }
        }
        for &(pi, _) in pres.iter().filter(|(i, _)| *i >= WARMUP) {
            pre_total += 1;
            if let Some(&(fi, _)) = flips.iter().find(|&&(fi, _)| fi > pi && fi - pi <= 3) {
                pre_followed += 1;
                lead_sum += fi - pi;
            }
        }

        for (r, (_, agg)) in rules.iter().zip(aggs.iter_mut()) {
            eval_rule(r, &high, &low, &close, agg);
        }
    }

    println!(
        "\nsymbols evaluated: {n_symbols} (of {} in cache), avg bars/symbol: {:.0}\n",
        symbols.len(),
        total_bars as f64 / n_symbols.max(1) as f64
    );
    for (label, agg) in &aggs {
        agg.report(label, n_symbols.max(1));
    }

    // Interpretation (docs plan): the "1 bar earlier" hypothesis is supported
    // iff I beats H on ret10/PF AND J tracks I; if I > H but J <= H, the
    // precursor definition needs tuning before any ranking integration.
    println!(
        "\nprecursor diagnostics: coverage={}/{} flips had a precursor within 3 bars ({:.0}%), \
         precision={}/{} precursors flipped within 3 bars ({:.0}%), mean lead={:.2} bars",
        flips_with_pre,
        flips_total,
        100.0 * flips_with_pre as f64 / flips_total.max(1) as f64,
        pre_followed,
        pre_total,
        100.0 * pre_followed as f64 / pre_total.max(1) as f64,
        lead_sum as f64 / pre_followed.max(1) as f64,
    );
}
