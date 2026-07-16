"use client";

import { Suspense, useEffect, useState } from "react";

import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";

import { MtfSummary } from "@/components/MtfSummary";
import { MultiPaneChart } from "@/components/MultiPaneChart";
import type { ChartVisibility } from "@/lib/chart-visibility";
import { getChartData } from "@/lib/invoke";
import { useScan } from "@/lib/scan-store";
import type { ChartData, Tf } from "@/lib/types";

// Query-param routing (`/chart?symbol=AAPL`): Next.js static export cannot
// prerender an open-ended dynamic `[symbol]` route. useSearchParams needs Suspense.

const TFS: { key: Tf; label: string }[] = [
  { key: "daily", label: "日足" },
  { key: "weekly", label: "週足" },
  { key: "monthly", label: "月足" },
];

const TOGGLES: { key: keyof ChartVisibility; label: string }[] = [
  { key: "ema", label: "EMAリボン" },
  { key: "supertrend", label: "Supertrend" },
  { key: "ichimoku", label: "一目均衡表" },
  { key: "macd", label: "MACD" },
  { key: "squeeze", label: "Squeeze" },
  { key: "score", label: "スコア" },
  { key: "markers", label: "売買マーカー" },
  { key: "qtrend", label: "Q-Trend" },
  { key: "qtPrecursor", label: "QT前兆" },
  { key: "stFlip", label: "STフリップ" },
];

// Targets that must swallow the back shortcut: Backspace has to delete a
// character and ← has to move the caret. Checkboxes/buttons/links deliberately
// do NOT count — the user clicks a toggle, leaving it focused, and still
// expects ← to go back.
const TEXT_INPUT_TYPES = new Set([
  "text",
  "search",
  "url",
  "tel",
  "email",
  "password",
  "number",
  "date",
  "datetime-local",
  "month",
  "week",
  "time",
]);

function isTextEntry(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target instanceof HTMLTextAreaElement) return true;
  // The DOM lowercases `type` and reports an unknown/missing one as "text", so
  // this errs toward protecting text input rather than navigating away.
  if (target instanceof HTMLInputElement) return TEXT_INPUT_TYPES.has(target.type);
  return false;
}

function ChartInner() {
  const symbol = useSearchParams().get("symbol") ?? "";
  const [tf, setTf] = useState<Tf>("daily");
  // The fetch result is tagged with the request it answered, and `data`/`error`
  // are DERIVED from it. That way switching symbol/timeframe drops the stale
  // result during render (→ 読み込み中…) instead of needing a setState(null)
  // inside the effect, which would cascade an extra render.
  const [fetched, setFetched] = useState<{
    key: string;
    data: ChartData | null;
    error: string | null;
  }>({ key: "", data: null, error: null });
  const reqKey = `${symbol}|${tf}`;
  const data = fetched.key === reqKey ? fetched.data : null;
  const error = fetched.key === reqKey ? fetched.error : null;
  // Visibility lives in the cross-navigation store so the checkbox state is
  // preserved when returning to the radar and reopening a chart.
  const { chartVisibility: vis, setChartVisibility: setVis } = useScan();
  const router = useRouter();

  // ← / Backspace returns to the radar — the same destination as the back link.
  // `push` rather than `back()`: the chart is also reachable as a first history
  // entry (reload / direct URL), where `back()` would leave the app.
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key !== "ArrowLeft" && e.key !== "Backspace") return;
      // Bare keys only: Ctrl/Alt/Meta/Shift combos belong to the OS, the
      // webview (Alt+← = history back), or text selection.
      if (e.ctrlKey || e.altKey || e.metaKey || e.shiftKey) return;
      if (isTextEntry(e.target)) return;
      e.preventDefault(); // Backspace would otherwise trigger webview history-back.
      router.push("/");
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [router]);

  useEffect(() => {
    if (!symbol) return;
    let active = true;
    getChartData(symbol, tf)
      .then((d) => {
        if (active) setFetched({ key: reqKey, data: d, error: null });
      })
      .catch((e) => {
        if (active) setFetched({ key: reqKey, data: null, error: String(e) });
      });
    return () => {
      active = false;
    };
  }, [symbol, tf, reqKey]);

  return (
    <div className="chart-page">
      <Link href="/" className="chart-back" title="← / Backspace キーでも戻れます">
        ← レーダーへ戻る
      </Link>
      <div className="chart-toolbar">
        <h1 style={{ margin: 0, fontSize: "1.25rem" }}>{symbol || "（銘柄未指定）"}</h1>
        <span style={{ flex: 1 }} />
        {TFS.map((x) => (
          <button
            key={x.key}
            className={tf === x.key ? "active" : ""}
            onClick={() => setTf(x.key)}
          >
            {x.label}
          </button>
        ))}
      </div>

      <div className="chart-toggles">
        {TOGGLES.map((x) => (
          <label key={x.key}>
            <input
              type="checkbox"
              checked={vis[x.key]}
              onChange={() => setVis((v) => ({ ...v, [x.key]: !v[x.key] }))}
            />
            {x.label}
          </label>
        ))}
      </div>

      {error && <div className="status error">エラー: {error}</div>}
      {!data && !error && <div className="status">読み込み中…</div>}
      {data && (
        <>
          <MtfSummary rows={data.mtf_summary} />
          <MultiPaneChart data={data} visible={vis} />
        </>
      )}
    </div>
  );
}

export default function ChartPage() {
  return (
    <Suspense fallback={<div className="chart-page">読み込み中…</div>}>
      <ChartInner />
    </Suspense>
  );
}
