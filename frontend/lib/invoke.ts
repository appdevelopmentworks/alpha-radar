// Tauri command wrappers (docs/01 command surface). All backend access goes
// through here. These only work inside the Tauri webview. The active config is
// persisted backend-side, so scan/chart don't pass it (they read the same one).

import { invoke } from "@tauri-apps/api/core";

import type { ChartData, ScanConfig, ScanResult, Tf } from "./types";
import type { UiPrefs } from "./ui-prefs";

/** The active (persisted) scan configuration, else the tuned Standard preset. */
export function getConfig(): Promise<ScanConfig> {
  return invoke<ScanConfig>("get_config");
}

/** Persist a new active scan configuration. */
export function updateConfig(config: ScanConfig): Promise<void> {
  return invoke("update_config", { config });
}

/**
 * The persisted UI preferences blob (chart toggles); `null` when never saved.
 * Typed `unknown` on purpose — the file is untrusted input, and only
 * `visibilityFromPrefs` (ui-prefs.ts) may interpret it.
 */
export function getUiPrefs(): Promise<unknown> {
  return invoke<unknown>("get_ui_prefs");
}

/** Persist the UI preferences blob (view state only — never the scan config). */
export function updateUiPrefs(prefs: UiPrefs): Promise<void> {
  return invoke("update_ui_prefs", { prefs });
}

/** Named presets: [label, config][] (conservative / standard / aggressive). */
export function getPresets(): Promise<[string, ScanConfig][]> {
  return invoke("get_presets");
}

/** Scan a CSV watchlist (on-disk path) with the active config. */
export function scanUniverse(csvPath: string): Promise<ScanResult> {
  return invoke<ScanResult>("scan_universe", { csvPath });
}

/** Scan a free-text ticker list (comma / space / newline separated). */
export function scanSymbols(symbols: string): Promise<ScanResult> {
  return invoke<ScanResult>("scan_symbols", { symbols });
}

/** Multi-pane chart data for a symbol on a timeframe. */
export function getChartData(symbol: string, tf: Tf): Promise<ChartData> {
  return invoke<ChartData>("get_chart_data", { symbol, tf });
}
