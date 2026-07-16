//! CSV watchlist parsing (docs/04). Header-based, lenient: malformed rows are
//! collected as `RowError` and the scan continues.

use std::path::Path;

use crate::data::universe::{dedupe, normalize_symbol, UniverseEntry};
use crate::error::AppResult;
use crate::models::{AssetClass, RowError};

/// Header aliases for the symbol/code column (case-insensitive). If none match,
/// the first column is used — JP watchlists exported from Excel/brokers put the
/// ticker code in column 1 under headers like "コード" or "証券コード".
const SYMBOL_ALIASES: &[&str] = &[
    "symbol", "code", "ticker", "コード", "ティッカー", "銘柄", "銘柄コード", "証券コード",
];
const NAME_ALIASES: &[&str] = &["name", "名称", "名前", "銘柄名", "会社名", "社名"];
const ASSET_ALIASES: &[&str] = &["asset_class", "asset", "種別", "資産クラス"];
const WEIGHT_ALIASES: &[&str] = &["weight", "ウェイト", "比率", "重み"];

/// Decode raw CSV bytes to a `String`, tolerating non-UTF-8 watchlists. Order:
/// UTF-8 BOM → valid UTF-8 → Shift_JIS/CP932 (Excel on Japanese Windows).
pub fn decode_bytes(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_owned();
    }
    encoding_rs::SHIFT_JIS.decode(bytes).0.into_owned()
}

/// Parse a watchlist CSV from a string. Returns deduped entries plus per-row
/// errors. The only required column is `symbol`; `name` / `asset_class` /
/// `weight` are optional.
pub fn parse_csv_str(content: &str) -> (Vec<UniverseEntry>, Vec<RowError>) {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(content.as_bytes());

    let headers = match rdr.headers() {
        Ok(h) => h.clone(),
        Err(e) => {
            return (
                vec![],
                vec![RowError {
                    symbol: "*".into(),
                    reason: format!("invalid header: {e}"),
                }],
            )
        }
    };
    let col = |name: &str| headers.iter().position(|h| h.eq_ignore_ascii_case(name));
    let find = |aliases: &[&str]| aliases.iter().find_map(|n| col(n));
    // Symbol/code column: a known alias, else the first column (docs/04).
    let i_sym = find(SYMBOL_ALIASES).unwrap_or(0);
    let (i_name, i_ac, i_wt) = (find(NAME_ALIASES), find(ASSET_ALIASES), find(WEIGHT_ALIASES));

    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for (n, rec) in rdr.records().enumerate() {
        let line = n + 2; // header is line 1
        let rec = match rec {
            Ok(r) => r,
            Err(e) => {
                errors.push(RowError {
                    symbol: format!("line {line}"),
                    reason: e.to_string(),
                });
                continue;
            }
        };
        let raw = rec.get(i_sym).unwrap_or("").trim();
        if raw.is_empty() {
            errors.push(RowError {
                symbol: format!("line {line}"),
                reason: "empty symbol".into(),
            });
            continue;
        }
        let symbol = normalize_symbol(raw);
        let name = i_name
            .and_then(|i| rec.get(i))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let asset_class = i_ac
            .and_then(|i| rec.get(i))
            .and_then(AssetClass::parse)
            .unwrap_or_else(|| AssetClass::infer(&symbol));
        let weight = i_wt
            .and_then(|i| rec.get(i))
            .and_then(|s| s.trim().parse::<f64>().ok());
        entries.push(UniverseEntry {
            symbol,
            name,
            asset_class,
            weight,
        });
    }

    (dedupe(entries), errors)
}

/// Parse a watchlist CSV from raw bytes (decodes UTF-8 / Shift_JIS — see
/// [`decode_bytes`]).
pub fn parse_csv_bytes(bytes: &[u8]) -> (Vec<UniverseEntry>, Vec<RowError>) {
    parse_csv_str(&decode_bytes(bytes))
}

/// Parse a watchlist CSV from a file path.
pub fn parse_csv_path(path: impl AsRef<Path>) -> AppResult<(Vec<UniverseEntry>, Vec<RowError>)> {
    let bytes = std::fs::read(path)?;
    Ok(parse_csv_bytes(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_required_and_optional_columns() {
        let csv = "symbol,name,asset_class,weight\n\
                   7974.T,任天堂,equity,1.0\n\
                   aapl,,,\n\
                   BTC-USD,Bitcoin,,2.5\n";
        let (entries, errors) = parse_csv_str(csv);
        assert!(errors.is_empty());
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].symbol, "7974.T");
        assert_eq!(entries[0].name.as_deref(), Some("任天堂"));
        assert_eq!(entries[0].weight, Some(1.0));
        // lowercased ticker is upper-cased; asset_class inferred as equity.
        assert_eq!(entries[1].symbol, "AAPL");
        assert_eq!(entries[1].asset_class, AssetClass::Equity);
        // crypto inferred from the -USD suffix.
        assert_eq!(entries[2].asset_class, AssetClass::Crypto);
        assert_eq!(entries[2].weight, Some(2.5));
    }

    #[test]
    fn empty_symbol_becomes_row_error_and_continues() {
        // Truly blank lines are skipped by the reader; an explicit empty symbol
        // field becomes a row error while the scan continues.
        let csv = "symbol,name\nAAPL,Apple\n,Ghost\nMSFT,Microsoft\n";
        let (entries, errors) = parse_csv_str(csv);
        assert_eq!(entries.len(), 2);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].reason, "empty symbol");
    }

    #[test]
    fn japanese_code_header_and_first_column_fallback() {
        // "コード" maps to the symbol via alias; "名称" to name. Bare TSE code
        // gets the .T suffix.
        let csv = "コード,名称\n7974,任天堂\n7203,トヨタ自動車\n";
        let (entries, errors) = parse_csv_str(csv);
        assert!(errors.is_empty());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].symbol, "7974.T");
        assert_eq!(entries[0].name.as_deref(), Some("任天堂"));

        // An unrecognized first-column header still works (first column = code).
        let csv2 = "証券番号,会社名\nAAPL,Apple\n";
        let (entries2, errors2) = parse_csv_str(csv2);
        assert!(errors2.is_empty());
        assert_eq!(entries2[0].symbol, "AAPL");
        assert_eq!(entries2[0].name.as_deref(), Some("Apple"));
    }

    #[test]
    fn decodes_shift_jis_watchlist() {
        // Excel on JP Windows saves Shift_JIS (CP932). Round-trip through the
        // encoder so the test has no hand-coded byte literals.
        let utf8 = "コード,名称\n7974,任天堂\n";
        let (sjis, _, had_errors) = encoding_rs::SHIFT_JIS.encode(utf8);
        assert!(!had_errors);
        assert_ne!(sjis.as_ref(), utf8.as_bytes()); // genuinely non-UTF-8 bytes
        let (entries, errors) = parse_csv_bytes(&sjis);
        assert!(errors.is_empty());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].symbol, "7974.T");
        assert_eq!(entries[0].name.as_deref(), Some("任天堂"));
    }
}
