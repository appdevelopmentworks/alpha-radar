# CLAUDE.md

Operating instructions for Claude Code working on **Alpha Radar**. Read this first, every session.

## What this project is

A Tauri 2 desktop app that scans a CSV watchlist and ranks tickers by how **close they are to a swing-trade entry** (an "entry-imminence radar"), plus a multi-pane chart for drill-down. Target user: a swing trader who wants to see, at a glance, which names are about to trigger a buy or sell.

The full spec is in `docs/`. **`docs/00-requirements.md`** is the master spec. This file is the short operating contract.

## Core principles (do not violate)

1. **Single source of truth for computation = Rust.** All indicator, regime, score, and proximity math lives in Rust pure functions. The frontend and the chart NEVER recompute indicators — they receive Rust-computed series. This guarantees the chart and the score always agree.
2. **Regime gating happens BEFORE aggregation.** Mean-reversion and trend signals are opposite by nature; summing them naively self-cancels. Always: detect regime → switch category weights (flip/zero mean-reversion in trends) → then aggregate. See `docs/03-scoring.md`.
3. **Two orthogonal axes.** Direction/conviction score is `-100..+100`. Entry proximity (imminence) is `0..100`. They are computed and stored separately. Proximity/timing is read off the **daily** timeframe; weekly/monthly are context/gates.
4. **Validate before tuning.** The evaluation harness (P7) must pass before any parameter tuning (P8). Never trust win-rate folklore; prove edge with the binomial test / MFE-MAE / walk-forward in `docs/07-testing.md`.
5. **Do not relitigate locked decisions.** All major decisions are fixed in `docs/08-decisions-adr.md`. If a decision genuinely needs revisiting, raise it explicitly with rationale — don't silently diverge.

## Language policy

- **Japanese**: all human/Shin-facing docs (everything under `docs/`, `README.md`) and all user-visible UI strings.
- **English**: this file, all code, identifiers, code comments, commit messages, test names, and log messages.
- Do not translate `docs/` to English. Do not write Japanese in code identifiers.

## Tech stack (pin versions when scaffolding)

- **Tauri 2** (desktop shell, Rust backend, sidecar support)
- **Next.js + TypeScript** (frontend, strict mode)
- **Rust** (computation core: indicators, regime, scoring, proximity, evaluation)
- **Python sidecar** wrapping **yfinance** (data fetch only), bundled via PyInstaller/Nuitka
- **lightweight-charts v5** (multi-pane charts; use `addSeries`/`addCustomSeries` on panes, series-markers plugin, set `attributionLogo`)
- **SQLite** for OHLCV + score cache; **DuckDB** optional for analytics

## Repository layout (target)

```
alpha-radar/
├── CLAUDE.md                 # this file
├── README.md
├── .gitignore
├── docs/                     # spec (Japanese) — read these
├── src-tauri/                # Rust backend
│   └── src/
│       ├── data/             # CSV parse, cache (SQLite), sidecar client, diff-update
│       ├── indicators/       # pure indicator fns + sub-score normalization (P1)
│       ├── regime/           # ADX/Choppiness regime detection (P2)
│       ├── scoring/          # category weights, single-TF + MTF composite (P2)
│       ├── proximity/        # imminence engine, state machine (P3)
│       ├── eval/             # validation harness, ported from prediction-eval (P7)
│       └── commands/         # Tauri command surface
├── sidecar/                  # Python yfinance fetcher (P0)
│   ├── fetch.py
│   └── build/                # PyInstaller output (gitignored)
├── frontend/ (or src/)       # Next.js app
│   ├── app/                  # routes: scanner (main), chart, settings
│   ├── components/
│   └── lib/                  # Tauri invoke wrappers, types mirrored from Rust
└── tests/
    └── fixtures/             # golden-value OHLCV slices + expected indicator values
```

## Workflow

