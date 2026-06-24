"use client";

import { Suspense, useEffect, useState } from "react";

import Link from "next/link";
import { useSearchParams } from "next/navigation";

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
  { key: "markers", label: "売買マーカー" },
];

function ChartInner() {
  const symbol = useSearchParams().get("symbol") ?? "";
  const [tf, setTf] = useState<Tf>("daily");
  const [data, setData] = useState<ChartData | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Visibility lives in the cross-navigation store so the checkbox state is
  // preserved when returning to the radar and reopening a chart.
  const { chartVisibility: vis, setChartVisibility: setVis } = useScan();

  useEffect(() => {
    if (!symbol) return;
    let active = true;
    setData(null);
    setError(null);
    getChartData(symbol, tf)
      .then((d) => active && setData(d))
      .catch((e) => active && setError(String(e)));
    return () => {
      active = false;
    };
  }, [symbol, tf]);

  return (
    <div className="chart-page">
      <Link href="/" className="chart-back">
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
