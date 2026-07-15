"use client";

import { useMemo } from "react";

import { useRouter } from "next/navigation";

import { ProximityBar } from "@/components/ProximityBar";
import { ScoreCell, scoreColor } from "@/components/ScoreCell";
import { SignalBadge } from "@/components/SignalBadge";
import type { Sort, SortField } from "@/lib/radar-view";
import { useScan } from "@/lib/scan-store";
import type { AssetClass, MarkerKind, Regime, SignalState, SymbolScore } from "@/lib/types";

const COLUMNS: { key: SortField | null; label: string }[] = [
  { key: "symbol", label: "銘柄 / 名称" },
  { key: "signal_state", label: "状態" },
  { key: "proximity_score", label: "近接度" },
  { key: "score_final", label: "方向スコア" },
  { key: "marker_hit_rate", label: "的中率" },
  { key: "last_marker_bars", label: "直近マーカー" },
  { key: "regime", label: "レジーム" },
  { key: null, label: "内訳" },
  { key: "bars_since_trigger", label: "経過" },
  { key: "atr", label: "ATR" },
  { key: "suggested_stop", label: "損切り" },
];

const REGIME_LABEL: Record<Regime, string> = {
  TrendUp: "上昇",
  TrendDown: "下降",
  Range: "レンジ",
  Transition: "転換",
};

// Sort ranks: fresher / stronger states and trends sort higher.
const STATE_RANK: Record<SignalState, number> = {
  TriggeredBuy: 3,
  TriggeredSell: 3,
  PrimedBuy: 2,
  PrimedSell: 2,
  ActiveBuy: 1,
  ActiveSell: 1,
  Neutral: 0,
};
const REGIME_RANK: Record<Regime, number> = {
  TrendUp: 3,
  TrendDown: 2,
  Range: 1,
  Transition: 0,
};

function sortValue(s: SymbolScore, field: SortField): number | string | null {
  switch (field) {
    case "actionability":
      return s.actionability;
    case "symbol":
      return s.symbol;
    case "signal_state":
      return STATE_RANK[s.signal_state];
    case "regime":
      return s.regime ? REGIME_RANK[s.regime] : null;
    case "proximity_score":
      return s.proximity_score;
    case "score_final":
      return s.score_final;
    case "marker_hit_rate":
      return s.marker_hit_rate;
    case "last_marker_bars":
      return s.last_marker ? s.last_marker.bars_ago : null;
    case "bars_since_trigger":
      return s.bars_since_trigger;
    case "atr":
      return s.atr;
    case "suggested_stop":
      return s.suggested_stop;
  }
}

function makeComparator({ field, dir }: Sort) {
  const mul = dir === "asc" ? 1 : -1;
  return (a: SymbolScore, b: SymbolScore) => {
    const va = sortValue(a, field);
    const vb = sortValue(b, field);
    if (va == null && vb == null) return 0;
    if (va == null) return 1; // nulls always last
    if (vb == null) return -1;
    if (typeof va === "string" && typeof vb === "string") return mul * va.localeCompare(vb);
    return mul * ((va as number) - (vb as number));
  };
}

function fmt(n: number | null, digits = 2): string {
  return n == null ? "—" : n.toFixed(digits);
}

// Marker hit rate as "56% (42)"; sample count shown so a thin history (few
// supertrend legs) is visibly less trustworthy than a deep one.
function HitRateCell({ s }: { s: SymbolScore }) {
  if (s.marker_hit_rate == null) return <td className="num">—</td>;
  const pct = s.marker_hit_rate * 100;
  return (
    <td
      className="num"
      title={`マーカー ${s.marker_samples} 回のうち ${Math.round((pct / 100) * s.marker_samples)} 回順行（設定のバー数後の終値で判定）`}
    >
      {pct.toFixed(0)}%
      <span className="hit-n"> ({s.marker_samples})</span>
    </td>
  );
}

