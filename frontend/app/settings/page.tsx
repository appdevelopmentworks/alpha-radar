"use client";

import { useEffect, useState } from "react";

import Link from "next/link";

import { getConfig, getPresets, updateConfig } from "@/lib/invoke";
import type { ScanConfig } from "@/lib/types";

const PRESET_LABEL: Record<string, string> = {
  conservative: "保守",
  standard: "標準",
  aggressive: "積極",
};

function Num({
  label,
  value,
  onChange,
  min,
  max,
  step,
  slider,
}: {
  label: string;
  value: number;
  onChange: (v: number) => void;
  min: number;
  max: number;
  step: number;
  slider?: boolean;
}) {
  const set = (s: string) => {
    const n = parseFloat(s);
    if (!Number.isNaN(n)) onChange(n);
  };
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      <span className="field-input">
        {slider && (
          <input
            type="range"
            min={min}
            max={max}
            step={step}
            value={value}
            onChange={(e) => set(e.target.value)}
          />
        )}
        <input
          type="number"
          min={min}
          max={max}
          step={step}
          value={value}
          onChange={(e) => set(e.target.value)}
        />
      </span>
    </label>
  );
}

export default function SettingsPage() {
  const [cfg, setCfg] = useState<ScanConfig | null>(null);
  const [presets, setPresets] = useState<[string, ScanConfig][]>([]);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getConfig().then(setCfg).catch((e) => setError(String(e)));
    getPresets().then(setPresets).catch(() => {});
  }, []);

  const back = (
    <Link href="/" className="chart-back">
      ← レーダーへ戻る
    </Link>
  );

  if (error)
    return (
      <div className="settings-page">
        {back}
        <div className="status error">エラー: {error}</div>
      </div>
    );
  if (!cfg)
    return (
      <div className="settings-page">
        {back}
        <div className="status">読み込み中…</div>
      </div>
    );

  const patch = (p: Partial<ScanConfig>) => {
    setCfg({ ...cfg, ...p });
    setSaved(false);
  };
  const patchMtf = (p: Partial<ScanConfig["mtf"]>) => patch({ mtf: { ...cfg.mtf, ...p } });
  const patchProx = (p: Partial<ScanConfig["proximity"]>) =>
    patch({ proximity: { ...cfg.proximity, ...p } });
  const setAlpha = (i: number, v: number) => {
    const alpha = [...cfg.mtf.alpha];
    alpha[i] = v;
    patchMtf({ alpha });
  };

  async function save() {
    try {
      await updateConfig(cfg!);
      setSaved(true);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="settings-page">
      {back}
      <h1>設定</h1>

      <section className="settings-section">
        <h2>プリセット</h2>
        <div className="preset-row">
          {presets.map(([label, pc]) => (
            <button
              key={label}
              onClick={() => {
                setCfg(pc);
                setSaved(false);
              }}
            >
              {PRESET_LABEL[label] ?? label}
            </button>
          ))}
        </div>
        <p className="hint">プリセットを適用してから個別に微調整できます。</p>
      </section>

      <section className="settings-section">
        <h2>しきい値</h2>
        <Num label="買いしきい値" value={cfg.buy_threshold} onChange={(v) => patch({ buy_threshold: v })} min={10} max={90} step={1} slider />
        <Num label="売りしきい値" value={cfg.sell_threshold} onChange={(v) => patch({ sell_threshold: v })} min={-90} max={-10} step={1} slider />
        <Num label="スクイーズ品質ゲート" value={cfg.squeeze_gate} onChange={(v) => patch({ squeeze_gate: v })} min={0} max={1} step={0.05} slider />
      </section>

      <section className="settings-section">
        <h2>MTF（上位足）</h2>
        <Num label="α 日足" value={cfg.mtf.alpha[0]} onChange={(v) => setAlpha(0, v)} min={0} max={1} step={0.05} />
        <Num label="α 週足" value={cfg.mtf.alpha[1]} onChange={(v) => setAlpha(1, v)} min={0} max={1} step={0.05} />
        <Num label="α 月足" value={cfg.mtf.alpha[2]} onChange={(v) => setAlpha(2, v)} min={0} max={1} step={0.05} />
        <Num label="週足ゲート（整合）" value={cfg.mtf.weekly_gate_aligned} onChange={(v) => patchMtf({ weekly_gate_aligned: v })} min={0} max={1.5} step={0.05} />
        <Num label="週足ゲート（中立）" value={cfg.mtf.weekly_gate_neutral} onChange={(v) => patchMtf({ weekly_gate_neutral: v })} min={0} max={1.5} step={0.05} />
        <Num label="週足ゲート（逆行）" value={cfg.mtf.weekly_gate_opposed} onChange={(v) => patchMtf({ weekly_gate_opposed: v })} min={0} max={1.5} step={0.05} />
        <Num label="月足修正子（整合）" value={cfg.mtf.monthly_mod_aligned} onChange={(v) => patchMtf({ monthly_mod_aligned: v })} min={0.5} max={1.5} step={0.05} />
        <Num label="月足修正子（逆行）" value={cfg.mtf.monthly_mod_opposed} onChange={(v) => patchMtf({ monthly_mod_opposed: v })} min={0.5} max={1.5} step={0.05} />
        <label className="field toggle">
          <span className="field-label">月足修正子を有効化</span>
          <input
            type="checkbox"
            checked={cfg.mtf.monthly_enabled}
            onChange={(e) => patchMtf({ monthly_enabled: e.target.checked })}
          />
        </label>
      </section>

      <section className="settings-section">
        <h2>近接度</h2>
        <Num label="新鮮バー数 (fresh_bars_n)" value={cfg.proximity.fresh_bars_n} onChange={(v) => patchProx({ fresh_bars_n: Math.round(v) })} min={0} max={5} step={1} slider />
        <Num label="接近開始 (approach_floor)" value={cfg.proximity.approach_floor} onChange={(v) => patchProx({ approach_floor: v })} min={-50} max={50} step={1} />
        <Num label="押し目距離 ATR (max_dist_atr)" value={cfg.proximity.pull_max_dist_atr} onChange={(v) => patchProx({ pull_max_dist_atr: v })} min={0.5} max={5} step={0.1} slider />
      </section>

      <section className="settings-section">
        <h2>リスク / 対象</h2>
        <Num label="最小バー数" value={cfg.min_bars} onChange={(v) => patch({ min_bars: Math.round(v) })} min={20} max={250} step={5} />
        <Num label="損切り ATR 倍率" value={cfg.stop_atr_mult} onChange={(v) => patch({ stop_atr_mult: v })} min={0.5} max={5} step={0.1} slider />
      </section>

      <div className="settings-actions">
        <button className="scan-btn" onClick={save}>
          保存
        </button>
        {saved && <span className="saved-msg">保存しました（次回スキャン／チャートから適用）</span>}
      </div>
    </div>
  );
}
