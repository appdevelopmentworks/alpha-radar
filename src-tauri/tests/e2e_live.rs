//! Live end-to-end pipeline test (network): CSV → sidecar (yfinance) → cache →
//! score → chart-data. Requires `uv` + the synced `sidecar` env. Run manually:
//!   cargo test --manifest-path src-tauri/Cargo.toml --test e2e_live -- --ignored --nocapture

use std::time::{SystemTime, UNIX_EPOCH};

use alpha_radar_lib::commands::chart::build_chart_data;
use alpha_radar_lib::commands::scan_universe_impl;
use alpha_radar_lib::config::ScanConfig;
use alpha_radar_lib::data::cache::Cache;
use alpha_radar_lib::data::sidecar::{repo_root, SidecarClient};
use alpha_radar_lib::eval::{evaluate, EvalConfig};
use alpha_radar_lib::models::{AssetClass, Tf};

#[test]
#[ignore = "live network via yfinance/uv; run manually"]
fn live_scan_then_chart() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mut cache = Cache::open_in_memory().unwrap();
    let sidecar = SidecarClient::dev_uv(repo_root());
    let cfg = ScanConfig::default();

    let csv = std::env::temp_dir().join("ar_e2e.csv");
    std::fs::write(&csv, "symbol,name\nAAPL,Apple\nMSFT,Microsoft\n").unwrap();

    let result =
        scan_universe_impl(csv.to_str().unwrap(), &cfg, &mut cache, &sidecar, now).unwrap();
    println!(
        "scored {} symbols, {} errors",
        result.scores.len(),
        result.errors.len()
    );
    for s in &result.scores {
        println!(
            "  {:<8} state={:?} prox={:.0} score_final={:?} action={:.1}",
            s.symbol, s.signal_state, s.proximity_score, s.score_final, s.actionability
        );
    }
    assert!(!result.scores.is_empty(), "expected at least one score");

    let sym = result.scores[0].symbol.clone();
    let cd = build_chart_data(&cache, &sym, Tf::Daily, &cfg).unwrap();
    let mtf: Vec<_> = cd
        .mtf_summary
        .iter()
        .map(|m| (m.tf.clone(), m.regime, m.velocity.clone()))
        .collect();
    println!(
        "chart {sym}: ohlc={} ema20={} macd_hist={} markers={} mtf={:?}",
        cd.ohlc.len(),
        cd.ema20.len(),
        cd.macd_hist.len(),
        cd.markers.len(),
        mtf
    );
    assert!(!cd.ohlc.is_empty());

    // P7: evaluate the model over the fetched daily history (2 symbols).
    let mut universe = Vec::new();
    for s in &result.scores {
        let candles = cache.load_candles(&s.symbol, Tf::Daily).unwrap();
        if !candles.is_empty() {
            universe.push((AssetClass::Equity, candles));
        }
    }
    let report = evaluate(&universe, &cfg, &EvalConfig::default());
    let o = report.overall;
    println!(
        "EVAL (horizon={}b, n={}): hit={:.1}% p={:.4} expectancy={:.3}% PF={:.2} MFE={:.2}% MAE={:.2}%",
        report.horizon_bars,
        o.n,
        o.hit_rate * 100.0,
        o.binomial_p,
        o.expectancy,
        o.profit_factor,
        o.avg_mfe,
        o.avg_mae
    );
    println!(
        "  OOS: hit={:.1}% n={} | by_state: {:?} | prox_lift: {:?} | degeneracy: {:?}",
        report.out_of_sample.hit_rate * 100.0,
        report.out_of_sample.n,
        report
            .by_state
            .iter()
            .map(|(k, s)| (k.clone(), (s.hit_rate * 100.0).round(), s.n))
            .collect::<Vec<_>>(),
        report
            .proximity_lift
            .iter()
            .map(|(k, s)| (k.clone(), (s.avg_return * 1000.0).round() / 1000.0, s.n))
            .collect::<Vec<_>>(),
        report.degeneracy
    );
    assert!(report.overall.n > 0);

    let _ = std::fs::remove_file(&csv);
}

