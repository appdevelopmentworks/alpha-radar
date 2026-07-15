"use client";

import { useEffect, useState } from "react";

import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";

function firstCsv(paths: string[]): string | undefined {
  return paths.find((p) => p.toLowerCase().endsWith(".csv"));
}

export function DropZone({
  onFile,
  disabled,
}: {
  onFile: (path: string) => void;
  disabled: boolean;
}) {
  const [hover, setHover] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === "enter" || p.type === "over") {
          setHover(true);
        } else if (p.type === "leave") {
          setHover(false);
        } else if (p.type === "drop") {
          setHover(false);
          if (disabled) return;
          const csv = firstCsv(p.paths);
          if (csv) onFile(csv);
        }
      })
      .then((u) => {
        unlisten = u;
      })
      .catch(() => {
        // Not running inside Tauri (e.g. plain `next dev` in a browser).
      });
    return () => unlisten?.();
  }, [onFile, disabled]);

  async function pick() {
    if (disabled) return;
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "CSV", extensions: ["csv"] }],
      });
      if (typeof selected === "string") onFile(selected);
    } catch {
      // dialog unavailable (non-Tauri) — ignore.
    }
  }

  return (
    <button
      type="button"
      className={`dropzone ${hover ? "hover" : ""} ${disabled ? "disabled" : ""}`}
      onClick={pick}
      disabled={disabled}
    >
      <span className="dz-icon" aria-hidden>
        ⤓
      </span>
      <span className="dz-title">ウォッチリスト CSV をドロップ</span>
      <span className="dz-sub">またはクリックして選択（先頭列＝コード / UTF-8・Shift_JIS対応）</span>
    </button>
  );
}
