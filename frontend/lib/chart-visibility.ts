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

export const DEFAULT_VISIBILITY: ChartVisibility = {
  ema: true,
  supertrend: true,
  ichimoku: false, // busiest overlay — hidden by default to declutter
  macd: true,
  squeeze: true,
  score: true,
  markers: true,
  // Q-Trend layer defaults ON: the point of the layer is eyeballing flip /
  // pre-flip timing, and it is sparse (≤1 marker per leg + one line).
  qtrend: true,
  qtPrecursor: true,
  // Supertrend flips (ATS visual comparison): measured no standalone edge
  // (hit10 50.3% / PF 1.01 — ADR-14 rule B), so OFF by default.
  stFlip: false,
};
