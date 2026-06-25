"use client";

import { useEffect, useState } from "react";

import Link from "next/link";

import { listen } from "@tauri-apps/api/event";

import { DropZone } from "@/components/DropZone";
import { RankingTable } from "@/components/RankingTable";
import { scanSymbols, scanUniverse } from "@/lib/invoke";
import { useScan } from "@/lib/scan-store";
import type { ScanResult, SymbolScore } from "@/lib/types";

// Mirrors the Rust `ScanProgress` (commands/mod.rs) emitted on `scan-progress`.
interface ScanProgress {
  phase: "fetch" | "load" | "done";
  done: number;
  total: number;
}

const EXPORT_COLS: (keyof SymbolScore)[] = [
  "symbol",
  "name",
  "asset_class",
  "direction",
  "signal_state",
  "regime",
  "proximity_score",
  "actionability",
  "score_final",
  "bars_since_trigger",
  "atr",
  "suggested_stop",
];

function ScanProgressBar({ progress }: { progress: ScanProgress | null }) {
  // "load" gives a determinate per-symbol bar; "fetch" (one batched network
  // call) and the initial state are indeterminate (animated).
  const determinate = progress?.phase === "load" && progress.total > 0;
  const pct = determinate ? Math.round((progress.done / progress.total) * 100) : 0;
  const label = !progress
    ? "スキャン準備中…"
    : progress.phase === "fetch"
      ? `データ取得中…（${progress.total}銘柄）`
      : progress.phase === "load"
        ? `スコアリング ${progress.done} / ${progress.total}`
        : "仕上げ中…";
  return (
    <div className="scan-progress" role="status" aria-live="polite">
      <div className="scan-progress-label">
        <span>{label}</span>
        {determinate && <span className="scan-progress-pct">{pct}%</span>}
      </div>
      <div className={`progress ${determinate ? "" : "indeterminate"}`}>
        <div className="progress-fill" style={determinate ? { width: `${pct}%` } : undefined} />
      </div>
    </div>
  );
}

function csvCell(v: unknown): string {
  if (v == null) return "";
  const s = String(v);
  return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

function download(filename: string, content: string, mime: string) {
  const url = URL.createObjectURL(new Blob([content], { type: mime }));
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

export default function Home() {
  const { result, setResult, lastInput, setLastInput } = useScan();
  const [scanning, setScanning] = useState(false);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [errorsOpen, setErrorsOpen] = useState(false);

  // Subscribe to backend scan-progress events (emitted per symbol while scoring).
  useEffect(() => {
    const un = listen<ScanProgress>("scan-progress", (e) => setProgress(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  async function run(fn: () => Promise<ScanResult>) {
    setError(null);
    setProgress(null);
    setScanning(true);
    try {
      setResult(await fn());
    } catch (e) {
      setError(String(e));
    } finally {
      setScanning(false);
      setProgress(null);
    }
  }

  const onFile = (path: string) => run(() => scanUniverse(path));
  const onScanTickers = () => {
    if (lastInput.trim()) run(() => scanSymbols(lastInput));
  };

  function exportAs(format: "json" | "csv") {
    if (!result) return;
    if (format === "json") {
      download(
        `alpha-radar-${result.scanned_at}.json`,
        JSON.stringify(result.scores, null, 2),
        "application/json",
      );
    } else {
      const head = EXPORT_COLS.join(",");
      const lines = result.scores.map((s) => EXPORT_COLS.map((c) => csvCell(s[c])).join(","));
      download(`alpha-radar-${result.scanned_at}.csv`, [head, ...lines].join("\n"), "text/csv");
    }
  }

  return (
    <main className="scanner">
      <header className="app-header">
        <div>
          <h1>Alpha Radar</h1>
          <span className="subtitle">Swing Entry Confluence Scanner</span>
        </div>
        <div className="header-actions">
          {result && (
            <div className="export">
              <button onClick={() => exportAs("csv")}>CSV 出力</button>
              <button onClick={() => exportAs("json")}>JSON 出力</button>
            </div>
          )}
          <Link href="/settings" className="settings-link">
            ⚙ 設定
          </Link>
        </div>
      </header>

      <section className="ticker-input">
        <label htmlFor="tickers">複数のティッカーを入力（カンマ・スペース・改行区切り）</label>
        <div className="ticker-row">
          <textarea
            id="tickers"
            value={lastInput}
            onChange={(e) => setLastInput(e.target.value)}
            onKeyDown={(e) => {
              if ((e.ctrlKey || e.metaKey) && e.key === "Enter") onScanTickers();
            }}
            placeholder="AAPL, MSFT, 7974.T, BTC-USD"
            disabled={scanning}
            rows={2}
          />
          <button
            className="scan-btn"
            onClick={onScanTickers}
            disabled={scanning || !lastInput.trim()}
          >
            スキャン実行
          </button>
        </div>
      </section>

      <DropZone onFile={onFile} disabled={scanning} />

      {scanning && <ScanProgressBar progress={progress} />}
      {error && <div className="status error">エラー: {error}</div>}

      {result && (
        <>
          {result.errors.length > 0 && (
            <div className="errors">
              <button className="errors-toggle" onClick={() => setErrorsOpen((v) => !v)}>
                {errorsOpen ? "▼" : "▶"} エラー（{result.errors.length}件）
              </button>
              {errorsOpen && (
                <ul>
                  {result.errors.map((e, i) => (
                    <li key={i}>
                      <code>{e.symbol}</code> {e.reason}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
          <RankingTable scores={result.scores} scannedAt={result.scanned_at} />
        </>
      )}
    </main>
  );
}
