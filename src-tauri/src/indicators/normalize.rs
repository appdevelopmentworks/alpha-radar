//! Sub-score normalization (docs/02-indicators.md "サブスコア正規化の方針").
//!
//! Every indicator emits a normalized sub-score `s ∈ [-1, +1]` with the
//! convention **+ = buy, − = sell**. Regime-dependent sign flip / zeroing is
//! applied later in `scoring` (ADR-07), never here.
//!
//! Three shapes:
//! - bounded oscillators (RSI, %B, Williams %R): linearly map their range to
//!   `[-1, +1]` ([`map_range`]),
//! - unbounded values (MACD histogram, z-score): compress with `tanh(x / k)`
//!   ([`tanh_compress`]), where `k` is an ATR/σ-based scale,
//! - trend stacks (MA order, Supertrend, ADX direction): sign × strength,
//!   assembled by the caller and passed through [`clamp_unit`].
//!
//! Insufficient history is represented as `None` by the indicator functions and
//! excluded from category means in `scoring`; these helpers operate on concrete
//! `f64` values only.

/// Clamp a value to the sub-score range `[-1, +1]`.
pub fn clamp_unit(x: f64) -> f64 {
    x.clamp(-1.0, 1.0)
}

/// Linearly map `value` from input range `[in_lo, in_hi]` onto `[-1, +1]`,
/// then clamp. `in_lo` maps to −1 and `in_hi` to +1.
///
/// Returns `0.0` for a degenerate range (`in_hi == in_lo`).
pub fn map_range(value: f64, in_lo: f64, in_hi: f64) -> f64 {
    let span = in_hi - in_lo;
    if span == 0.0 {
        return 0.0;
    }
    let t = (value - in_lo) / span; // 0..1 across the range
    clamp_unit(2.0 * t - 1.0)
}

/// Compress an unbounded value into `(-1, +1)` via `tanh(x / k)`.
///
/// `k` is the scale at which the score reaches ~0.76 (`tanh(1)`); it is
/// typically derived from ATR or a rolling standard deviation. A non-positive
/// `k` is treated as a degenerate scale and yields `0.0`.
pub fn tanh_compress(x: f64, k: f64) -> f64 {
    if k <= 0.0 {
        return 0.0;
    }
    (x / k).tanh()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_unit_bounds() {
        assert_eq!(clamp_unit(2.0), 1.0);
        assert_eq!(clamp_unit(-2.0), -1.0);
        assert_eq!(clamp_unit(0.3), 0.3);
    }

    #[test]
    fn map_range_endpoints_and_midpoint() {
        // RSI-style: map [0, 100] so that 50 is neutral.
        assert_eq!(map_range(0.0, 0.0, 100.0), -1.0);
        assert_eq!(map_range(100.0, 0.0, 100.0), 1.0);
        assert_eq!(map_range(50.0, 0.0, 100.0), 0.0);
    }

    #[test]
    fn map_range_degenerate_is_neutral() {
        assert_eq!(map_range(5.0, 3.0, 3.0), 0.0);
    }

    #[test]
    fn tanh_compress_is_odd_and_bounded() {
        assert_eq!(tanh_compress(0.0, 0.5), 0.0);
        // tanh_compress(x, k) == tanh(x / k); here 0.5 / 0.5 == 1.0.
        assert!((tanh_compress(0.5, 0.5) - 1.0f64.tanh()).abs() < 1e-12);
        assert!((tanh_compress(1.0, 0.5) + tanh_compress(-1.0, 0.5)).abs() < 1e-12);
        assert_eq!(tanh_compress(10.0, 0.0), 0.0); // degenerate scale
    }
}
