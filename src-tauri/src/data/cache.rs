//! SQLite cache — the source of truth for OHLCV and scores, with differential
//! update (docs/04). Schema and the freshness/diff logic mirror docs/04.

use std::path::Path;

use rusqlite::{params, Connection};

use crate::data::universe::UniverseEntry;
use crate::error::AppResult;
use crate::models::{Candle, SymbolScore, Tf};

const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS symbols (
    symbol       TEXT PRIMARY KEY,
    name         TEXT,
    asset_class  TEXT NOT NULL,
    last_seen    INTEGER
);
CREATE TABLE IF NOT EXISTS ohlcv (
    symbol  TEXT NOT NULL,
    tf      TEXT NOT NULL,
    ts      INTEGER NOT NULL,
    open REAL, high REAL, low REAL, close REAL, volume REAL, adj_close REAL,
    PRIMARY KEY (symbol, tf, ts)
);
CREATE INDEX IF NOT EXISTS idx_ohlcv_sym_tf_ts ON ohlcv(symbol, tf, ts);
CREATE TABLE IF NOT EXISTS scores (
    symbol            TEXT NOT NULL,
    scanned_at        INTEGER NOT NULL,
    regime            TEXT,
    score_final       REAL,
    score_daily REAL, score_weekly REAL, score_monthly REAL,
    s_trend REAL, s_momentum REAL, s_mean_reversion REAL,
    signal_state      TEXT,
    direction         TEXT,
    proximity_score   REAL,
    bars_since_trigger INTEGER,
    actionability     REAL,
    atr               REAL,
    suggested_stop    REAL,
    marker_hit_rate   REAL,
    marker_samples    INTEGER,
    last_marker_kind  TEXT,
    last_marker_dir   INTEGER,
    last_marker_bars  INTEGER,
    PRIMARY KEY (symbol, scanned_at)
);
CREATE INDEX IF NOT EXISTS idx_scores_scan ON scores(scanned_at);
CREATE TABLE IF NOT EXISTS scan_runs (
    scanned_at  INTEGER PRIMARY KEY,
    csv_path    TEXT,
    config_json TEXT,
    n_symbols   INTEGER,
    n_errors    INTEGER
);
"#;

/// Additive schema migration: `CREATE TABLE IF NOT EXISTS` never alters an
/// existing table, so columns added after a DB was created must be back-filled
/// with `ALTER TABLE` (values default to NULL for older rows).
fn migrate(conn: &Connection) -> AppResult<()> {
    let existing: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('scores')")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for (col, ty) in [
        ("marker_hit_rate", "REAL"),
        ("marker_samples", "INTEGER"),
        ("last_marker_kind", "TEXT"),
        ("last_marker_dir", "INTEGER"),
        ("last_marker_bars", "INTEGER"),
    ] {
        if !existing.iter().any(|c| c == col) {
            conn.execute(&format!("ALTER TABLE scores ADD COLUMN {col} {ty}"), [])?;
        }
    }
    Ok(())
}

/// What to fetch for a (symbol, tf) given the cache state (docs/04 diff update).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchDecision {
    /// No cached bars — request full history (`period="max"`).
    Full,
    /// Cached but stale — request from the last cached bar (overlap upserts).
    From(i64),
    /// Cache is fresh for this timeframe.
    Skip,
}

/// SQLite-backed cache.
pub struct Cache {
    conn: Connection,
}

