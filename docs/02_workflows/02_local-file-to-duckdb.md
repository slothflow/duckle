# Local File to DuckDB

This note documents the standard local-studio pattern for reading local files, shaping them, validating them, and writing durable DuckDB/Parquet outputs.

Use this with:

- `docs/01_nodes/01_source-node-contracts.md`
- `docs/01_nodes/02_transform-node-contracts.md`
- `docs/01_nodes/03_sink-node-contracts.md`
- `docs/01_nodes/04_quality-node-contracts.md`
- `docs/02_workflows/00_workflow-design-principles.md`

## Default Pattern

```text
file source -> normalize columns -> quality gate -> local artifact
```

Recommended first local-studio graph:

```text
src.csv -> xf.project -> xf.cast -> qa.contract -> snk.duckdb
```

Recommended debug branch:

```text
xf.cast -> xf.count -> ctl.log
```

Recommended durable checkpoint:

```text
xf.cast -> ctl.checkpoint
```

## Source Choice

| Input | Prefer | Notes |
|---|---|---|
| CSV/TSV | `src.csv`, `src.tsv` | Supports delimiter/header/date/timestamp options. Use declared schema when types matter. |
| Parquet | `src.parquet` | Best for stable local artifacts and repeatable test fixtures. |
| JSON/JSONL | `src.json`, `src.jsonl` | Use `recordsPath` for nested API-style responses. |
| Excel | `src.excel` | Good for user-supplied spreadsheets. Requires DuckDB excel extension. |
| XML | `src.xml` | Runtime-backed parser. Verify `rowPath` prop when emitting raw JSON. |
| YAML/TOML | `src.yaml`, `src.toml` | Good for config-data ETL, not bulk data. |
| Fixed-width | `src.fixedwidth` | Good for banking/mainframe exports. Verify exact prop shape in builder. |

## Output Choice

| Output | Prefer | Why |
|---|---|---|
| Durable analytics file | `snk.parquet` | Compact, typed, fast to re-read. |
| Queryable local database | `snk.duckdb` | Best local studio artifact for iterative work. |
| Compatibility database | `snk.sqlite` | Useful for tools expecting SQLite. |
| Debug export | `snk.csv`, `snk.jsonl` | Easy to inspect outside the app. |
| Mid-graph artifact | `ctl.checkpoint` | Writes a parquet sidecar without changing the row flow. |

## Canonical CSV to DuckDB Flow

```text
src.csv
  -> xf.rename
  -> xf.cast
  -> xf.audit
  -> qa.contract
  -> snk.duckdb
```

Purpose of each stage:

| Stage | Purpose |
|---|---|
| `src.csv` | Read local file or glob. |
| `xf.rename` | Normalize column names to app/project convention. |
| `xf.cast` | Convert string-ish inferred columns into real types. |
| `xf.audit` | Add `_loaded_at`, `_source`, `_batch_id`. |
| `qa.contract` | Enforce required fields/ranges/unique keys before persistence. |
| `snk.duckdb` | Write a queryable local table. |

Useful additions:

- Add `xf.row_hash` before the sink if later diff/upsert workflows need stable fingerprints.
- Add `ctl.deadletter` from validator reject branches when bad rows should be saved.
- Add `xf.count -> ctl.log` after major shape-changing steps.

## Parquet-First Pattern

If upstream data is already typed or expensive to parse, use Parquet as the handoff format.

```text
src.parquet -> xf.project -> qa.contract -> snk.duckdb
```

Benefits:

- Faster iteration.
- Less type ambiguity than CSV/JSON.
- Good for checkpoints between workflows.

## Excel Import Pattern

```text
src.excel -> xf.rename -> xf.cast -> qa.notnull -> snk.parquet
```

Notes:

- Set `sheet` and `range` when possible.
- Cast intentionally after import; spreadsheet inference is often weak.
- Prefer writing to Parquet first, then loading Parquet into downstream models.

## JSON Normalization Pattern

```text
src.json -> xf.json.flatten -> xf.project -> xf.cast -> snk.parquet
```

Notes:

- Use `recordsPath` on `src.json` when rows live inside a nested object.
- Use `xf.json.flatten` when nested struct fields need top-level columns.
- Use `xf.project` after flattening to lock down the desired output contract.

## Quality Gate Pattern

```text
src.csv -> xf.cast -> qa.contract -> snk.duckdb
```

Example checks to encode in `qa.contract`:

- Required business key is not null.
- Numeric measures are non-negative.
- Status/category is in an allowed set.
- Date/timestamp columns are parseable and recent enough when paired with `qa.freshness`.

When bad rows should be retained:

```text
src.csv -> qa.notnull -> xf.cast -> snk.duckdb
              |
              reject -> ctl.deadletter
```

## Agent Rules

- Prefer `src.parquet` and `snk.parquet` for internal workflow handoffs.
- Use `snk.duckdb` when the next step is interactive querying/modeling.
- Normalize names and types early.
- Add audit columns before durable outputs.
- Put quality gates before sinks, not after.
- Do not rely on inferred CSV/Excel types for important workflows; cast explicitly.
- Add a checkpoint before slow external sinks or expensive downstream transforms.

## TODO

- Add real saved workflow JSON once a local pipeline is created.
- Document exact schema/autodetect UI behavior from a live pipeline.
- Add sample file fixtures under a stable test/demo folder.
