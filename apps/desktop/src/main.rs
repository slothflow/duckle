// Windows: hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Headless self-test of the in-app updater (download/verify/swap), compiled
    // in only with `--features update-selftest`. Never present in releases.
    #[cfg(feature = "update-selftest")]
    if std::env::args().any(|a| a == "--self-update-selftest") {
        duckle_desktop_lib::self_update_selftest();
    }
    #[cfg(feature = "update-selftest")]
    if std::env::args().any(|a| a == "--self-update-run") {
        duckle_desktop_lib::self_update_run_selftest();
    }
    // `duckle serve [...]` and `duckle run [...]` delegate to the embedded
    // headless runner. Anything else is a GUI launch.
    if duckle_desktop_lib::run_headless_cli() {
        return;
    }
    duckle_desktop_lib::run();
}
