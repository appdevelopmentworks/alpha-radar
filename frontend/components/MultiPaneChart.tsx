"use client";

import { useEffect, useRef } from "react";

import type { UTCTimestamp } from "lightweight-charts";

import type { ChartVisibility } from "@/lib/chart-visibility";
import type { ChartData, HistBar, TimeValue } from "@/lib/types";

const t = (n: number) => n as UTCTimestamp;
const line = (s: TimeValue[]) => s.map((p) => ({ time: t(p.time), value: p.value }));
const hist = (s: HistBar[]) => s.map((p) => ({ time: t(p.time), value: p.value, color: p.color }));

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Refs = any;

function applyVisibility(s: Refs, v: ChartVisibility, data: ChartData) {
  s.ema.forEach((x: Refs) => x.applyOptions({ visible: v.ema }));
  s.supertrend.applyOptions({ visible: v.supertrend });
  s.ichimoku.forEach((x: Refs) => x.applyOptions({ visible: v.ichimoku }));
  s.macd.forEach((x: Refs) => x.applyOptions({ visible: v.macd }));
  s.squeeze.applyOptions({ visible: v.squeeze });
  s.markers.setMarkers(
    v.markers
      ? data.markers.map((m) => ({
          time: t(m.time),
          position: m.position,
          color: m.color,
          shape: m.shape,
          text: m.text,
        }))
      : [],
  );
}

export function MultiPaneChart({
  data,
  visible,
}: {
  data: ChartData;
  visible: ChartVisibility;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const seriesRef = useRef<Refs>(null);
  // Latest visibility, read by the (async) chart-creation effect for its
  // initial state without making `visible` a dependency (which would rebuild).
  const visibleRef = useRef(visible);
  visibleRef.current = visible;

  // Create the chart + all series once per `data`. Toggling checkboxes does NOT
  // run this effect, so it never rebuilds/resizes the chart.
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

      // Pane 0 — price + overlays + markers
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
      const ema = [
        addLine(0, "#5b8def"),
        addLine(0, "#e0a458"),
        addLine(0, "#a36bd8"),
      ];
      ema[0].setData(line(data.ema20));
      ema[1].setData(line(data.ema50));
      ema[2].setData(line(data.ema200));
      const supertrend = addLine(0, "#3fb950", 2);
      supertrend.setData(line(data.supertrend));
      const ichimoku = [
        addLine(0, "#56c0c0"),
        addLine(0, "#d56b6b"),
        addLine(0, "#3a5a40"),
        addLine(0, "#5a3a3a"),
      ];
      ichimoku[0].setData(line(data.tenkan));
      ichimoku[1].setData(line(data.kijun));
      ichimoku[2].setData(line(data.senkou_a));
      ichimoku[3].setData(line(data.senkou_b));
      const markers = createSeriesMarkers(candle, []);

      // Pane 1 — MACD ; Pane 2 — Squeeze ; Pane 3 — score (fixed layout so
      // toggling never changes the pane structure / triggers a resize).
      const macdHist = c.addSeries(
        HistogramSeries,
        { priceLineVisible: false, lastValueVisible: false },
        1,
      );
      macdHist.setData(hist(data.macd_hist));
      const macdLine = addLine(1, "#e6e8eb");
      macdLine.setData(line(data.macd));
      const macdSignal = addLine(1, "#f0a040");
      macdSignal.setData(line(data.macd_signal));

      const squeeze = c.addSeries(
        HistogramSeries,
        { priceLineVisible: false, lastValueVisible: false },
        2,
      );
      squeeze.setData(hist(data.sqz_val));

      const score = addLine(3, "#e6e8eb", 2);
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
      // Initial zoom: show only the most recent `initial_bars` candles (a swing
      // view fits ~100), keeping the chart's right offset as breathing room.
      // Fall back to fitting everything when there are fewer bars than that.
      const bars = data.ohlc.length;
      const n = data.initial_bars;
      const ts = c.timeScale();
      if (n > 0 && bars > n) {
        // `to` runs a few bars past the last index to keep the same right-edge
        // whitespace as `rightOffset` (the newest candle isn't flush-right).
        ts.setVisibleLogicalRange({ from: bars - n, to: bars - 1 + 4 });
      } else {
        ts.fitContent();
      }

      seriesRef.current = {
        ema,
        supertrend,
        ichimoku,
        macd: [macdHist, macdLine, macdSignal],
        squeeze,
        markers,
      };
      applyVisibility(seriesRef.current, visibleRef.current, data);
    })();

    return () => {
      disposed = true;
      seriesRef.current = null;
      chart?.remove();
    };
  }, [data]);

  // Toggle series visibility only — no chart rebuild, no resize.
  useEffect(() => {
    if (seriesRef.current) applyVisibility(seriesRef.current, visible, data);
  }, [visible, data]);

  return <div ref={ref} className="chart-host" />;
}
