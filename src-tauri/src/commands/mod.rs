//! Tauri command surface (docs/01). `scan_universe` ties the data layer (P0/P4)
//! to the computation core (P1–P3): CSV → diff-update fetch → cache → parallel
//! scoring → `ScanResult`.

pub mod chart;

use rayon::prelude::*;
use tauri::Manager;

use crate::config::{Preset, ScanConfig};
use crate::data::cache::{Cache, FetchDecision};
use crate::data::csv;
use crate::data::sidecar::{FetchRequest, FetchRequestItem, SidecarClient};
use crate::data::universe::UniverseEntry;
use crate::error::{AppError, AppResult};
use crate::eval::{evaluate, EvalConfig, EvalReport};
use crate::indicators::atr;
use crate::models::{Candle, ChartData, RowError, ScanResult, SymbolScore, Tf};
use crate::proximity::{actionability, latest_proximity, Direction, SignalState};
use crate::scoring::composite::category_scores;
use crate::scoring::direction_score;

/// A symbol's loaded multi-timeframe candles (weekly/monthly may be absent).
struct SymbolData {
    entry: UniverseEntry,
    daily: Vec<Candle>,
    weekly: Option<Vec<Candle>>,
    monthly: Option<Vec<Candle>>,
}

/// Suggested stop distance from the last close (docs/06 risk field).
fn suggested_stop(close: Option<f64>, atr: Option<f64>, dir: Direction, mult: f64) -> Option<f64> {
    let (c, a) = (close?, atr?);
    match dir {
        Direction::Buy => Some(c - mult * a),
        Direction::Sell => Some(c + mult * a),
        Direction::None => None,
    }
}

/// Assemble both axes (direction + proximity) plus risk fields into a
/// `SymbolScore`. Pure — safe to run across rayon threads.
fn assemble_symbol_score(d: &SymbolData, cfg: &ScanConfig) -> SymbolScore {
    let ds = direction_score(&d.daily, d.weekly.as_deref(), d.monthly.as_deref(), cfg);
    let cats = category_scores(&d.daily, cfg).last().copied();

    let high: Vec<f64> = d.daily.iter().map(|c| c.high).collect();
    let low: Vec<f64> = d.daily.iter().map(|c| c.low).collect();
    let close: Vec<f64> = d.daily.iter().map(|c| c.close).collect();
    let atr_last = atr(&high, &low, &close, cfg.indicators.atr_period)
        .last()
        .copied()
        .flatten();

    let (state, proximity_score, bars_since) = match latest_proximity(&d.daily, cfg) {
        Some(p) => (p.state, p.proximity_score, p.bars_since_trigger),
        None => (SignalState::Neutral, 0.0, None),
    };
    let direction = state.direction();
    let action = actionability(proximity_score, ds.score_final.unwrap_or(0.0));
    let stop = suggested_stop(
        close.last().copied(),
        atr_last,
        direction,
        cfg.stop_atr_mult,
    );

    SymbolScore {
        symbol: d.entry.symbol.clone(),
        name: d.entry.name.clone(),
        asset_class: d.entry.asset_class,
        regime: ds.regime_daily,
        score_final: ds.score_final,
        score_daily: ds.score_daily,
        score_weekly: ds.score_weekly,
        score_monthly: ds.score_monthly,
        s_trend: cats.and_then(|c| c.trend),
        s_momentum: cats.and_then(|c| c.momentum),
        s_mean_reversion: cats.and_then(|c| c.mean_reversion),
        signal_state: state,
        direction,
        proximity_score,
        bars_since_trigger: bars_since,
        actionability: action,
        atr: atr_last,
        suggested_stop: stop,
    }
}

fn nonempty(v: Vec<Candle>) -> Option<Vec<Candle>> {
    (!v.is_empty()).then_some(v)
}

fn tf_from_interval(s: &str) -> Option<Tf> {
    Tf::ALL.into_iter().find(|tf| tf.interval() == s)
}

