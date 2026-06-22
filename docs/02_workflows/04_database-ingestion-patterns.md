# Database Ingestion Patterns

This note documents database and warehouse ingestion into local DuckDB, Parquet, or lakehouse outputs.

Use this with:

- `docs/01_nodes/01_source-node-contracts.md`
- `docs/01_nodes/02_transform-node-contracts.md`
- `docs/01_nodes/03_sink-node-contracts.md`
- `docs/02_workflows/05_quality-gates-and-deadletters.md`

## Default Pattern

```text
database source -> project/cast/audit -> quality gate -> local artifact
```

Recommended local-studio graph:

```text
src.postgres/src.mysql/src.sqlserver
  -> xf.project
  -> xf.audit
  -> qa.contract
  -> snk.duckdb or snk.parquet
```

## Source Runtime Families

| Family | Nodes | Notes |
|---|---|---|
| DuckDB attach sources | `src.postgres`, `src.mysql`, `src.mariadb`, `src.cockroach`, `src.redshift`, `src.motherduck`, `src.ducklake`, `src.quack`, `src.pgvector` | Fast and DuckDB-native. Best when supported by extensions and credentials. |
| Runtime SQL clients | `src.sqlserver`, `src.synapse`, `src.oracle`, `src.clickhouse`, `src.cassandra`, `src.scylla` | Runtime materializes query result into DuckDB. |
| API-backed warehouses | `src.snowflake`, `src.databricks` | Uses vendor SQL APIs, not native drivers. |
| Local databases | `src.duckdb`, `src.sqlite` | Best for fixtures, local artifacts, and tests. |
| Generic driver | `src.adbc` | Requires external ADBC driver library. |

## Query vs Table Mode

Prefer table mode when:

- The whole table is small enough for local work.
- You want simple reproducible ingestion.
- You can filter downstream in DuckDB.

Prefer query mode when:

- Source table is large.
- You need source-side filtering.
- You need joins/projections that the remote DB can do more efficiently.
- You want stable ordering for incremental/high-water mark extraction.

Example query-mode shape:

```sql
SELECT id, updated_at, status, amount
FROM orders
WHERE updated_at >= '2026-01-01'
```

## Postgres/MySQL to Local DuckDB

```text
src.postgres or src.mysql
  -> xf.project
  -> xf.cast
  -> xf.audit
  -> qa.contract
  -> snk.duckdb
```

Use this for:

- Local analytics extracts.
- Fixture creation.
- Building local-studio snapshots from production-like sources.

Notes:

- `src.postgres` and `src.mysql` are DuckDB attach-backed.
- Use `xf.project` to keep only the columns needed downstream.
- Use `xf.audit` to record load metadata.
- Use `snk.parquet` instead of `snk.duckdb` when the artifact is a handoff/checkpoint.

## Dolt SQL Server Pattern

Dolt speaks the MySQL wire protocol, so the expected v2 path is:

```text
code.shell(optional bootstrap/start server)
  -> src.mysql
  -> transforms
  -> snk.duckdb/snk.parquet
```

Use `code.shell` for Dolt-native CLI steps:

- Check repo exists.
- Initialize/clone repo.
- Start or verify Dolt SQL server.
- Run `dolt status`, `dolt branch`, or setup SQL scripts.

Then use `src.mysql` for actual table reads.

Important caveat: this needs a tested Dolt SQL Server config before becoming a foundation workflow. Capture host, port, database, user, and startup assumptions in `07_dolt-bootstrap-and-sqlserver-patterns.md`.

## SQL Server and Synapse Pattern

```text
src.sqlserver or src.synapse
  -> xf.project
  -> xf.audit
  -> qa.contract
  -> snk.parquet
```

Notes:

- Source path is runtime-backed via TDS.
- Required props include host, user, password, database, and query or table/schema.
- Use `trustCert` for self-signed local/dev SQL Server when appropriate.
- Write to Parquet first if downstream publication is separate.

## Snowflake and Databricks Pattern

```text
src.snowflake/src.databricks
  -> xf.project
  -> qa.contract
  -> snk.parquet
```

Notes:

- These are API-backed SQL sources.
- Keep query results bounded.
- Prefer remote-side filters/projections.
- Add `xf.count -> ctl.log` after ingestion for observability.

## Local Database Fixture Pattern

Use local DB sources to test workflows without external services:

```text
src.duckdb/src.sqlite -> transform -> qa.contract -> snk.parquet
```

Use local DB sinks to create stable fixtures:

```text
src.csv -> transform -> snk.duckdb
```

## Incremental Database Extract

```text
src.postgres/query
  -> xf.incremental
  -> xf.audit
  -> snk.parquet
```

Notes:

- `xf.incremental` filters rows past the persisted high-water mark.
- Use a monotonic timestamp or id.
- Persist to Parquet/DuckDB before pushing to external sinks.
- Add `xf.row_hash` when later diff/upsert is needed.

## Agent Rules

- Use query mode to reduce data before it enters local DuckDB.
- Prefer local Parquet/DuckDB outputs as the first sink during development.
- Add `qa.contract` before any external write-back sink.
- For Dolt, use `code.shell` only for bootstrap/server lifecycle and `src.mysql` for table data.
- Treat API-backed warehouses as slower/costlier than local sources; project/filter early.
- Check runtime class before debugging. DuckDB attach failures differ from Rust runtime/API failures.

## TODO

- Add tested Postgres -> DuckDB workflow JSON.
- Add tested Dolt SQL Server -> DuckDB workflow JSON.
- Add SQL Server local-dev fixture workflow.
- Document connection storage/secrets once the local studio connection model is finalized.
