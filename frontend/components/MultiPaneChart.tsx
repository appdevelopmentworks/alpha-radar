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

// Target pixel height of a visible sub-pane (MACD / Squeeze / score); the
// price pane absorbs the remaining host height (window-fill layout).
const SUB_PANE_PX = 130;
const MIN_PRICE_PX = 240;
const TIME_AXIS_PX = 28; // approximation — affects px accuracy only, not ratios

/// Collapse/expand panes to fit the toggles. Uses stretch factors, NEVER
/// `setHeight`: lightweight-charts clamps `setHeight` to a 30px minimum and
/// redistributes the delta across sibling panes (re-inflating collapsed ones),
/// while a stretch factor of 0 collapses a pane to a 2px sliver. Factors are
/// fed in pixels so visible sub-panes stay ~fixed and the price pane takes all
/// resize delta.
function applyPaneLayout(chart: Refs, v: ChartVisibility, host: HTMLElement) {
  const panes = chart.panes();
  if (panes.length < 4 || host.clientHeight === 0) return;
  const subs = [v.macd, v.squeeze, v.score]; // panes 1 / 2 / 3
  const total = Math.max(0, host.clientHeight - TIME_AXIS_PX);
  const visibleSubs = subs.filter(Boolean).length;
  const pricePx = Math.max(MIN_PRICE_PX, total - visibleSubs * SUB_PANE_PX);
  panes[0].setStretchFactor(pricePx);
  subs.forEach((on, i) => panes[i + 1].setStretchFactor(on ? SUB_PANE_PX : 0));
}

function applyVisibility(s: Refs, v: ChartVisibility, data: ChartData) {
  s.ema.forEach((x: Refs) => x.applyOptions({ visible: v.ema }));
  s.supertrend.applyOptions({ visible: v.supertrend });
  s.ichimoku.forEach((x: Refs) => x.applyOptions({ visible: v.ichimoku }));
  s.macd.forEach((x: Refs) => x.applyOptions({ visible: v.macd }));
  s.squeeze.applyOptions({ visible: v.squeeze });
  // Hiding the score series also hides its buy/sell createPriceLine dashes and
  // axis labels (price lines render only while their series is visible).
  s.score.applyOptions({ visible: v.score });
  s.qtrend.applyOptions({ visible: v.qtrend });
  // One markers plugin, four toggleable sources (ADR-14 confluence markers,
  // Q-Trend flips, Q-Trend precursors, Supertrend flips); lightweight-charts
  // requires the merged array sorted ascending by time.
  const merged = [
    ...(v.markers ? data.markers : []),
    ...(v.qtrend ? data.qt_markers : []),
    ...(v.qtPrecursor ? data.qt_precursors : []),
    ...(v.stFlip ? data.st_markers : []),
  ]
    .map((m) => ({
      time: t(m.time),
      position: m.position,
      color: m.color,
      shape: m.shape,
      text: m.text,
    }))
    .sort((a, b) => (a.time as number) - (b.time as number));
  s.markers.setMarkers(merged);
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
  // Latest visibility, read by the (async) chart-creation effect for its initial
  // state and by the long-lived ResizeObserver — without making `visible` a
  // dependency of either (which would rebuild the chart). Synced in an effect,
  // not during render: refs must not be written while rendering.
  const visibleRef = useRef(visible);
  useEffect(() => {
    visibleRef.current = visible;
  });

  // Create the chart + all series once per `data`. Toggling checkboxes does NOT
  // run this effect, so it never rebuilds/resizes the chart.
  useEffect(() => {
    const host = ref.current;
    if (!host) return;
    let disposed = false;
    let chart: { remove: () => void } | undefined;
    let cleanupResize: (() => void) | undefined;

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
          panes: {
            separatorColor: "#232b38",
            separatorHoverColor: "#2f3a4d",
            // Separator dragging would silently overwrite the stretch factors
            // applyPaneLayout manages.
            enableResize: false,
          },
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
      // Q-Trend ratcheting trend line (ADR-15) — blue to match its QT markers.
      const qtrendLine = addLine(0, "#2196f3", 2);
      qtrendLine.setData(line(data.qtrend));
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

      // Pane 1 — MACD ; Pane 2 — Squeeze ; Pane 3 — score. The pane STRUCTURE
      // is fixed (no series rebuild on toggle); heights are managed by
      // applyPaneLayout, which collapses hidden panes via stretch factors.
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

      applyPaneLayout(c, visibleRef.current, host);
      // Re-fit pane heights when the host box changes (window resize / flex
      // reflow). autoSize handles the chart canvas itself; this keeps the
      // sub-panes at ~fixed px with the price pane absorbing the delta.
      const ro = new ResizeObserver(() => {
        if (!disposed) applyPaneLayout(c, visibleRef.current, host);
      });
      ro.observe(host);
      cleanupResize = () => ro.disconnect();
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
        chart: c,
        host,
        ema,
        supertrend,
        qtrend: qtrendLine,
        ichimoku,
        macd: [macdHist, macdLine, macdSignal],
        squeeze,
        score,
        markers,
      };
      applyVisibility(seriesRef.current, visibleRef.current, data);
    })();

    return () => {
      disposed = true;
      seriesRef.current = null;
      cleanupResize?.();
      chart?.remove();
    };
  }, [data]);

  // Toggle series visibility + pane heights only — no chart rebuild, zoom is
  // preserved (stretch factors change, pane structure does not).
  useEffect(() => {
    const s = seriesRef.current;
    if (!s) return;
    applyVisibility(s, visible, data);
    applyPaneLayout(s.chart, visible, s.host);
  }, [visible, data]);

  return <div ref={ref} className="chart-host" />;
}