// 直近マーカー badge: kind × direction colors match the chart's marker layers.
const MARKER_BADGE: Record<MarkerKind, { label: string; buy: string; sell: string }> = {
  confluence: { label: "確定", buy: "#26a69a", sell: "#ef5350" },
  qt_flip: { label: "QT", buy: "#2196f3", sell: "#ff9800" },
  qt_precursor: { label: "前兆", buy: "#90caf9", sell: "#ffcc80" },
};

function LastMarkerCell({ s }: { s: SymbolScore }) {
  const m = s.last_marker;
  if (!m) return <td className="num">—</td>;
  const b = MARKER_BADGE[m.kind];
  const color = m.dir > 0 ? b.buy : b.sell;
  const when = m.bars_ago === 0 ? "本日" : `${m.bars_ago}日前`;
  return (
    <td title="経過は営業日ベース（日足バー数）">
      <span className="marker-badge" style={{ color, borderColor: color }}>
        {b.label}
        {m.dir > 0 ? "買" : "売"} {when}
      </span>
    </td>
  );
}

function relTime(unixSec: number): string {
  const diff = Math.max(0, Math.floor(Date.now() / 1000 - unixSec));
  if (diff < 60) return `${diff}秒前`;
  if (diff < 3600) return `${Math.floor(diff / 60)}分前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}時間前`;
  return `${Math.floor(diff / 86400)}日前`;
}

function MiniBreakdown({ s }: { s: SymbolScore }) {
  const cats: [string, number | null][] = [
    ["トレンド", s.s_trend],
    ["モメンタム", s.s_momentum],
    ["逆張り", s.s_mean_reversion],
  ];
  return (
    <div className="breakdown" title={cats.map(([k, v]) => `${k}: ${fmt(v)}`).join("\n")}>
      {cats.map(([k, v]) => (
        <div key={k} className="bd-track">
          <div
            className="bd-fill"
            style={{
              width: `${Math.min(100, Math.abs(v ?? 0) * 100)}%`,
              background: scoreColor((v ?? 0) * 100),
            }}
          />
        </div>
      ))}
    </div>
  );
}

function Row({ s, onOpen }: { s: SymbolScore; onOpen: (symbol: string) => void }) {
  return (
    <tr onClick={() => onOpen(s.symbol)} className="row">
      <td className="cell-sym">
        <span className="sym">{s.symbol}</span>
        {s.name && <span className="name">{s.name}</span>}
      </td>
      <td>
        <SignalBadge state={s.signal_state} />
      </td>
      <td className="cell-prox">
        <ProximityBar value={s.proximity_score} direction={s.direction} />
      </td>
      <td className="cell-score">
        <ScoreCell value={s.score_final} />
      </td>
      <HitRateCell s={s} />
      <LastMarkerCell s={s} />
      <td className="cell-regime">{s.regime ? REGIME_LABEL[s.regime] : "—"}</td>
      <td>
        <MiniBreakdown s={s} />
      </td>
      <td className="num">{s.bars_since_trigger ?? "—"}</td>
      <td className="num">{fmt(s.atr)}</td>
      <td className="num">{fmt(s.suggested_stop)}</td>
    </tr>
  );
}

function HeaderRow({ sort, onSort }: { sort: Sort; onSort: (f: SortField) => void }) {
  return (
    <thead>
      <tr>
        {COLUMNS.map((col) =>
          col.key ? (
            <th
              key={col.label}
              className={`sortable ${sort.field === col.key ? "active" : ""}`}
              aria-sort={
                sort.field === col.key
                  ? sort.dir === "asc"
                    ? "ascending"
                    : "descending"
                  : "none"
              }
              onClick={() => onSort(col.key!)}
            >
              {col.label}
              <span className="sort-arrow">
                {sort.field === col.key ? (sort.dir === "asc" ? " ▲" : " ▼") : ""}
              </span>
            </th>
          ) : (
            <th key={col.label}>{col.label}</th>
          ),
        )}
      </tr>
    </thead>
  );
}

