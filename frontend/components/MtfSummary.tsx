import type { Regime, TfSummary } from "@/lib/types";

const TF_LABEL: Record<string, string> = {
  "1d": "日足",
  "1wk": "週足",
  "1mo": "月足",
};

const REGIME_LABEL: Record<Regime, string> = {
  TrendUp: "上昇",
  TrendDown: "下降",
  Range: "レンジ",
  Transition: "転換",
};

const VELOCITY_LABEL: Record<string, string> = {
  Accelerating: "加速",
  Decelerating: "減速",
  Flat: "横ばい",
};

export function MtfSummary({ rows }: { rows: TfSummary[] }) {
  return (
    <table className="mtf-summary">
      <thead>
        <tr>
          <th>足</th>
          <th>レジーム</th>
          <th>勢い</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => (
          <tr key={r.tf}>
            <td>{TF_LABEL[r.tf] ?? r.tf}</td>
            <td>{r.regime ? REGIME_LABEL[r.regime] : "—"}</td>
            <td>{VELOCITY_LABEL[r.velocity] ?? r.velocity}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
