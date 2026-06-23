"use client";

import { createContext, useContext, useState, type ReactNode } from "react";

import type { ScanResult } from "./types";

// Holds the latest scan result above the route tree (in the layout), so
// navigating list → chart → list preserves the ranking without re-scanning.
// (The active config is persisted backend-side; the settings screen edits it.)
interface ScanState {
  result: ScanResult | null;
  setResult: (r: ScanResult | null) => void;
  lastInput: string;
  setLastInput: (s: string) => void;
}

const ScanContext = createContext<ScanState | null>(null);

export function ScanProvider({ children }: { children: ReactNode }) {
  const [result, setResult] = useState<ScanResult | null>(null);
  const [lastInput, setLastInput] = useState("");
  return (
    <ScanContext.Provider value={{ result, setResult, lastInput, setLastInput }}>
      {children}
    </ScanContext.Provider>
  );
}

export function useScan(): ScanState {
  const ctx = useContext(ScanContext);
  if (!ctx) throw new Error("useScan must be used within <ScanProvider>");
  return ctx;
}
