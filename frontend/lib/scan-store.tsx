"use client";

import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type ReactNode,
  type SetStateAction,
} from "react";

import { type ChartVisibility, DEFAULT_VISIBILITY } from "./chart-visibility";
import { getUiPrefs, updateUiPrefs } from "./invoke";
import { type RadarView, DEFAULT_RADAR_VIEW } from "./radar-view";
import type { ScanResult } from "./types";
import { visibilityFromPrefs } from "./ui-prefs";

// Holds the latest scan result above the route tree (in the layout), so
// navigating list → chart → list preserves the ranking without re-scanning.
// The chart's indicator visibility and the radar's sort/filter view live here
// too, so navigating away and back restores both (the checkbox state and the
// ranking's sort/filters). The chart visibility ALSO survives an app restart
// (ui_prefs.json, ADR-17); the radar view and the chart timeframe do not.
// (The active scan config is persisted separately backend-side; the settings
// screen edits it.)
interface ScanState {
  result: ScanResult | null;
  setResult: (r: ScanResult | null) => void;
  lastInput: string;
  setLastInput: (s: string) => void;
  chartVisibility: ChartVisibility;
  setChartVisibility: Dispatch<SetStateAction<ChartVisibility>>;
  radarView: RadarView;
  setRadarView: Dispatch<SetStateAction<RadarView>>;
}

const ScanContext = createContext<ScanState | null>(null);

export function ScanProvider({ children }: { children: ReactNode }) {
  const [result, setResult] = useState<ScanResult | null>(null);
  const [lastInput, setLastInput] = useState("");
  const [chartVisibility, setChartVisibility] =
    useState<ChartVisibility>(DEFAULT_VISIBILITY);
  const [radarView, setRadarView] = useState<RadarView>(DEFAULT_RADAR_VIEW);

  // The chart toggles are the only persisted UI pref (app_data_dir/ui_prefs.json
  // — ADR-17). The timeframe and the radar sort/filter stay session-scoped by
  // design. `hydrated` gates the writer so the initial defaults never overwrite
  // the saved file before the read resolves.
  const hydrated = useRef(false);

  useEffect(() => {
    let active = true;
    getUiPrefs()
      .then((p) => {
        if (!active) return;
        // Identity check: useState was seeded with the DEFAULT_VISIBILITY
        // constant *by reference*, and every toggle allocates a new object. So
        // `cur === DEFAULT_VISIBILITY` proves nothing was toggled while the read
        // was in flight, and applying the saved value cannot clobber the user.
        // (Seeding with `{...DEFAULT_VISIBILITY}` would silently break this.)
        setChartVisibility((cur) =>
          cur === DEFAULT_VISIBILITY ? visibilityFromPrefs(p) : cur,
        );
      })
      .catch(() => {
        // Best-effort: an unreadable prefs file (or a plain browser, where
        // invoke throws) just leaves DEFAULT_VISIBILITY in place.
      })
      .finally(() => {
        if (active) hydrated.current = true;
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!hydrated.current) return;
    updateUiPrefs({ chartVisibility }).catch((e) => {
      // No UI surface — prefs are cosmetic; but silent loss would leave the
      // user with toggles that mysteriously don't stick.
      console.warn("failed to persist ui prefs:", e);
    });
  }, [chartVisibility]);

  return (
    <ScanContext.Provider
      value={{
        result,
        setResult,
        lastInput,
        setLastInput,
        chartVisibility,
        setChartVisibility,
        radarView,
        setRadarView,
      }}
    >
      {children}
    </ScanContext.Provider>
  );
}

export function useScan(): ScanState {
  const ctx = useContext(ScanContext);
  if (!ctx) throw new Error("useScan must be used within <ScanProvider>");
  return ctx;
}