- Work **phase by phase** per `docs/06-implementation-plan.md` (P0–P8). Do not jump ahead.
- **P0 (Python sidecar)** and **P1 (Rust indicator engine)** are independent and can run in parallel.
- Recommended model usage (Shin's policy): Opus-class for architecture/design review and the math-heavy P2/P3 design; Sonnet-class for implementation; reserve Fable 5 for ambiguous upstream decisions, mathematical agents, and orchestration.
- After completing a phase, update the relevant `docs/` file and the README status. Keep docs and code in sync.
- Each indicator must ship with golden-value unit tests (see `docs/07-testing.md`) — no indicator is "done" without them.

## Conventions

- **Rust**: idiomatic, `thiserror`/`anyhow` for errors, `rayon` for cross-symbol parallelism, pure functions for math (no I/O inside indicator/score fns), deterministic float handling (document any non-determinism).
- **TypeScript**: strict, functional React components, mirror Rust DTOs as TS types in `frontend/lib/types.ts`. No browser storage hacks; state via React.
- **Errors are row-scoped on scan**: a failed symbol (delisted, bad code, rate-limited) is collected and reported, never aborts the whole scan.
- **No magic numbers in code**: indicator params, weights, thresholds come from config (`ScanConfig`/`MtfConfig`), with documented defaults.

## Guardrails

- **No network calls except through the Python sidecar / yfinance.** The Rust core does not hit the internet directly.
- Respect yfinance rate limits: throttle, exponential backoff, batch multi-ticker downloads, cache aggressively (SQLite), differential updates (fetch only after the last cached bar).
- No secrets/keys in the repo (none are needed for yfinance). Add `.env*` to `.gitignore` regardless.
- Data files (`*.db`, `*.sqlite`, caches), build artifacts, `node_modules/`, `target/`, and PyInstaller output are gitignored.

## Quick reference: the pipeline

`CSV → (sidecar) yfinance 1d/1wk/1mo → SQLite cache → Rust indicators → regime → direction score (regime-weighted confluence + MTF α/gates) → proximity engine (Primed/Triggered/Active) → ranking list (sorted by actionability) → chart drill-down`

## Commands

```
# dev (Tauri + Next.js dev)    : cargo tauri dev
# full app build (bundle)      : cargo tauri build
# frontend static export       : npm run build         (in frontend/, emits out/)
# rust tests (golden values)   : cargo test --manifest-path src-tauri/Cargo.toml
# rust lint                    : cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
# regenerate golden fixtures   : uv run --project tools/golden python tools/golden/gen_golden.py
# sync sidecar env (yfinance)  : uv sync --project sidecar
# run sidecar manually (dev)   : uv run --project sidecar python sidecar/fetch.py   (stdin: JSON request)
# bundle sidecar (PyInstaller) : pwsh tools/package-sidecar.ps1   (then `cargo tauri build`)
# package installer (NSIS)     : pwsh tools/package-sidecar.ps1; cargo tauri build --bundles nsis
# frontend lint/typecheck      : npm run lint; npx tsc --noEmit    (in frontend/ — the CI gate)
# release (installers via CI)  : bump tauri.conf.json version; git tag v0.1.0; git push origin v0.1.0
```

## Packaging

`tools/package-sidecar.ps1` builds `sidecar/fetch.py` into a one-file exe (PyInstaller, `--collect-all yfinance`, ~41 MB) and copies it to `src-tauri/binaries/fetch-<target-triple>.exe`. `tauri.conf.json` registers `bundle.externalBin = ["binaries/fetch"]`, so `cargo tauri build` bundles it next to the app exe (Tauri strips the triple → `fetch.exe`). `SidecarClient::resolve()` spawns the bundled `fetch*` next to the running exe in production, else falls back to `uv run` in dev. `src-tauri/binaries/` is gitignored (per-platform build artifact). Verified: the release app launches standalone (embedded frontend, no dev server) and the bundled exe fetches via yfinance.

**Changing the sidecar requires rebuilding the exe** — editing `fetch.py` alone does not affect a bundled build (dev falls back to `uv run` only when no `fetch*` sits next to the running exe).

**CI** (`.github/workflows/`): `ci.yml` (push/PR, windows-latest) runs frontend lint/typecheck/export then `cargo test` + `clippy -D warnings`. `release.yml` (tag `v*` / `workflow_dispatch`) builds the sidecar per-OS via the same ps1 (it is platform-parameterized) and bundles NSIS on windows-latest + an **arm64 .dmg on macos-latest** (Apple Silicon only, **unsigned** — ADR-18; users need `xattr -dr com.apple.quarantine`). Cross-compiling macOS from Windows is impossible (Tauri needs the macOS SDK; PyInstaller embeds the host's Python).

## Pinned versions / tooling (session 1)

- **Tauri**: CLI 2.10.1, crate `tauri 2.10` (build-resolved 2.11.3), `tauri-build 2.5.6`.
- **Frontend**: Next.js 16.2.9 (App Router, `output: 'export'`), React 19.2.4, TypeScript 5. Frontend lives in `frontend/`; package manager is npm.
- **Rust**: edition 2021, toolchain stable (1.96) pinned via `rust-toolchain.toml`.
- **Golden reference**: TA-Lib 0.6.8 via a uv-managed Python 3.12 env under `tools/golden/` (test-time only — NOT the P0 runtime sidecar). pandas-ta 0.3.14b0 was yanked from PyPI, so TA-Lib (named first in ADR-13) is the numeric basis. ATR is pinned to TA-Lib's Wilder seed; this differs from TradingView only in warm-up (signal-irrelevant), per docs/02.
- **App icon**: source `docs/icon.png` (1254² square); regenerate with `cargo tauri icon docs/icon.png`. The in-app header logo is a **copy** of the generated `src-tauri/icons/128x128.png` at `frontend/public/logo.png` — regenerating the icon must refresh both.
