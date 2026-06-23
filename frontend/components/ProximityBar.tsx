import type { Direction } from "@/lib/types";

export function ProximityBar({ value, direction }: { value: number; direction: Direction }) {
  const pct = Math.max(0, Math.min(100, value));
  return (
    <div className="pbar" title={`近接度 ${Math.round(value)}/100`}>
      <div className={`pbar-fill dir-${direction}`} style={{ width: `${pct}%` }} />
      <span className="pbar-num">{Math.round(value)}</span>
    </div>
  );
}
