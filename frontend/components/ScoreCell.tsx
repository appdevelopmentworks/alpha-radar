// Direction score −100..+100 → red / gray / green continuous scale (docs/05).

function mix(a: number[], b: number[], t: number): string {
  const c = a.map((v, i) => Math.round(v + (b[i] - v) * t));
  return `rgb(${c[0]}, ${c[1]}, ${c[2]})`;
}

const GRAY = [140, 140, 150];
const RED = [240, 86, 86];
const GREEN = [70, 200, 120];

export function scoreColor(value: number): string {
  const t = Math.max(-100, Math.min(100, value)) / 100;
  return t >= 0 ? mix(GRAY, GREEN, t) : mix(GRAY, RED, -t);
}

export function ScoreCell({ value }: { value: number | null }) {
  if (value == null) return <span className="num score-na">—</span>;
  return (
    <span className="num score" style={{ color: scoreColor(value) }}>
      {value > 0 ? "+" : ""}
      {Math.round(value)}
    </span>
  );
}
