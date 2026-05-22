# Launch Duckle in development mode.
#
# Starts the Vite dev server AND the Tauri desktop shell together, then
# opens the app window pointed at the dev server. This is the correct
# way to run Duckle while developing - do NOT use `cargo run -p
# duckle-desktop` on its own, that only starts the Rust shell and the
# window will show "localhost refused to connect" because Vite isn't up.
#
#   PS> .\dev.ps1
$ErrorActionPreference = 'Stop'
Set-Location "$PSScriptRoot\apps\desktop"
cargo tauri dev
