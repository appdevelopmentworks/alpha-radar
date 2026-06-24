// Per-indicator chart overlay visibility (the chart's checkbox state).
// Lives here (not in the chart component) so it can be shared by the chart,
// the toolbar, and the cross-navigation store as a single source of truth.

export interface ChartVisibility {
  ema: boolean;
  supertrend: boolean;
  ichimoku: boolean;
  macd: boolean;
  squeeze: boolean;
  markers: boolean;
}

export const DEFAULT_VISIBILITY: ChartVisibility = {
  ema: true,
  supertrend: true,
  ichimoku: false, // busiest overlay — hidden by default to declutter
  macd: true,
  squeeze: true,
  markers: true,
};
