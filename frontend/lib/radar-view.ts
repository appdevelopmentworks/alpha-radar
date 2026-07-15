// Radar (ranking list) view state: sort column/direction + filters. Lives here
// (not in the table component) so it can be held in the cross-navigation store,
// preserving sort/filter when returning from the chart to the radar.

import type { AssetClass } from "./types";

export type SortField =
  | "actionability" // default ranking (docs/05): timing × conviction; not a visible column
  | "symbol"
  | "signal_state"
  | "proximity_score"
  | "score_final"
  | "regime"
  | "marker_hit_rate"
  | "last_marker_bars"
  | "bars_since_trigger"
  | "atr"
  | "suggested_stop";

export type SortDir = "asc" | "desc";

export interface Sort {
  field: SortField;
  dir: SortDir;
}

export interface RadarView {
  sort: Sort;
  asset: AssetClass | "all";
  query: string;
  showNeutral: boolean;
}

export const DEFAULT_RADAR_VIEW: RadarView = {
  // Default: actionability desc (docs/05) — "what can I trade now" stays on top.
  sort: { field: "actionability", dir: "desc" },
  asset: "all",
  query: "",
  showNeutral: false,
};
