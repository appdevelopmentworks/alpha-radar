"use client";

import { useEffect, useRef } from "react";

import type { UTCTimestamp } from "lightweight-charts";

import type { ChartData, HistBar, TimeValue } from "@/lib/types";

export interface ChartVisibility {
  ema: boolean;
  supertrend: boolean;
  ichimoku: boolean;
  macd: boolean;
  squeeze: boolean;
  markers: boolean;
}

export const DEFAULT_VISIBILITY: ChartVisibility = {
  ema: true,
  supertrend: true,
  ichimoku: false, // busiest overlay — hidden by default to declutter
  macd: true,
  squeeze: true,
  markers: true,
};

const t = (n: number) => n as UTCTimestamp;
const line = (s: TimeValue[]) => s.map((p) => ({ time: t(p.time), value: p.value }));
const hist = (s: HistBar[]) => s.map((p) => ({ time: t(p.time), value: p.value, color: p.color }));

export function MultiPaneChart({
  data,
  visible,
}: {
  data: ChartData;
  visible: ChartVisibility;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const host = ref.current;
    if (!host) return;
    let disposed = false;
    let chart: { remove: () => void } | undefined;

    (async () => {
      const lwc = await import("lightweight-charts");
      if (disposed || !ref.current) return;
      const {
        createChart,
        createSeriesMarkers,
        CandlestickSeries,
        LineSeries,
        HistogramSeries,
        ColorType,
        LineStyle,
      } = lwc;

      const c = createChart(host, {
        autoSize: true,
        layout: {
          background: { type: ColorType.Solid, color: "#11161f" },
          textColor: "#8a93a3",
          attributionLogo: true, // TradingView license requirement (docs/05)
          panes: { separatorColor: "#232b38", separatorHoverColor: "#2f3a4d" },
        },
        grid: { vertLines: { color: "#1a212d" }, horzLines: { color: "#1a212d" } },
        crosshair: { mode: 0 },
        timeScale: { borderColor: "#232b38", rightOffset: 4 },
        rightPriceScale: { borderColor: "#232b38" },
      });
      chart = c;

      const addLine = (pane: number, color: string, width: 1 | 2 = 1) =>
        c.addSeries(
          LineSeries,
          { color, lineWidth: width, priceLineVisible: false, lastValueVisible: false },
          pane,
        );

      // Pane 0 — price + (optional) overlays + (optional) markers
      const candle = c.addSeries(
        CandlestickSeries,
        {
          upColor: "#26a69a",
          downColor: "#ef5350",
          borderVisible: false,
          wickUpColor: "#26a69a",
          wickDownColor: "#ef5350",
        },
        0,
      );
      candle.setData(
        data.ohlc.map((k) => ({
          time: t(k.ts),
          open: k.open,
          high: k.high,
          low: k.low,
          close: k.close,
        })),
      );
      if (visible.ema) {
        addLine(0, "#5b8def").setData(line(data.ema20));
        addLine(0, "#e0a458").setData(line(data.ema50));
        addLine(0, "#a36bd8").setData(line(data.ema200));
      }
      if (visible.supertrend) {
        addLine(0, "#3fb950", 2).setData(line(data.supertrend));
      }
      if (visible.ichimoku) {
        addLine(0, "#56c0c0").setData(line(data.tenkan));
        addLine(0, "#d56b6b").setData(line(data.kijun));
        addLine(0, "#3a5a40").setData(line(data.senkou_a));
        addLine(0, "#5a3a3a").setData(line(data.senkou_b));
      }
      if (visible.markers) {
        createSeriesMarkers(
          candle,
          data.markers.map((m) => ({
            time: t(m.time),
            position: m.position,
            color: m.color,
            shape: m.shape,
            text: m.text,
          })),
        );
      }

      // Lower panes are allocated only for what's visible (no empty panes).
      let pane = 1;
      if (visible.macd) {
        const p = pane++;
        c.addSeries(HistogramSeries, { priceLineVisible: false, lastValueVisible: false }, p).setData(
          hist(data.macd_hist),
        );
        addLine(p, "#e6e8eb").setData(line(data.macd));
        addLine(p, "#f0a040").setData(line(data.macd_signal));
      }
      if (visible.squeeze) {
        const p = pane++;
        c.addSeries(HistogramSeries, { priceLineVisible: false, lastValueVisible: false }, p).setData(
          hist(data.sqz_val),
        );
      }
      // Score pane (the headline signal) is always shown.
      const scorePane = pane;
      const score = addLine(scorePane, "#e6e8eb", 2);
      score.setData(line(data.score));
      score.createPriceLine({
        price: data.buy_threshold,
        color: "#26a69a",
        lineStyle: LineStyle.Dashed,
        lineWidth: 1,
        axisLabelVisible: true,
        title: "buy",
      });
      score.createPriceLine({
        price: data.sell_threshold,
        color: "#ef5350",
        lineStyle: LineStyle.Dashed,
        lineWidth: 1,
        axisLabelVisible: true,
        title: "sell",
      });

      c.panes().forEach((p, i) => p.setHeight(i === 0 ? 340 : 120));
      c.timeScale().fitContent();
    })();

    return () => {
      disposed = true;
      chart?.remove();
    };
  }, [data, visible]);

  return <div ref={ref} className="chart-host" />;
}
