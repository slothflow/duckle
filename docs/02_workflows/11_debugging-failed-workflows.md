# Debugging Failed Workflows

This note documents a pragmatic debug loop for failed Stitchly v2/Duckle workflows.

Use this with:

- `docs/01_nodes/00_node-inventory.md`
- `docs/01_nodes/05_control-and-code-node-contracts.md`
- `docs/00_foundation/02_local-studio-quickstart.md`
- `docs/00_foundation/03_duckdb-cli-execution-model.md`

## First Pass

1. Identify the failing node id and `component_id`.
2. Check whether the node is SQL-backed or runtime-backed.
3. Check whether the failure is config, missing input, column binding, external dependency, credential, or side-effect failure.
4. Inspect upstream row shape with `qa.describe`, `xf.count`, or preview.
5. Add explicit `ctl.log` or `ctl.deadletter` branches if the failure is hidden.

## Failure Categories

| Symptom | Likely cause | Next check |
|---|---|---|
| Missing input | Wrong edge/port wiring | Inspect graph ports and node inputs. |
| Column not found | Upstream schema changed or prop references wrong column | Check `plan/graph.rs` validation and upstream preview. |
| DuckDB binder/parser error | Generated SQL or custom SQL issue | Inspect node props and builder branch. |
| Extension load failure | DuckDB extension unavailable or network blocked | Check source/sink runtime class and extension prelude. |
| Credential/auth failure | External source/sink/API issue | Verify props, saved connection, token, host. |
| Empty output | Filter/pagination/offset/query too strict | Add `xf.count`, preview earlier node, lower filters. |
| Sink wrote wrong data | Quality gate missing or branch shape wrong | Add `qa.contract`, `xf.count`, checkpoint before sink. |
| Runtime branch failed | Rust runtime spec or external client issue | Inspect `plan/mod.rs` spec and `lib.rs` executor path. |

## Debug Nodes

| Need | Node |
|---|---|
| Count rows | `xf.count` |
| Show/pass rows | `xf.log` |
| Log message/count | `ctl.log`, `ctl.warn` |
| Inspect schema | `qa.describe` |
| Inspect distribution | `qa.profile`, `qa.histogram` |
| Persist intermediate | `ctl.checkpoint`, `snk.parquet`, `snk.duckdb` |
| Stop intentionally | `ctl.die`, `xf.assert`, `qa.contract` |
| Capture bad rows | `ctl.deadletter` |

## SQL-Backed Node Debugging

For SQL-backed nodes:

1. Find the `component_id` branch in `builders.rs`.
2. Check required props and generated expression shape.
3. Check whether the node needs an extension prelude.
4. Reproduce with a minimal upstream if possible.
5. Add `qa.describe` before and after shape-changing transforms.

## Runtime-Backed Node Debugging

For runtime-backed nodes:

1. Find the planner spec in `plan/mod.rs`.
2. Check required props and default values.
3. Find the executor handler in `lib.rs`.
4. Check external dependencies: binaries, API tokens, network, local files, service ports.
5. Add a checkpoint before the runtime node if the upstream data is expensive to recreate.

## Agent Rules

- Do not patch a workflow blindly. First classify the node runtime and failure type.
- If a sink fails, insert a checkpoint immediately before it.
- If an API/source returns empty rows, verify response path and pagination before changing transforms.
- If downstream columns fail, inspect the nearest shape-changing upstream node.
- Keep debug branches in the workflow until the pattern stabilizes, then remove or convert them to logs/contracts.

## TODO

- Add run-log file paths once confirmed.
- Add examples of generated SQL inspection.
- Add common DuckDB extension errors and fixes.
