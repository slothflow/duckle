# Workflow Design Principles

This note defines the default workflow design approach for Stitchly v2/Duckle graphs.

Use this with:

- `docs/01_nodes/00_node-inventory.md`
- `docs/01_nodes/01_source-node-contracts.md`
- `docs/01_nodes/02_transform-node-contracts.md`
- `docs/01_nodes/03_sink-node-contracts.md`
- `docs/01_nodes/04_quality-node-contracts.md`
- `docs/01_nodes/05_control-and-code-node-contracts.md`

## Default Shape

Most useful workflows follow this shape:

```text
source -> normalize/shape -> validate -> enrich/model -> sink
```

For local-studio work, prefer:

```text
source -> transform -> quality gate -> parquet/duckdb checkpoint
```

Then use later workflows to publish or sync outward.

## Design Rules

| Rule | Reason |
|---|---|
| Prefer built-in nodes before custom code. | Built-ins are easier to inspect, test, and evolve. |
| Prefer DuckDB-native paths for local workflows. | They are fast, deterministic, and fit the current CLI execution model. |
| Add quality gates before external side effects. | Bad rows should fail or dead-letter before reaching external systems. |
| Keep report branches separate from row-preserving branches. | Profiling/reconciliation nodes often emit reports, not original rows. |
| Add checkpoints at useful boundaries. | Parquet/DuckDB artifacts make debugging and reruns easier. |
| Use `code.shell` only for CLI-native work. | Shell is powerful but less structured than graph-native nodes. |
| Treat sinks as side effects. | Once a sink runs, external state may have changed. |

## Branching Patterns

### Validation With Rejects

```text
source -> qa.notnull -> transform -> sink
              |
              reject -> ctl.deadletter
```

Use this when bad rows should be preserved for inspection.

### Validation Gate

```text
source -> transform -> qa.contract -> sink
```

Use this when a bad dataset should stop the whole run.

### Debug Branch

```text
source -> transform -> sink
              |
              -> xf.count -> ctl.log
```

Use this when a workflow needs visible row counts without changing the main branch.

### Report Branch

```text
source -> transform -> sink
              |
              -> qa.profile -> snk.json
```

Use this when a profile/reconciliation/check should be captured separately from the main data flow.

## Node Selection Heuristics

| Need | Prefer |
|---|---|
| Read files/local artifacts | `src.parquet`, `src.csv`, `src.duckdb`, `src.sqlite` |
| Persist local result | `snk.parquet`, `snk.duckdb`, `snk.sqlite` |
| Inspect/debug | `xf.log`, `xf.count`, `qa.describe`, `qa.profile` |
| Block bad rows | `qa.contract`, `xf.assert`, `ctl.die` |
| Preserve bad rows | Validator reject port -> `ctl.deadletter` |
| External API read | `src.rest`, `src.graphql`, SaaS aliases |
| External API write | `snk.rest`, `snk.webhook`, `snk.graphql` |
| CLI bootstrap/setup | `code.shell` |
| Child workflow orchestration | `ctl.runjob`, `ctl.runpipeline`, `ctl.foreach`, `ctl.iterate` |

## Agent Rules

- Start from the workflow goal and data boundary: what comes in, what must be true, what goes out.
- Identify side effects early. Put validation and logging before them.
- Use the node contract docs to check whether each node preserves rows or emits a report.
- When generating workflow JSON, verify prop names in planner/runtime code, not only the UI manifest.
- Keep workflows small enough to debug. Use child jobs for orchestration rather than one large graph.
- Prefer durable intermediate files over implicit state when designing early Stitchly v2 workflows.
