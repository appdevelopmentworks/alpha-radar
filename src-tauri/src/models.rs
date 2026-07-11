//! Core data-transfer objects shared across the computation pipeline.
//!
//! All DTOs are `serde`-(de)serializable and manually mirrored to
//! `frontend/lib/types.ts` (docs/01-architecture.md).

use serde::{Deserialize, Serialize};

use crate::proximity::{Direction, SignalState};
use crate::regime::Regime;

/// A single OHLCV bar.
///
/// `ts` is the bar-open time as a Unix timestamp (seconds, UTC). Prices are
/// split/dividend-adjusted upstream by the Python sidecar (`auto_adjust`);
/// `adj_close` is retained even though it usually equals `close`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Candle {
    pub ts: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub adj_close: f64,
}

impl Candle {
    /// Convenience constructor; sets `adj_close = close`.
    pub fn ohlcv(ts: i64, open: f64, high: f64, low: f64, close: f64, volume: f64) -> Self {
        Self {
            ts,
            open,
            high,
            low,
            close,
            volume,
            adj_close: close,
        }
    }
}

/// Timeframe of a series. Proximity/timing is read from `Daily`; `Weekly` and
/// `Monthly` provide MTF context/gates (ADR-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tf {
    Daily,
    Weekly,
    Monthly,
}

impl Tf {
    /// yfinance interval string.
    pub fn interval(self) -> &'static str {
        match self {
            Tf::Daily => "1d",
            Tf::Weekly => "1wk",
            Tf::Monthly => "1mo",
        }
    }

    /// Approximate seconds per bar (for cache-freshness heuristics).
    pub fn bar_seconds(self) -> i64 {
        match self {
            Tf::Daily => 86_400,
            Tf::Weekly => 7 * 86_400,
            Tf::Monthly => 30 * 86_400,
        }
    }

    pub const ALL: [Tf; 3] = [Tf::Daily, Tf::Weekly, Tf::Monthly];
}

/// Asset class. Inferred from the symbol when the CSV omits it (docs/04).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetClass {
    Equity,
    Crypto,
}

impl AssetClass {
    /// Heuristic from the yfinance symbol: Yahoo crypto pairs end with `-USD`
    /// (or `-USDT`); everything else is treated as equity.
    pub fn infer(symbol: &str) -> Self {
        let s = symbol.to_uppercase();
        if s.ends_with("-USD") || s.ends_with("-USDT") {
            AssetClass::Crypto
        } else {
            AssetClass::Equity
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AssetClass::Equity => "equity",
            AssetClass::Crypto => "crypto",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "equity" | "equities" | "stock" => Some(AssetClass::Equity),
            "crypto" | "cryptocurrency" => Some(AssetClass::Crypto),
            _ => None,
        }
    }
}

/// A row-scoped failure collected during a scan; never aborts the whole run
/// (docs/01, ADR error policy).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowError {
    pub symbol: String,
    pub reason: String,
}

/// Per-symbol scan result: the direction axis, the proximity axis, and risk
/// fields. Persisted to the `scores` table (docs/04).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolScore {
    pub symbol: String,
    pub name: Option<String>,
    pub asset_class: AssetClass,
    pub regime: Option<Regime>,
    pub score_final: Option<f64>,
    pub score_daily: Option<f64>,
    pub score_weekly: Option<f64>,
    pub score_monthly: Option<f64>,
    pub s_trend: Option<f64>,
    pub s_momentum: Option<f64>,
    pub s_mean_reversion: Option<f64>,
    pub signal_state: SignalState,
    pub direction: Direction,
    pub proximity_score: f64,
    pub bars_since_trigger: Option<u32>,
    pub actionability: f64,
    pub atr: Option<f64>,
    pub suggested_stop: Option<f64>,
    /// Historical hit rate ∈ [0,1] of the FR-8 marker rule over this symbol's
    /// daily history (signed forward return over `marker_horizon_bars` > 0).
    /// `None` when no marker has a full forward window.
    pub marker_hit_rate: Option<f64>,
    /// Number of marker events the hit rate was evaluated on.
    pub marker_samples: u32,
}

/// Result of a universe scan returned across the Tauri boundary (docs/01).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanResult {
    pub scores: Vec<SymbolScore>,
    pub errors: Vec<RowError>,
    /// Scan time, Unix seconds (UTC).
    pub scanned_at: i64,
}

// ---- Chart DTOs (P6) — shaped for direct use by lightweight-charts v5 ----

/// `{ time, value }` line point (`time` = Unix seconds).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TimeValue {
    pub time: i64,
    pub value: f64,
}

/// A colored histogram bar (MACD / Squeeze 4-color).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistBar {
    pub time: i64,
    pub value: f64,
    pub color: String,
}

/// A BUY/SELL marker on the price pane (series-markers plugin).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartMarker {
    pub time: i64,
    pub position: String, // "aboveBar" | "belowBar"
    pub color: String,
    pub shape: String, // "arrowUp" | "arrowDown"
    pub text: String,
}

/// One row of the MTF summary header (docs/05).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TfSummary {
    pub tf: String,
    pub regime: Option<Regime>,
    pub velocity: String, // "Accelerating" | "Decelerating" | "Flat"
}

/// All series for the multi-pane chart of one symbol × timeframe (docs/01/05).
/// Computed entirely in Rust so the chart and the ranking score always agree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartData {
    pub ohlc: Vec<Candle>,
    pub ema20: Vec<TimeValue>,
    pub ema50: Vec<TimeValue>,
    pub ema200: Vec<TimeValue>,
    pub supertrend: Vec<TimeValue>,
    pub tenkan: Vec<TimeValue>,
    pub kijun: Vec<TimeValue>,
    pub senkou_a: Vec<TimeValue>,
    pub senkou_b: Vec<TimeValue>,
    pub macd: Vec<TimeValue>,
    pub macd_signal: Vec<TimeValue>,
    pub macd_hist: Vec<HistBar>,
    pub sqz_val: Vec<HistBar>,
    pub score: Vec<TimeValue>,
    pub buy_threshold: f64,
    pub sell_threshold: f64,
    pub markers: Vec<ChartMarker>,
    pub mtf_summary: Vec<TfSummary>,
    /// Display-only: how many of the most recent bars to fit in the initial
    /// view (from `ScanConfig::chart_bars`). The full series is still sent.
    pub initial_bars: usize,
}
