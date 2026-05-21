// Windows: hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    duckle_desktop_lib::run();
}
