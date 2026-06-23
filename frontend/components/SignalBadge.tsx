import type { SignalState } from "@/lib/types";

// Freshness tier (docs/05): Triggered = strong, Primed = mid, Active = faint.
type Tier = "trig" | "prime" | "active" | "neutral";

function tier(state: SignalState): Tier {
  if (state === "TriggeredBuy" || state === "TriggeredSell") return "trig";
  if (state === "PrimedBuy" || state === "PrimedSell") return "prime";
  if (state === "ActiveBuy" || state === "ActiveSell") return "active";
  return "neutral";
}

function side(state: SignalState): "buy" | "sell" | "neutral" {
  if (state.endsWith("Buy")) return "buy";
  if (state.endsWith("Sell")) return "sell";
  return "neutral";
}

// Text label (color is not the only signal — accessibility, docs/05).
const LABEL: Record<Tier, string> = {
  trig: "発火",
  prime: "仕込",
  active: "出遅",
  neutral: "中立",
};

export function SignalBadge({ state }: { state: SignalState }) {
  const t = tier(state);
  return (
    <span className={`badge badge-${t} side-${side(state)}`} title={state}>
      {LABEL[t]}
    </span>
  );
}
