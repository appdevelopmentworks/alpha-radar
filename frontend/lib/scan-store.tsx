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
import type { ScanResult } from "./types";

// Holds the latest scan result above the route tree (in the layout), so
// navigating list → chart → list preserves the ranking without re-scanning.
// The chart's indicator visibility lives here too, so toggling checkboxes,
// returning to the radar, and reopening a chart restores the prior toggles.
// (The active config is persisted backend-side; the settings screen edits it.)
interface ScanState {
  result: ScanResult | null;
  setResult: (r: ScanResult | null) => void;
  lastInput: string;
  setLastInput: (s: string) => void;
  chartVisibility: ChartVisibility;
  setChartVisibility: Dispatch<SetStateAction<ChartVisibility>>;
}

const ScanContext = createContext<ScanState | null>(null);

export function ScanProvider({ children }: { children: ReactNode }) {
  const [result, setResult] = useState<ScanResult | null>(null);
  const [lastInput, setLastInput] = useState("");
  const [chartVisibility, setChartVisibility] =
    useState<ChartVisibility>(DEFAULT_VISIBILITY);
  return (
    <ScanContext.Provider
      value={{
        result,
        setResult,
        lastInput,
        setLastInput,
        chartVisibility,
        setChartVisibility,
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
