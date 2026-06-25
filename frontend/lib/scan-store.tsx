"use client";

import {
  createContext,
  useContext,
  useState,
  type Dispatch,
  type ReactNode,
  type SetStateAction,
} from "react";

import { type ChartVisibility, DEFAULT_VISIBILITY } from "./chart-visibility";
import { type RadarView, DEFAULT_RADAR_VIEW } from "./radar-view";
import type { ScanResult } from "./types";

// Holds the latest scan result above the route tree (in the layout), so
// navigating list → chart → list preserves the ranking without re-scanning.
// The chart's indicator visibility and the radar's sort/filter view live here
// too, so navigating away and back restores both (the checkbox state and the
// ranking's sort/filters). (The active config is persisted backend-side; the
// settings screen edits it.)
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
