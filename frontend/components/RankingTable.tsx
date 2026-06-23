"use client";

import { useMemo, useState } from "react";

import { useRouter } from "next/navigation";

import { ProximityBar } from "@/components/ProximityBar";
import { ScoreCell, scoreColor } from "@/components/ScoreCell";
import { SignalBadge } from "@/components/SignalBadge";
import type { AssetClass, Regime, SymbolScore } from "@/lib/types";

type SortKey = "actionability" | "proximity_score" | "score_final";

const REGIME_LABEL: Record<Regime, string> = {
  TrendUp: "上昇",
  TrendDown: "下降",
  Range: "レンジ",
  Transition: "転換",
};

function fmt(n: number | null, digits = 2): string {
  return n == null ? "—" : n.toFixed(digits);
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

function Block({
  title,
  side,
  rows,
  onOpen,
}: {
  title: string;
  side: "buy" | "sell";
  rows: SymbolScore[];
  onOpen: (symbol: string) => void;
}) {
  if (rows.length === 0) return null;
  return (
    <section className={`block side-${side}`}>
      <h2 className="block-title">
        {title} <span className="count">{rows.length}</span>
      </h2>
      <table className="ranking">
        <thead>
          <tr>
            <th>銘柄 / 名称</th>
            <th>状態</th>
            <th>近接度</th>
            <th>方向スコア</th>
            <th>レジーム</th>
            <th>内訳</th>
            <th>経過</th>
            <th>ATR</th>
            <th>損切り</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((s) => (
            <Row key={s.symbol} s={s} onOpen={onOpen} />
          ))}
        </tbody>
      </table>
    </section>
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
  const [sortKey, setSortKey] = useState<SortKey>("actionability");
  const [asset, setAsset] = useState<AssetClass | "all">("all");
  const [query, setQuery] = useState("");
  const [showNeutral, setShowNeutral] = useState(false);

  const open = (symbol: string) =>
    router.push(`/chart?symbol=${encodeURIComponent(symbol)}`);

  const { buys, sells, neutrals } = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = scores.filter(
      (s) =>
        (asset === "all" || s.asset_class === asset) &&
        (q === "" ||
          s.symbol.toLowerCase().includes(q) ||
          (s.name ?? "").toLowerCase().includes(q)),
    );
    const by = (a: SymbolScore, b: SymbolScore) =>
      (b[sortKey] ?? -Infinity) - (a[sortKey] ?? -Infinity);
    return {
      buys: filtered.filter((s) => s.direction === "Buy").sort(by),
      sells: filtered.filter((s) => s.direction === "Sell").sort(by),
      neutrals: filtered.filter((s) => s.direction === "None").sort(by),
    };
  }, [scores, asset, query, sortKey]);

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
        <select value={sortKey} onChange={(e) => setSortKey(e.target.value as SortKey)}>
          <option value="actionability">アクション度順</option>
          <option value="proximity_score">近接度順</option>
          <option value="score_final">方向スコア順</option>
        </select>
        <span className="scanned-at">更新 {relTime(scannedAt)}</span>
      </div>

      <Block title="買い接近" side="buy" rows={buys} onOpen={open} />
      <Block title="売り接近" side="sell" rows={sells} onOpen={open} />

      {neutrals.length > 0 && (
        <section className="block neutral">
          <button className="neutral-toggle" onClick={() => setShowNeutral((v) => !v)}>
            {showNeutral ? "▼" : "▶"} 中立 <span className="count">{neutrals.length}</span>
          </button>
          {showNeutral && (
            <table className="ranking">
              <tbody>
                {neutrals.map((s) => (
                  <Row key={s.symbol} s={s} onOpen={open} />
                ))}
              </tbody>
            </table>
          )}
        </section>
      )}

      {buys.length === 0 && sells.length === 0 && neutrals.length === 0 && (
        <p className="empty">該当する銘柄がありません。</p>
      )}
    </div>
  );
}
