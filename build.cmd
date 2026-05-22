@echo off
REM Produce a packaged Duckle build: compiles the frontend, bundles it
REM INTO the app (no localhost / Vite dependency), and emits the
REM installer + standalone exe under
REM   apps\desktop\target\release\bundle\
REM This is what end users get - double-click to run, fully offline.
cd /d "%~dp0apps\desktop"
cargo tauri build
