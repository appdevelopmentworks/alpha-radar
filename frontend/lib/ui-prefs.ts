// Shape of app_data_dir/ui_prefs.json. Rust stores this blob opaquely
// (serde_json::Value — ADR-17), so the schema is owned here: adding a pref
// needs no backend change. Reads are deliberately tolerant — a file written by
// an older or newer build, or a truncated one, must degrade to defaults rather
// than throw.

import { type ChartVisibility, DEFAULT_VISIBILITY } from "./chart-visibility";

export interface UiPrefs {
  chartVisibility: ChartVisibility;
}

/** Narrow an unknown JSON value to a plain object (not null, not an array). */
function asRecord(v: unknown): Record<string, unknown> | null {
  return typeof v === "object" && v !== null && !Array.isArray(v)
    ? (v as Record<string, unknown>)
    : null;
}

/**
 * Start from DEFAULT_VISIBILITY and copy only known keys whose saved value is a
 * boolean. Unknown/stale keys are dropped, a toggle added in a later build keeps
 * its default, and a corrupt file yields the defaults.
 */
export function mergeVisibility(saved: unknown): ChartVisibility {
  const merged = { ...DEFAULT_VISIBILITY };
  const obj = asRecord(saved);
  if (!obj) return merged;
  for (const key of Object.keys(DEFAULT_VISIBILITY) as (keyof ChartVisibility)[]) {
    const v = obj[key];
    if (typeof v === "boolean") merged[key] = v;
  }
  return merged;
}

/** Extract the chart visibility from a raw ui_prefs.json value. */
export function visibilityFromPrefs(prefs: unknown): ChartVisibility {
  return mergeVisibility(asRecord(prefs)?.chartVisibility);
}