/// Core scan logic (testable without Tauri). `now` is Unix seconds. Per-row
/// failures are collected; the scan never aborts (docs/01 error policy).
fn scan_entries(
    universe: Vec<UniverseEntry>,
    mut errors: Vec<RowError>,
    source: &str,
    cfg: &ScanConfig,
    cache: &mut Cache,
    sidecar: &SidecarClient,
    now: i64,
) -> AppResult<ScanResult> {
    // 1. Decide which (symbol, tf) need fetching (differential update, docs/04).
    let mut fetch_items = Vec::new();
    for e in &universe {
        for tf in Tf::ALL {
            let (start, fetch) = match cache.fetch_plan(&e.symbol, tf, now)? {
                FetchDecision::Full => (None, true),
                FetchDecision::From(last) => (Some(last), true),
                FetchDecision::Skip => (None, false),
            };
            if fetch {
                fetch_items.push(FetchRequestItem {
                    symbol: e.symbol.clone(),
                    interval: tf.interval().into(),
                    start,
                    end: None,
                });
            }
        }
    }

    // 2. One batched sidecar call for the missing data; upsert into the cache.
    if !fetch_items.is_empty() {
        let resp = sidecar.fetch(&FetchRequest::json(fetch_items))?;
        for r in resp.results {
            if let Some(tf) = tf_from_interval(&r.interval) {
                let candles: Vec<Candle> = r.candles.into_iter().map(Into::into).collect();
                cache.upsert_candles(&r.symbol, tf, &candles)?;
            }
        }
        for fe in resp.errors {
            errors.push(RowError {
                symbol: fe.symbol,
                reason: format!("fetch {}: {}", fe.interval, fe.reason),
            });
        }
    }

    for e in &universe {
        cache.upsert_symbol(e, now)?;
    }

    // 3. Load candles into memory (the cache is !Sync; DB stays single-threaded).
    let mut data = Vec::new();
    for e in &universe {
        let daily = cache.load_candles(&e.symbol, Tf::Daily)?;
        if daily.len() < cfg.min_bars {
            errors.push(RowError {
                symbol: e.symbol.clone(),
                reason: format!(
                    "insufficient history: {} < {} bars",
                    daily.len(),
                    cfg.min_bars
                ),
            });
            continue;
        }
        let weekly = nonempty(cache.load_candles(&e.symbol, Tf::Weekly)?);
        let monthly = nonempty(cache.load_candles(&e.symbol, Tf::Monthly)?);
        data.push(SymbolData {
            entry: e.clone(),
            daily,
            weekly,
            monthly,
        });
    }

    // 4. Score in parallel (pure functions, no I/O).
    let scores: Vec<SymbolScore> = data
        .par_iter()
        .map(|d| assemble_symbol_score(d, cfg))
        .collect();

    // 5. Persist scores + the config snapshot.
    for s in &scores {
        cache.save_score(s, now)?;
    }
    let config_json = serde_json::to_string(cfg)?;
    cache.save_scan_run(now, source, &config_json, universe.len(), errors.len())?;

    Ok(ScanResult {
        scores,
        errors,
        scanned_at: now,
    })
}

/// Scan a CSV watchlist file.
pub fn scan_universe_impl(
    csv_path: &str,
    cfg: &ScanConfig,
    cache: &mut Cache,
    sidecar: &SidecarClient,
    now: i64,
) -> AppResult<ScanResult> {
    let (universe, errors) = csv::parse_csv_path(csv_path)?;
    scan_entries(universe, errors, csv_path, cfg, cache, sidecar, now)
}

