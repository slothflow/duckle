# Contributing to Duckle

Thanks for your interest in Duckle. This project is in early development; contributions, issues, and design discussion are all welcome.

## Prerequisites

- **Rust** stable, installed via [rustup](https://rustup.rs). The repository pins the toolchain in `rust-toolchain.toml`.
- **Node.js 20+** and **npm 10+**.
- **Tauri 2 system prerequisites** - see https://tauri.app/start/prerequisites for your OS. On Windows this means MSVC build tools and WebView2.

## First-time setup

```sh
# install frontend dependencies
npm --prefix frontend install

# build the workspace (compiles every crate)
cargo build --workspace

# run the desktop app
cargo run -p duckle-desktop
```

The desktop app launches Vite's dev server automatically and opens a Tauri window pointing at it.

## Repository layout

See [ARCHITECTURE.md](./ARCHITECTURE.md). In short:

- `apps/desktop/` - Tauri 2 shell.
- `crates/` - Rust crates for runtime, connectors, engines, workflow, scheduling, plugins.
- `frontend/` - React + TypeScript UI.

## Style and conventions

- **Rust**: `cargo fmt` and `cargo clippy --workspace --all-targets -- -D warnings` must pass.
- **TypeScript**: 2-space indent, single quotes, trailing commas. Run `npm --prefix frontend run lint` before pushing.
- **Commits**: small, atomic, and self-explanatory. Use imperative subject lines (`Add Parquet source connector`, not `Added` or `Adding`).
- **Comments**: only when the *why* is non-obvious. Don't restate what the code already says.

## Tests

- **Unit tests** live alongside the code (`#[cfg(test)] mod tests` in Rust; co-located `*.test.ts` in the frontend).
- **Integration tests** for crates that need them live under `crates/<name>/tests/`.
- Run everything with `cargo test --workspace`.

## Adding a connector

1. Add a new module under `crates/connectors/src/`.
2. Implement the `Connector` trait from `plugin-sdk`.
3. Register the connector in `crates/connectors/src/lib.rs`.
4. Add an integration test under `crates/connectors/tests/`.
5. Add a corresponding node type in `frontend/src/canvas/nodes/`.

## Adding a transform

1. Add a new module under `crates/transform-engine/src/ops/`.
2. Implement the `Transform` trait from `plugin-sdk`.
3. Register in the transform registry.
4. Add a node type and properties panel in the frontend.

## Legal

By contributing, you agree your contribution is dual-licensed under MIT and Apache-2.0, as the rest of the project is.

Do not paste or port code from incompatibly licensed sources. If you draw inspiration from another project, that is fine - but write the implementation from scratch.
