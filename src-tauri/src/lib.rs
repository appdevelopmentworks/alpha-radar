// Alpha Radar — Rust computation core.
//
// Single source of truth for all indicator/regime/score/proximity math
// (docs/01-architecture.md, ADR-06). The frontend and chart never recompute;
// they render Rust-produced series.
//
// Module tree mirrors docs/01-architecture.md. Modules carrying only a phase
// marker are skeleton stubs filled in by their phase (P2–P7); session 1
// implements config/error/models and the indicators base primitives (P1).

pub mod config;
pub mod error;
pub mod models;

pub mod indicators;

// --- skeleton (filled in by later phases) ---
pub mod commands;
pub mod data;
pub mod eval;
pub mod proximity;
pub mod regime;
pub mod scoring;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::scan_universe,
            commands::scan_symbols,
            commands::get_config,
            commands::update_config,
            commands::get_ui_prefs,
            commands::update_ui_prefs,
            commands::get_presets,
            commands::get_chart_data,
            commands::evaluate_model
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