/// Scan a free-text ticker list (comma / space / newline separated).
pub fn scan_symbols_impl(
    input: &str,
    cfg: &ScanConfig,
    cache: &mut Cache,
    sidecar: &SidecarClient,
    now: i64,
) -> AppResult<ScanResult> {
    let universe = crate::data::universe::parse_symbols_str(input);
    scan_entries(universe, Vec::new(), "(symbols)", cfg, cache, sidecar, now)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn app_data_dir(app: &tauri::AppHandle) -> AppResult<std::path::PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::InvalidInput(format!("app_data_dir: {e}")))?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Load the persisted active config, falling back to the P8-tuned Standard
/// preset when none is saved yet (config persistence lives backend-side so the
/// scan and the chart always read the same config — ADR-06/10).
fn load_active_config(app: &tauri::AppHandle) -> ScanConfig {
    app_data_dir(app)
        .ok()
        .map(|d| d.join("config.json"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| ScanConfig::preset(Preset::Standard))
}

/// Scan a CSV universe and return the ranked scores (docs/01 command contract).
#[tauri::command]
pub async fn scan_universe(
    app: tauri::AppHandle,
    csv_path: String,
) -> Result<ScanResult, AppError> {
    let cfg = load_active_config(&app);
    let cache_path = app_data_dir(&app)?.join("alpha-radar.db");
    let now = now_unix();

    tauri::async_runtime::spawn_blocking(move || {
        let mut cache = Cache::open(&cache_path)?;
        let sidecar = SidecarClient::resolve();
        scan_universe_impl(&csv_path, &cfg, &mut cache, &sidecar, now)
    })
    .await
    .map_err(|e| AppError::Sidecar(format!("scan task join: {e}")))?
}

/// Scan a free-text ticker list (comma / space / newline separated) instead of
/// a CSV file.
#[tauri::command]
pub async fn scan_symbols(app: tauri::AppHandle, symbols: String) -> Result<ScanResult, AppError> {
    let cfg = load_active_config(&app);
    let cache_path = app_data_dir(&app)?.join("alpha-radar.db");
    let now = now_unix();

    tauri::async_runtime::spawn_blocking(move || {
        let mut cache = Cache::open(&cache_path)?;
        let sidecar = SidecarClient::resolve();
        scan_symbols_impl(&symbols, &cfg, &mut cache, &sidecar, now)
    })
    .await
    .map_err(|e| AppError::Sidecar(format!("scan task join: {e}")))?
}

/// Return the active scan configuration (persisted, else the tuned Standard
/// preset). Used by the settings screen and as the scan/chart config.
#[tauri::command]
pub fn get_config(app: tauri::AppHandle) -> ScanConfig {
    load_active_config(&app)
}

/// Persist a new active scan configuration (docs/05 settings; ADR-10).
#[tauri::command]
pub fn update_config(app: tauri::AppHandle, config: ScanConfig) -> Result<(), AppError> {
    let path = app_data_dir(&app)?.join("config.json");
    std::fs::write(path, serde_json::to_string_pretty(&config)?)?;
    Ok(())
}

/// Return the named presets (Conservative / Standard / Aggressive) for the
/// settings screen.
#[tauri::command]
pub fn get_presets() -> Vec<(String, ScanConfig)> {
    [
        ("conservative", Preset::Conservative),
        ("standard", Preset::Standard),
        ("aggressive", Preset::Aggressive),
    ]
    .into_iter()
    .map(|(label, p)| (label.to_string(), ScanConfig::preset(p)))
    .collect()
}

/// Validate the score/proximity model over the cached history of a universe
/// (docs/07 evaluation harness, P7). Must pass before tuning (P8).
#[tauri::command]
pub async fn evaluate_model(
    app: tauri::AppHandle,
    symbols: String,
    eval: EvalConfig,
) -> Result<EvalReport, AppError> {
    let cfg = load_active_config(&app);
    let cache_path = app_data_dir(&app)?.join("alpha-radar.db");
    tauri::async_runtime::spawn_blocking(move || -> AppResult<EvalReport> {
        let cache = Cache::open(&cache_path)?;
        let entries = crate::data::universe::parse_symbols_str(&symbols);
        let mut universe = Vec::new();
        for e in &entries {
            let candles = cache.load_candles(&e.symbol, Tf::Daily)?;
            if !candles.is_empty() {
                universe.push((e.asset_class, candles));
            }
        }
        Ok(evaluate(&universe, &cfg, &eval))
    })
    .await
    .map_err(|e| AppError::Sidecar(format!("eval task join: {e}")))?
}

/// Multi-pane chart data for one symbol × timeframe (docs/01 command contract).
/// Uses the active config, so the chart matches the ranking.
#[tauri::command]
pub async fn get_chart_data(
    app: tauri::AppHandle,
    symbol: String,
    tf: Tf,
) -> Result<ChartData, AppError> {
    let cfg = load_active_config(&app);
    let cache_path = app_data_dir(&app)?.join("alpha-radar.db");
    tauri::async_runtime::spawn_blocking(move || {
        let cache = Cache::open(&cache_path)?;
        chart::build_chart_data(&cache, &symbol, tf, &cfg)
    })
    .await
    .map_err(|e| AppError::Sidecar(format!("chart task join: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_tf(cache: &mut Cache, symbol: &str, tf: Tf, count: usize, now: i64) {
        let step = tf.bar_seconds();
        let candles: Vec<Candle> = (0..count)
            .map(|i| {
                let ts = now - ((count - 1 - i) as i64) * step;
                let c = 100.0 + i as f64;
                Candle::ohlcv(ts, c, c + 1.0, c - 1.0, c, 1_000_000.0)
            })
            .collect();
        cache.upsert_candles(symbol, tf, &candles).unwrap();
    }

    #[test]
    fn scan_uses_fresh_cache_without_fetching() {
        let now = 220 * 86_400;
        let mut cache = Cache::open_in_memory().unwrap();
        for tf in Tf::ALL {
            seed_tf(&mut cache, "AAPL", tf, 220, now);
        }

        // Write a temp CSV with one symbol.
        let csv_path = std::env::temp_dir().join("alpha_radar_scan_test.csv");
        std::fs::write(&csv_path, "symbol,name\nAAPL,Apple\n").unwrap();

        // Sidecar points at a nonexistent program; it must never be called
        // because all timeframes are fresh.
        let sidecar = SidecarClient::new("no-such-program-xyz", vec![]);
        let cfg = ScanConfig::default();
        let result =
            scan_universe_impl(csv_path.to_str().unwrap(), &cfg, &mut cache, &sidecar, now)
                .unwrap();

        assert_eq!(result.scores.len(), 1);
        assert!(result.errors.is_empty());
        let s = &result.scores[0];
        assert_eq!(s.symbol, "AAPL");
        assert!((0.0..=100.0).contains(&s.proximity_score));
        // The persisted score is queryable back from the cache.
        let _ = std::fs::remove_file(&csv_path);
    }

    #[test]
    fn short_history_becomes_row_error() {
        let now = 220 * 86_400;
        let mut cache = Cache::open_in_memory().unwrap();
        seed_tf(&mut cache, "TINY", Tf::Daily, 10, now); // < min_bars

        let csv_path = std::env::temp_dir().join("alpha_radar_scan_short.csv");
        std::fs::write(&csv_path, "symbol\nTINY\n").unwrap();
        // Make weekly/monthly fresh too so no fetch is attempted.
        seed_tf(&mut cache, "TINY", Tf::Weekly, 10, now);
        seed_tf(&mut cache, "TINY", Tf::Monthly, 10, now);

        let sidecar = SidecarClient::new("no-such-program-xyz", vec![]);
        let cfg = ScanConfig::default();
        let result =
            scan_universe_impl(csv_path.to_str().unwrap(), &cfg, &mut cache, &sidecar, now)
                .unwrap();

        assert!(result.scores.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].reason.contains("insufficient history"));
        let _ = std::fs::remove_file(&csv_path);
    }
}
