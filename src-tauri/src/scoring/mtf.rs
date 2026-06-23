//! Multi-timeframe integration (docs/03 §5, ADR-09): α-weighted blend, a strong
//! weekly direction gate, and a soft monthly modifier (never a hard gate).

use crate::config::MtfConfig;
use crate::regime::Regime;

/// Directional sign of a score: +1 buy, −1 sell, 0 flat.
fn dir_of(score: f64) -> i8 {
    if score > 0.0 {
        1
    } else if score < 0.0 {
        -1
    } else {
        0
    }
}

/// Weekly gate (strong): aligned ⇒ 1.0, neutral/missing ⇒ 0.8, opposed ⇒ 0.4
/// (docs/03 §5). Never zero — a counter higher-timeframe is low-conviction, not
/// a veto.
pub fn weekly_gate(daily_dir: i8, weekly_regime: Option<Regime>, cfg: &MtfConfig) -> f64 {
    if daily_dir == 0 {
        return cfg.weekly_gate_neutral;
    }
    match weekly_regime {
        Some(r) if r.is_trend() => {
            if r.direction() == daily_dir {
                cfg.weekly_gate_aligned
            } else {
                cfg.weekly_gate_opposed
            }
        }
        _ => cfg.weekly_gate_neutral, // range / transition / missing
    }
}

/// Monthly modifier (soft): aligned ⇒ 1.1, opposed ⇒ 0.85, else 1.0 (docs/03
/// §5). Disabled or flat-daily ⇒ 1.0. Never a hard gate.
pub fn monthly_mod(daily_dir: i8, monthly_regime: Option<Regime>, cfg: &MtfConfig) -> f64 {
    if !cfg.monthly_enabled || daily_dir == 0 {
        return 1.0;
    }
    match monthly_regime {
        Some(r) if r.is_trend() => {
            if r.direction() == daily_dir {
                cfg.monthly_mod_aligned
            } else {
                cfg.monthly_mod_opposed
            }
        }
        _ => 1.0,
    }
}

/// Combine the per-timeframe scores into `Score_final` (docs/03 §5).
///
/// The α blend is renormalized over the available timeframes (daily always
/// present), so a symbol missing weekly/monthly data degrades gracefully rather
/// than being scaled down. The weekly gate / monthly modifier then apply.
pub fn mtf_combine(
    daily: f64,
    weekly: Option<f64>,
    monthly: Option<f64>,
    weekly_regime: Option<Regime>,
    monthly_regime: Option<Regime>,
    cfg: &MtfConfig,
) -> f64 {
    let mut num = cfg.alpha[0] * daily;
    let mut den = cfg.alpha[0];
    if let Some(w) = weekly {
        num += cfg.alpha[1] * w;
        den += cfg.alpha[1];
    }
    if let Some(m) = monthly {
        num += cfg.alpha[2] * m;
        den += cfg.alpha[2];
    }
    let score_mtf = num / den;

    let dir = dir_of(daily);
    let gate = weekly_gate(dir, weekly_regime, cfg) * monthly_mod(dir, monthly_regime, cfg);
    (score_mtf * gate).clamp(-100.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MtfConfig;

    #[test]
    fn weekly_gate_directions() {
        let cfg = MtfConfig::default();
        assert_eq!(weekly_gate(1, Some(Regime::TrendUp), &cfg), 1.0);
        assert_eq!(weekly_gate(1, Some(Regime::TrendDown), &cfg), 0.4);
        assert_eq!(weekly_gate(1, Some(Regime::Range), &cfg), 0.8);
        assert_eq!(weekly_gate(1, None, &cfg), 0.8);
    }

    #[test]
    fn monthly_modifier_is_soft_and_disable_able() {
        let mut cfg = MtfConfig::default();
        assert_eq!(monthly_mod(1, Some(Regime::TrendUp), &cfg), 1.1);
        assert_eq!(monthly_mod(1, Some(Regime::TrendDown), &cfg), 0.85);
        cfg.monthly_enabled = false;
        assert_eq!(monthly_mod(1, Some(Regime::TrendDown), &cfg), 1.0);
    }

    #[test]
    fn opposed_weekly_attenuates_but_keeps_sign() {
        let cfg = MtfConfig::default();
        // Daily buy 80, weekly down-trend ⇒ gated to 0.4, still positive.
        let f = mtf_combine(80.0, Some(60.0), None, Some(Regime::TrendDown), None, &cfg);
        assert!(f > 0.0 && f < 80.0, "got {f}");
    }

    #[test]
    fn missing_weekly_monthly_renormalizes() {
        let cfg = MtfConfig::default();
        // Daily-only: blend == daily; neutral weekly gate (0.8) applies.
        let f = mtf_combine(50.0, None, None, None, None, &cfg);
        assert!((f - 50.0 * 0.8).abs() < 1e-9, "got {f}");
    }
}
