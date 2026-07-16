//! Symbol universe management (docs/04). A `UniverseEntry` is one watchlist row
//! after normalization; duplicates are collapsed keeping the first occurrence.

use std::collections::HashSet;

use crate::models::AssetClass;

/// A normalized watchlist entry.
#[derive(Debug, Clone, PartialEq)]
pub struct UniverseEntry {
    pub symbol: String,
    pub name: Option<String>,
    pub asset_class: AssetClass,
    pub weight: Option<f64>,
}

/// Collapse duplicate symbols, keeping the first occurrence's metadata
/// (docs/04: "重複 symbol は1回だけ取得・スコア").
pub fn dedupe(entries: Vec<UniverseEntry>) -> Vec<UniverseEntry> {
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .filter(|e| seen.insert(e.symbol.clone()))
        .collect()
}

/// Normalize a raw symbol to yfinance form. Bare Tokyo Stock Exchange codes
/// (e.g. `7974`, `130A`) get a `.T` suffix so `7974` and `7974.T` both resolve.
pub fn normalize_symbol(raw: &str) -> String {
    let s = raw.trim().to_uppercase();
    if is_bare_tse_code(&s) {
        format!("{s}.T")
    } else {
        s
    }
}

/// A bare TSE code: exactly 4 chars, no exchange/quote suffix, with the first
/// three digits — classic `7974` or post-2024 alphanumeric `130A`.
fn is_bare_tse_code(s: &str) -> bool {
    if s.len() != 4 || s.contains('.') || s.contains('-') {
        return false;
    }
    let b = s.as_bytes();
    b[..3].iter().all(u8::is_ascii_digit) && (b[3].is_ascii_digit() || b[3].is_ascii_alphabetic())
}

/// Parse a free-text ticker list (comma / whitespace / newline separated) into
/// universe entries. Symbols are normalized (see [`normalize_symbol`]) and the
/// asset class is inferred.
pub fn parse_symbols_str(input: &str) -> Vec<UniverseEntry> {
    let entries = input
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| {
            let symbol = normalize_symbol(t);
            let asset_class = AssetClass::infer(&symbol);
            UniverseEntry {
                symbol,
                name: None,
                asset_class,
                weight: None,
            }
        })
        .collect();
    dedupe(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_bare_tse_codes() {
        assert_eq!(normalize_symbol("7974"), "7974.T"); // classic 4-digit code
        assert_eq!(normalize_symbol("7974.T"), "7974.T"); // already suffixed
        assert_eq!(normalize_symbol("130A"), "130A.T"); // post-2024 alphanumeric
        assert_eq!(normalize_symbol("aapl"), "AAPL"); // US ticker
        assert_eq!(normalize_symbol("MSFT"), "MSFT"); // 4 alpha — not a code
        assert_eq!(normalize_symbol("BTC-USD"), "BTC-USD"); // crypto pair
    }

    #[test]
    fn parse_symbols_handles_mixed_separators_and_bare_codes() {
        // "7974" normalizes to "7974.T" and dedupes against an explicit "7974.T".
        let out = parse_symbols_str(" aapl, 7974, 7974.T, btc-usd, 130A, aapl ");
        let syms: Vec<&str> = out.iter().map(|e| e.symbol.as_str()).collect();
        assert_eq!(syms, ["AAPL", "7974.T", "BTC-USD", "130A.T"]);
        assert_eq!(out[1].asset_class, AssetClass::Equity);
        assert_eq!(out[2].asset_class, AssetClass::Crypto);
    }

    #[test]
    fn dedupe_keeps_first() {
        let entries = vec![
            UniverseEntry {
                symbol: "AAPL".into(),
                name: Some("Apple".into()),
                asset_class: AssetClass::Equity,
                weight: None,
            },
            UniverseEntry {
                symbol: "AAPL".into(),
                name: Some("dup".into()),
                asset_class: AssetClass::Equity,
                weight: None,
            },
        ];
        let out = dedupe(entries);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name.as_deref(), Some("Apple"));
    }
}