function Grid({
  rows,
  sort,
  onSort,
  onOpen,
}: {
  rows: SymbolScore[];
  sort: Sort;
  onSort: (f: SortField) => void;
  onOpen: (symbol: string) => void;
}) {
  return (
    <table className="ranking">
      <HeaderRow sort={sort} onSort={onSort} />
      <tbody>
        {rows.map((s) => (
          <Row key={s.symbol} s={s} onOpen={onOpen} />
        ))}
      </tbody>
    </table>
  );
}

export function RankingTable({
  scores,
  scannedAt,
}: {
  scores: SymbolScore[];
  scannedAt: number;
}) {
  const router = useRouter();
  // View state (sort + filters) lives in the cross-navigation store so it is
  // preserved when returning to the radar from the chart.
  const { radarView, setRadarView } = useScan();
  const { sort, asset, query, showNeutral } = radarView;

  const setAsset = (asset: AssetClass | "all") => setRadarView((v) => ({ ...v, asset }));
  const setQuery = (query: string) => setRadarView((v) => ({ ...v, query }));
  const toggleNeutral = () => setRadarView((v) => ({ ...v, showNeutral: !v.showNeutral }));

  const open = (symbol: string) =>
    router.push(`/chart?symbol=${encodeURIComponent(symbol)}`);

  // Click a header: toggle direction if it's the active field, else select it
  // (numbers default to descending; the symbol name and marker recency —
  // most-recent-first — to ascending).
  const onSort = (field: SortField) =>
    setRadarView((v) => ({
      ...v,
      sort:
        v.sort.field === field
          ? { field, dir: v.sort.dir === "asc" ? "desc" : "asc" }
          : {
              field,
              dir: field === "symbol" || field === "last_marker_bars" ? "asc" : "desc",
            },
    }));

  const { buys, sells, neutrals } = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = scores.filter(
      (s) =>
        (asset === "all" || s.asset_class === asset) &&
        (q === "" ||
          s.symbol.toLowerCase().includes(q) ||
          (s.name ?? "").toLowerCase().includes(q)),
    );
    const cmp = makeComparator(sort);
    return {
      buys: filtered.filter((s) => s.direction === "Buy").sort(cmp),
      sells: filtered.filter((s) => s.direction === "Sell").sort(cmp),
      neutrals: filtered.filter((s) => s.direction === "None").sort(cmp),
    };
  }, [scores, asset, query, sort]);

  return (
    <div className="ranking-wrap">
      <div className="toolbar">
        <input
          className="search"
          placeholder="銘柄 / 名称で絞り込み"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <select value={asset} onChange={(e) => setAsset(e.target.value as AssetClass | "all")}>
          <option value="all">全資産</option>
          <option value="equity">株式</option>
          <option value="crypto">暗号資産</option>
        </select>
        <span className="sort-hint">列ヘッダーをクリックで並べ替え</span>
        <span className="scanned-at">更新 {relTime(scannedAt)}</span>
      </div>

      {buys.length > 0 && (
        <section className="block side-buy">
          <h2 className="block-title">
            買い接近 <span className="count">{buys.length}</span>
          </h2>
          <Grid rows={buys} sort={sort} onSort={onSort} onOpen={open} />
        </section>
      )}
      {sells.length > 0 && (
        <section className="block side-sell">
          <h2 className="block-title">
            売り接近 <span className="count">{sells.length}</span>
          </h2>
          <Grid rows={sells} sort={sort} onSort={onSort} onOpen={open} />
        </section>
      )}

      {neutrals.length > 0 && (
        <section className="block neutral">
          <button className="neutral-toggle" onClick={toggleNeutral}>
            {showNeutral ? "▼" : "▶"} 中立 <span className="count">{neutrals.length}</span>
          </button>
          {showNeutral && <Grid rows={neutrals} sort={sort} onSort={onSort} onOpen={open} />}
        </section>
      )}

      {buys.length === 0 && sells.length === 0 && neutrals.length === 0 && (
        <p className="empty">該当する銘柄がありません。</p>
      )}
    </div>
  );
}
