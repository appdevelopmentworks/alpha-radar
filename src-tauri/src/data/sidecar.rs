//! Python sidecar client (P0). Spawns the stateless yfinance fetcher, writes a
//! JSON request to its stdin, and parses the JSON response from stdout. The
//! sidecar does fetching only; caching / diff logic lives in `cache.rs`
//! (docs/04 "Python サイドカー I/O 契約").

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::models::Candle;

/// One fetch request item (`start=None` ⇒ `period="max"`).
#[derive(Debug, Clone, Serialize)]
pub struct FetchRequestItem {
    pub symbol: String,
    pub interval: String,
    pub start: Option<i64>,
    pub end: Option<i64>,
}

/// A batched fetch request (multiple symbols × intervals in one process).
#[derive(Debug, Clone, Serialize)]
pub struct FetchRequest {
    pub requests: Vec<FetchRequestItem>,
    pub auto_adjust: bool,
    pub output: String,
}

impl FetchRequest {
    /// A JSON batch request with `auto_adjust=true`, `output="json"`.
    pub fn json(requests: Vec<FetchRequestItem>) -> Self {
        Self {
            requests,
            auto_adjust: true,
            output: "json".to_string(),
        }
    }
}

/// One OHLCV bar from the sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct CandleDto {
    pub ts: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub adj_close: f64,
}

impl From<CandleDto> for Candle {
    fn from(d: CandleDto) -> Self {
        Candle {
            ts: d.ts,
            open: d.open,
            high: d.high,
            low: d.low,
            close: d.close,
            volume: d.volume,
            adj_close: d.adj_close,
        }
    }
}

/// Fetched series for one symbol × interval.
#[derive(Debug, Clone, Deserialize)]
pub struct FetchResultItem {
    pub symbol: String,
    pub interval: String,
    pub candles: Vec<CandleDto>,
}

/// A structured per-(symbol, interval) fetch failure.
#[derive(Debug, Clone, Deserialize)]
pub struct FetchErrorItem {
    pub symbol: String,
    pub interval: String,
    pub reason: String,
}

/// The sidecar response.
#[derive(Debug, Clone, Deserialize)]
pub struct FetchResponse {
    pub results: Vec<FetchResultItem>,
    #[serde(default)]
    pub errors: Vec<FetchErrorItem>,
}

/// Spawns the sidecar process for a JSON round-trip. The command is injectable
/// so dev runs the script via `uv` and release runs the bundled `externalBin`.
#[derive(Debug, Clone)]
pub struct SidecarClient {
    program: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
}

impl SidecarClient {
    pub fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            cwd: None,
        }
    }

    /// Pick the production sidecar (the bundled `fetch` binary next to the app
    /// executable, placed there via `bundle.externalBin`) when present, else
    /// fall back to the dev script run via uv.
    pub fn resolve() -> Self {
        match bundled_sidecar() {
            Some(exe) => Self::new(exe.to_string_lossy().into_owned(), Vec::new()),
            None => Self::dev_uv(repo_root()),
        }
    }

    /// Dev client: run `sidecar/fetch.py` via uv from `repo_root`.
    pub fn dev_uv(repo_root: impl Into<PathBuf>) -> Self {
        Self {
            program: "uv".to_string(),
            args: ["run", "--project", "sidecar", "python", "sidecar/fetch.py"]
                .map(String::from)
                .to_vec(),
            cwd: Some(repo_root.into()),
        }
    }

    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Run one batch request through the sidecar.
    pub fn fetch(&self, request: &FetchRequest) -> AppResult<FetchResponse> {
        let payload = serde_json::to_vec(request)?;

        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &self.cwd {
            cmd.current_dir(cwd);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| AppError::Sidecar(format!("spawn {}: {e}", self.program)))?;

        // Write the request and close stdin so the child sees EOF. The payload
        // is small, so writing fully before draining stdout cannot deadlock.
        child
            .stdin
            .take()
            .ok_or_else(|| AppError::Sidecar("no stdin pipe".into()))?
            .write_all(&payload)
            .map_err(|e| AppError::Sidecar(format!("write stdin: {e}")))?;

        let output = child
            .wait_with_output()
            .map_err(|e| AppError::Sidecar(format!("wait: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Sidecar(format!(
                "exit {:?}: {}",
                output.status.code(),
                stderr.trim()
            )));
        }

        serde_json::from_slice(&output.stdout).map_err(|e| {
            let stderr = String::from_utf8_lossy(&output.stderr);
            AppError::Sidecar(format!("parse response: {e}; stderr: {}", stderr.trim()))
        })
    }
}

/// Resolve the repository root (parent of `src-tauri`) for dev sidecar spawns.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Find a bundled `fetch[-<triple>][.exe]` sidecar next to the running app
/// executable (Tauri places `externalBin` binaries there). `None` in a plain
/// dev checkout where no bundle has been produced.
fn bundled_sidecar() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| {
                        let n = n.to_ascii_lowercase();
                        n == "fetch" || n.starts_with("fetch.") || n.starts_with("fetch-")
                    })
                    .unwrap_or(false)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes_to_contract_shape() {
        let req = FetchRequest::json(vec![FetchRequestItem {
            symbol: "7974.T".into(),
            interval: "1d".into(),
            start: Some(1_719_100_800),
            end: None,
        }]);
        let v: serde_json::Value = serde_json::to_value(&req).unwrap();
        assert_eq!(v["auto_adjust"], true);
        assert_eq!(v["output"], "json");
        assert_eq!(v["requests"][0]["symbol"], "7974.T");
        assert_eq!(v["requests"][0]["interval"], "1d");
        assert_eq!(v["requests"][0]["start"], 1_719_100_800_i64);
        assert!(v["requests"][0]["end"].is_null());
    }

    #[test]
    fn response_parses_from_contract_json() {
        let json = r#"{
            "results": [
                { "symbol": "7974.T", "interval": "1d",
                  "candles": [
                    { "ts": 1719100800, "open": 7030, "high": 7080, "low": 7001,
                      "close": 7079, "volume": 1234500, "adj_close": 7079 }
                  ] }
            ],
            "errors": [
                { "symbol": "XXXX.T", "interval": "1d", "reason": "no data / delisted" }
            ]
        }"#;
        let resp: FetchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.results.len(), 1);
        assert_eq!(resp.results[0].candles.len(), 1);
        assert_eq!(resp.errors.len(), 1);
        let c: Candle = resp.results[0].candles[0].into();
        assert_eq!(c.close, 7079.0);
    }

    /// Real process round-trip via the offline mock fetcher. Requires `uv` on
    /// PATH; run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "spawns uv + python; run manually"]
    fn mock_sidecar_round_trip() {
        let client = SidecarClient::new(
            "uv",
            ["run", "--python", "3.12", "python", "sidecar/mock_fetch.py"]
                .map(String::from)
                .to_vec(),
        )
        .with_cwd(repo_root());
        let req = FetchRequest::json(vec![FetchRequestItem {
            symbol: "TEST".into(),
            interval: "1d".into(),
            start: None,
            end: None,
        }]);
        let resp = client.fetch(&req).expect("round-trip");
        assert_eq!(resp.results.len(), 1);
        assert!(!resp.results[0].candles.is_empty());
    }
}