impl Cache {
    /// Open (and initialize) a file-backed cache.
    pub fn open(path: impl AsRef<Path>) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(DDL)?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Open an in-memory cache (tests).
    pub fn open_in_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(DDL)?;
        Ok(Self { conn })
    }

    /// Latest cached bar timestamp for a (symbol, tf).
    pub fn last_ts(&self, symbol: &str, tf: Tf) -> AppResult<Option<i64>> {
        let v = self.conn.query_row(
            "SELECT max(ts) FROM ohlcv WHERE symbol = ?1 AND tf = ?2",
            params![symbol, tf.interval()],
            |row| row.get::<_, Option<i64>>(0),
        )?;
        Ok(v)
    }

    /// Decide what to fetch for a (symbol, tf) at time `now` (Unix seconds).
    pub fn fetch_plan(&self, symbol: &str, tf: Tf, now: i64) -> AppResult<FetchDecision> {
        Ok(match self.last_ts(symbol, tf)? {
            None => FetchDecision::Full,
            Some(last) if now - last > tf.bar_seconds() => FetchDecision::From(last),
            Some(_) => FetchDecision::Skip,
        })
    }

    /// Upsert OHLCV bars (re-fetched final bars overwrite provisional ones).
    pub fn upsert_candles(&mut self, symbol: &str, tf: Tf, candles: &[Candle]) -> AppResult<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO ohlcv(symbol, tf, ts, open, high, low, close, volume, adj_close)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(symbol, tf, ts) DO UPDATE SET
                   open=excluded.open, high=excluded.high, low=excluded.low,
                   close=excluded.close, volume=excluded.volume, adj_close=excluded.adj_close",
            )?;
            for c in candles {
                stmt.execute(params![
                    symbol,
                    tf.interval(),
                    c.ts,
                    c.open,
                    c.high,
                    c.low,
                    c.close,
                    c.volume,
                    c.adj_close
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Load all cached bars for a (symbol, tf), oldest first.
    pub fn load_candles(&self, symbol: &str, tf: Tf) -> AppResult<Vec<Candle>> {
        let mut stmt = self.conn.prepare(
            "SELECT ts, open, high, low, close, volume, adj_close
             FROM ohlcv WHERE symbol = ?1 AND tf = ?2 ORDER BY ts",
        )?;
        let rows = stmt.query_map(params![symbol, tf.interval()], |row| {
            Ok(Candle {
                ts: row.get(0)?,
                open: row.get(1)?,
                high: row.get(2)?,
                low: row.get(3)?,
                close: row.get(4)?,
                volume: row.get(5)?,
                adj_close: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Record / refresh a symbol's metadata.
    pub fn upsert_symbol(&self, e: &UniverseEntry, last_seen: i64) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO symbols(symbol, name, asset_class, last_seen)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(symbol) DO UPDATE SET
               name=excluded.name, asset_class=excluded.asset_class, last_seen=excluded.last_seen",
            params![e.symbol, e.name, e.asset_class.as_str(), last_seen],
        )?;
        Ok(())
    }

    /// Persist a scored symbol for a scan run.
    pub fn save_score(&self, s: &SymbolScore, scanned_at: i64) -> AppResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO scores(
                symbol, scanned_at, regime, score_final, score_daily, score_weekly, score_monthly,
                s_trend, s_momentum, s_mean_reversion, signal_state, direction, proximity_score,
                bars_since_trigger, actionability, atr, suggested_stop,
                marker_hit_rate, marker_samples,
                last_marker_kind, last_marker_dir, last_marker_bars)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)",
            params![
                s.symbol,
                scanned_at,
                s.regime.map(|r| format!("{r:?}")),
                s.score_final,
                s.score_daily,
                s.score_weekly,
                s.score_monthly,
                s.s_trend,
                s.s_momentum,
                s.s_mean_reversion,
                format!("{:?}", s.signal_state),
                format!("{:?}", s.direction),
                s.proximity_score,
                s.bars_since_trigger,
                s.actionability,
                s.atr,
                s.suggested_stop,
                s.marker_hit_rate,
                s.marker_samples,
                s.last_marker.map(|m| m.kind.as_str()),
                s.last_marker.map(|m| m.dir),
                s.last_marker.map(|m| m.bars_ago),
            ],
        )?;
        Ok(())
    }

    /// Record a scan run (config snapshot for reproducibility, ADR-10).
    pub fn save_scan_run(
        &self,
        scanned_at: i64,
        csv_path: &str,
        config_json: &str,
        n_symbols: usize,
        n_errors: usize,
    ) -> AppResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO scan_runs(scanned_at, csv_path, config_json, n_symbols, n_errors)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![scanned_at, csv_path, config_json, n_symbols as i64, n_errors as i64],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candle(ts: i64, c: f64) -> Candle {
        Candle::ohlcv(ts, c, c + 1.0, c - 1.0, c, 1000.0)
    }

    #[test]
    fn upsert_load_and_freshness() {
        let mut cache = Cache::open_in_memory().unwrap();
        assert_eq!(
            cache.fetch_plan("AAPL", Tf::Daily, 1_000_000).unwrap(),
            FetchDecision::Full
        );

        let bars = vec![candle(100, 10.0), candle(186_500, 11.0)];
        cache.upsert_candles("AAPL", Tf::Daily, &bars).unwrap();
        assert_eq!(cache.last_ts("AAPL", Tf::Daily).unwrap(), Some(186_500));
        assert_eq!(cache.load_candles("AAPL", Tf::Daily).unwrap().len(), 2);

        // now close to last bar → fresh; far past → fetch from last.
        assert_eq!(
            cache.fetch_plan("AAPL", Tf::Daily, 186_500 + 10).unwrap(),
            FetchDecision::Skip
        );
        assert_eq!(
            cache
                .fetch_plan("AAPL", Tf::Daily, 186_500 + 5 * 86_400)
                .unwrap(),
            FetchDecision::From(186_500)
        );
    }

    #[test]
    fn upsert_overwrites_provisional_bar() {
        let mut cache = Cache::open_in_memory().unwrap();
        cache
            .upsert_candles("X", Tf::Daily, &[candle(100, 10.0)])
            .unwrap();
        cache
            .upsert_candles("X", Tf::Daily, &[candle(100, 12.0)])
            .unwrap();
        let loaded = cache.load_candles("X", Tf::Daily).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].close, 12.0);
    }

    #[test]
    fn migrate_adds_new_score_columns_to_old_db() {
        use crate::models::{AssetClass, LastMarker, MarkerKind, SymbolScore};
        use crate::proximity::{Direction, SignalState};

        // Simulate a DB created before the marker columns existed.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE scores (
                symbol TEXT NOT NULL, scanned_at INTEGER NOT NULL,
                regime TEXT, score_final REAL,
                score_daily REAL, score_weekly REAL, score_monthly REAL,
                s_trend REAL, s_momentum REAL, s_mean_reversion REAL,
                signal_state TEXT, direction TEXT, proximity_score REAL,
                bars_since_trigger INTEGER, actionability REAL,
                atr REAL, suggested_stop REAL,
                PRIMARY KEY (symbol, scanned_at)
            );",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let cache = Cache { conn };

        let score = SymbolScore {
            symbol: "AAPL".into(),
            name: None,
            asset_class: AssetClass::Equity,
            regime: None,
            score_final: Some(50.0),
            score_daily: None,
            score_weekly: None,
            score_monthly: None,
            s_trend: None,
            s_momentum: None,
            s_mean_reversion: None,
            signal_state: SignalState::Neutral,
            direction: Direction::None,
            proximity_score: 0.0,
            bars_since_trigger: None,
            actionability: 0.0,
            atr: None,
            suggested_stop: None,
            marker_hit_rate: Some(0.5),
            marker_samples: 10,
            last_marker: Some(LastMarker {
                kind: MarkerKind::QtPrecursor,
                dir: 1,
                bars_ago: 2,
            }),
        };
        cache.save_score(&score, 1_000).unwrap();
        let (kind, dir, bars): (String, i8, u32) = cache
            .conn
            .query_row(
                "SELECT last_marker_kind, last_marker_dir, last_marker_bars
                 FROM scores WHERE symbol='AAPL'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(kind, "qt_precursor");
        assert_eq!(dir, 1);
        assert_eq!(bars, 2);
    }
}
