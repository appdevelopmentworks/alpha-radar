// Per-indicator chart overlay visibility (the chart's checkbox state).
// Lives here (not in the chart component) so it can be shared by the chart,
// the toolbar, and the cross-navigation store as a single source of truth.

export interface ChartVisibility {
  ema: boolean;
  supertrend: boolean;
  ichimoku: boolean;
  macd: boolean;
  squeeze: boolean;
  score: boolean;
  markers: boolean;
  qtrend: boolean;
  qtPrecursor: boolean;
  stFlip: boolean;
}

// Shin's preferred initial view: EMA ribbon + MACD + Squeeze + the Q-Trend
// layer (line/flips/precursors) + ST flips. Supertrend line, Ichimoku, the
// score pane, and the confluence markers start hidden (all toggleable).
export const DEFAULT_VISIBILITY: ChartVisibility = {
  ema: true,
  supertrend: false,
  ichimoku: false,
  macd: true,
  squeeze: true,
  score: false,
  markers: false,
  qtrend: true,
  qtPrecursor: true,
  stFlip: true,
};