/// Broader-universe edge re-check (US large caps + JP + crypto).
#[test]
#[ignore = "live network via yfinance/uv; run manually"]
fn live_eval_universe() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mut cache = Cache::open_in_memory().unwrap();
    let sidecar = SidecarClient::dev_uv(repo_root());
    let cfg = ScanConfig::default();

    let csv = std::env::temp_dir().join("ar_eval_universe.csv");
    std::fs::write(
        &csv,
        "symbol\nAAPL\nMSFT\nGOOGL\nNVDA\nAMZN\nMETA\nTSLA\nJPM\nKO\nWMT\nUNH\nXOM\n7974.T\n6758.T\n8306.T\nBTC-USD\nETH-USD\n",
    )
    .unwrap();

    let result =
        scan_universe_impl(csv.to_str().unwrap(), &cfg, &mut cache, &sidecar, now).unwrap();
    println!(
        "scored {} ({} errors)",
        result.scores.len(),
        result.errors.len()
    );

    let mut universe = Vec::new();
    for s in &result.scores {
        let candles = cache.load_candles(&s.symbol, Tf::Daily).unwrap();
        if candles.len() > 250 {
            universe.push((s.asset_class, candles));
        }
    }
    let report = evaluate(&universe, &cfg, &EvalConfig::default());
    let o = report.overall;
    println!(
        "UNIVERSE EVAL: symbols={} n={} hit={:.1}% p={:.4} expectancy={:.3}% PF={:.2} MFE={:.2}% MAE={:.2}%",
        report.n_symbols, o.n, o.hit_rate * 100.0, o.binomial_p, o.expectancy, o.profit_factor, o.avg_mfe, o.avg_mae
    );
    println!(
        "  OOS hit={:.1}% n={}  IS hit={:.1}% n={}",
        report.out_of_sample.hit_rate * 100.0,
        report.out_of_sample.n,
        report.in_sample.hit_rate * 100.0,
        report.in_sample.n
    );
    let row = |kv: &(String, alpha_radar_lib::eval::Stats)| {
        format!(
            "{}: hit={:.0}% exp={:.3}% PF={:.2} n={}",
            kv.0,
            kv.1.hit_rate * 100.0,
            kv.1.expectancy,
            kv.1.profit_factor,
            kv.1.n
        )
    };
    println!(
        "  by_regime: {:?}",
        report.by_regime.iter().map(row).collect::<Vec<_>>()
    );
    println!(
        "  by_asset:  {:?}",
        report.by_asset_class.iter().map(row).collect::<Vec<_>>()
    );
    println!(
        "  by_state:  {:?}",
        report.by_state.iter().map(row).collect::<Vec<_>>()
    );
    println!(
        "  prox_lift: {:?}",
        report.proximity_lift.iter().map(row).collect::<Vec<_>>()
    );
    println!("  degeneracy: {:?}", report.degeneracy);
    assert!(report.overall.n > 0);
    let _ = std::fs::remove_file(&csv);
}

/// P8: walk-forward tuning over the broad universe.
#[test]
#[ignore = "live network via yfinance/uv; run manually"]
fn live_tune() {
    use alpha_radar_lib::eval::tuning::{default_candidates, tune};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mut cache = Cache::open_in_memory().unwrap();
    let sidecar = SidecarClient::dev_uv(repo_root());
    let cfg = ScanConfig::default();

    let csv = std::env::temp_dir().join("ar_tune_universe.csv");
    std::fs::write(
        &csv,
        "symbol\nAAPL\nMSFT\nGOOGL\nNVDA\nAMZN\nMETA\nTSLA\nJPM\nKO\nWMT\nUNH\nXOM\n7974.T\n6758.T\n8306.T\nBTC-USD\nETH-USD\n",
    )
    .unwrap();
    let result =
        scan_universe_impl(csv.to_str().unwrap(), &cfg, &mut cache, &sidecar, now).unwrap();

    let mut universe = Vec::new();
    for s in &result.scores {
        let candles = cache.load_candles(&s.symbol, Tf::Daily).unwrap();
        if candles.len() > 250 {
            universe.push((s.asset_class, candles));
        }
    }

    let report = tune(&universe, &default_candidates(cfg), &EvalConfig::default());
    println!("TUNING (objective: {})", report.objective);
    println!(
        "  baseline: IS exp={:.3}% PF={:.2} (n={}) | OOS exp={:.3}% PF={:.2} (n={})",
        report.baseline.is_expectancy,
        report.baseline.is_pf,
        report.baseline.is_n,
        report.baseline.oos_expectancy,
        report.baseline.oos_pf,
        report.baseline.oos_n
    );
    println!("  ranked by IS expectancy:");
    for r in &report.candidates {
        println!(
            "    {:<22} IS exp={:.3}% PF={:.2} | OOS exp={:.3}% PF={:.2} (oos n={})",
            r.label, r.is_expectancy, r.is_pf, r.oos_expectancy, r.oos_pf, r.oos_n
        );
    }
    println!(
        "  >> BEST (selected on IS): {}  OOS exp={:.3}% PF={:.2}",
        report.best.label, report.best.oos_expectancy, report.best.oos_pf
    );
    let _ = std::fs::remove_file(&csv);
}
