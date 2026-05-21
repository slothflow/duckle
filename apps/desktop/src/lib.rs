//! Duckle desktop shell.
//!
//! Boots the Tauri runtime, wires it to `duckle-runtime`, and exposes
//! invoke commands to the frontend.

use tracing_subscriber::EnvFilter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    tracing::info!("duckle starting");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![ping])
        .run(tauri::generate_context!())
        .expect("error while running duckle");
}

/// Liveness probe. Returns the string `"pong"`.
///
/// Used by the frontend on startup to confirm the Rust runtime is reachable.
#[tauri::command]
fn ping() -> &'static str {
    "pong"
}
