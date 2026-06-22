# Quality Gates and Dead Letters

This note documents workflow patterns for validation, reject handling, contracts, and dead-letter outputs.

Use this with:

- `docs/01_nodes/04_quality-node-contracts.md`
- `docs/01_nodes/05_control-and-code-node-contracts.md`

## Core Patterns

### Gate Before Sink

Use when any rule failure should stop the load.

```text
source -> transform -> qa.contract -> sink
```

Good for scheduled loads, CI checks, and external writes.

### Reject Branch to Dead Letter

Use when bad rows should be preserved.

```text
source -> qa.notnull -> transform -> sink
              |
              reject -> ctl.deadletter
```

Use `ctl.deadletter` with JSON, CSV, or Parquet output.

### Reject Branch That Fails the Run

Use when bad rows should be saved and also fail the run.

```text
source -> qa.regex -> transform -> sink
             |
             reject -> ctl.deadletter
             reject -> xf.count -> ctl.die(has-rows)
```

The `xf.count` -> `ctl.die` branch makes the failure condition explicit.

### Scorecard Branch

Use when the main data should continue but quality metrics should be captured.

```text
source -> transform -> sink
              |
              -> qa.expect -> snk.json
```

`qa.expect` emits one scorecard row per rule.

### Freshness Gate

Use before loads that must meet an SLA.

```text
source -> qa.freshness(mode=gate) -> sink
```

Report mode emits a scorecard row instead of gating.

## Node Selection

| Need | Use |
|---|---|
| Required fields | `qa.notnull` |
| Format checks | `qa.regex` |
| Numeric/date bounds | `qa.range` |
| Unique business key | `qa.unique` |
| Declarative scorecard | `qa.expect` |
| Declarative gate | `qa.contract` |
| Freshness SLA | `qa.freshness` |
| Save invalid rows | `ctl.deadletter` |
| Fail on invalid rows | `ctl.die` |
| Log counts | `xf.count` + `ctl.log` |

## Agent Rules

- Decide whether bad rows should be dropped, saved, or fail the run.
- Do not put report-producing nodes inline before sinks that expect original rows.
- Use reject branches for row-level failures.
- Use `qa.contract` or `xf.assert` for run-level gates.
- Add a visible dead-letter path before external sinks.
- Prefer Parquet for dead-letter data that will be inspected/reprocessed later.

## TODO

- Add concrete workflow JSON once saved examples exist.
- Document exact reject edge shape from persisted workflow files.
- Add examples for combined not-null + regex + uniqueness validation.
