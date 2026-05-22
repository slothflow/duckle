@echo off
REM Launch Duckle in development mode (Vite dev server + Tauri shell
REM together). Do NOT use `cargo run -p duckle-desktop` on its own -
REM that only starts the Rust shell and the window shows
REM "localhost refused to connect" because Vite isn't running.
cd /d "%~dp0apps\desktop"
cargo tauri dev
