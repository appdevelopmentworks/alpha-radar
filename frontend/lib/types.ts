// Hand-mirrored from the Rust DTOs (src-tauri/src/models.rs, scoring, proximity).
// Keep in sync with serde serialization (ts-rs auto-generation is a future option).

export type Regime = "TrendUp" | "TrendDown" | "Range" | "Transition";

export type Direction = "Buy" | "Sell" | "None";

export type SignalState =
  | "PrimedBuy"
  | "TriggeredBuy"
  | "ActiveBuy"
  | "Neutral"
  | "PrimedSell"
  | "TriggeredSell"
  | "ActiveSell";

export type AssetClass = "equity" | "crypto";

export interface SymbolScore {
  symbol: string;
  name: string | null;
  asset_class: AssetClass;
  regime: Regime | null;
  score_final: number | null;
  score_daily: number | null;
  score_weekly: number | null;
  score_monthly: number | null;
  s_trend: number | null;
  s_momentum: number | null;
  s_mean_reversion: number | null;
  signal_state: SignalState;
  direction: Direction;
  proximity_score: number;
  bars_since_trigger: number | null;
  actionability: number;
  atr: number | null;
  suggested_stop: number | null;
  // Historical hit rate (0..1) of the chart-marker rule over the daily
  // history; null when no marker had a full forward window.
  marker_hit_rate: number | null;
  // Number of marker events the hit rate was evaluated on.
  marker_samples: number;
  // Most recent marker event across all sources (直近マーカー column).
  last_marker: LastMarker | null;
}

export type MarkerKind = "confluence" | "qt_flip" | "qt_precursor";

export interface LastMarker {
  kind: MarkerKind;
  // +1 buy-side, -1 sell-side.
  dir: number;
  // Daily bars since the event; 0 = the latest bar.
  bars_ago: number;
}

export interface RowError {
  symbol: string;
  reason: string;
}

export interface ScanResult {
  scores: SymbolScore[];
  errors: RowError[];
  scanned_at: number; // Unix seconds (UTC)
}

// ScanConfig mirror (src-tauri/src/config.rs). `indicators` / `regime` /
// `weights` are kept opaque (not edited by the settings screen — passed through
// unchanged on save); the settings screen edits the fields below.
export interface MtfConfig {
  alpha: number[]; // [daily, weekly, monthly]
  weekly_gate_aligned: number;
  weekly_gate_neutral: number;
  weekly_gate_opposed: number;
  monthly_mod_aligned: number;
  monthly_mod_opposed: number;
  monthly_enabled: boolean;
}

export interface ProximityConfig {
  approach_floor: number;
  velocity_bars: number;
  velocity_scale: number;
  velocity_max_bonus: number;
  typical_squeeze_len: number;
  cr_buy_zone: number;
  cr_sell_zone: number;
  pull_max_dist_atr: number;
  fresh_bars_n: number;
  active_decay: number;
  primed_floor: number;
  triggered_floor: number;
}

export interface ScanConfig {
  indicators: Record<string, unknown>;
  regime: Record<string, unknown>;
  weights: Record<string, unknown>;
  mtf: MtfConfig;
  proximity: ProximityConfig;
  buy_threshold: number;
  sell_threshold: number;
  squeeze_gate: number;
  min_bars: number;
  stop_atr_mult: number;
  // Chart display only: number of most-recent bars fit in the initial view.
  chart_bars: number;
  // Forward window (daily bars) for the marker hit-rate column.
  marker_horizon_bars: number;
}

export function directionOf(state: SignalState): Direction {
  if (state.endsWith("Buy")) return "Buy";
  if (state.endsWith("Sell")) return "Sell";
  return "None";
}

// ---- Chart DTOs (P6), serialized for lightweight-charts ----

export type Tf = "daily" | "weekly" | "monthly";

export interface Candle {
  ts: number;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
  adj_close: number;
}

export interface TimeValue {
  time: number;
  value: number;
}

export interface HistBar {
  time: number;
  value: number;
  color: string;
}

export interface ChartMarker {
  time: number;
  position: "aboveBar" | "belowBar";
  color: string;
  shape: "arrowUp" | "arrowDown" | "circle";
  text: string;
}

export interface TfSummary {
  tf: string; // "1d" | "1wk" | "1mo"
  regime: Regime | null;
  velocity: string;
}

export interface ChartData {
  ohlc: Candle[];
  ema20: TimeValue[];
  ema50: TimeValue[];
  ema200: TimeValue[];
  supertrend: TimeValue[];
  tenkan: TimeValue[];
  kijun: TimeValue[];
  senkou_a: TimeValue[];
  senkou_b: TimeValue[];
  macd: TimeValue[];
  macd_signal: TimeValue[];
  macd_hist: HistBar[];
  sqz_val: HistBar[];
  score: TimeValue[];
  buy_threshold: number;
  sell_threshold: number;
  markers: ChartMarker[];
  // Q-Trend display layer (ADR-15): trend line, flip markers, precursor circles.
  qtrend: TimeValue[];
  qt_markers: ChartMarker[];
  qt_precursors: ChartMarker[];
  // Supertrend flip markers (ADR-16, ATS visual comparison; default OFF).
  st_markers: ChartMarker[];
  mtf_summary: TfSummary[];
  // Number of most-recent bars to fit in the initial view (from chart_bars).
  initial_bars: number;
}
