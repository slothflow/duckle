# Orchestration and Jobs

This note documents child pipelines, jobs, iteration, foreach, parallel branches, checkpoints, and side-effect orchestration.

Use this with:

- `docs/01_nodes/05_control-and-code-node-contracts.md`
- `docs/02_workflows/00_workflow-design-principles.md`
- `docs/02_workflows/11_debugging-failed-workflows.md`

## Current Model

The current orchestration model is side-effect oriented.

| Control node | Current behavior |
|---|---|
| `ctl.runpipeline`, `ctl.trigger`, `ctl.runjob` | Runs a child pipeline/job as a side effect, then passes parent rows through or emits a placeholder. Child output is not composed back into the parent. |
| `ctl.iterate` | Runs a child pipeline N times with `${ITER_INDEX}` substitution. |
| `ctl.foreach` | Runs a child pipeline once per upstream row with `${ITER_INDEX}` and `${ITER_ITEM_<FIELD>}` substitution. |
| `ctl.parallelize` | Snapshots upstream and runs independent downstream branches concurrently. |
| `ctl.checkpoint` | Writes a durable parquet artifact while passing rows through. |

Agent rule: do not design parent workflows that depend on child output rows returning to the parent unless that composition is explicitly implemented later.

## Master Job Pattern

Use this to run several workflows in order.

```text
ctl.runjob(load_dimensions)
  -> ctl.runjob(load_facts)
  -> ctl.runjob(run_quality_report)
```

Good for:

- Local studio bootstrap.
- Multi-step batch process.
- Running child pipelines with clear boundaries.

Notes:

- Each child runs in its own temp DB context.
- Use context variables to pass run parameters.
- If child outputs are needed later, write them to durable artifacts such as Parquet or DuckDB, then read them in the next child.

## Child Output Handoff Pattern

Because child output is not composed into the parent, use file/database artifacts:

```text
parent:
  ctl.runjob(child_extract)
  -> ctl.runjob(child_model)

child_extract:
  source -> transform -> snk.parquet(/tmp/stitchly/orders.parquet)

child_model:
  src.parquet(/tmp/stitchly/orders.parquet) -> transform -> sink
```

Prefer stable workspace-relative paths once the workspace artifact convention is finalized.

## For Each Pattern

Use when an upstream table drives repeated child executions.

```text
src.csv(work_items)
  -> ctl.foreach(child_pipeline, concurrency=1)
```

Child pipeline can use substitutions:

```text
${ITER_INDEX}
${ITER_ITEM_CUSTOMER_ID}
${ITER_ITEM_REGION}
```

Good for:

- Per-tenant extract.
- Per-file processing.
- Per-database/table sync.

Notes:

- Start with `concurrency=1`.
- Increase concurrency only when child side effects are safe.
- Each row runs the child in isolation.

## Iterate Pattern

Use when a child pipeline should run a fixed number of times.

```text
ctl.iterate(child_pipeline, count=10)
```

Child pipeline can use:

```text
${ITER_INDEX}
```

Good for:

- Simple batch loops.
- Page/index experiments.
- Repeated test runs.

If the loop depends on data values, use `ctl.foreach` instead.

## Parallel Branch Pattern

Use when several downstream branches can run independently from the same upstream snapshot.

```text
source -> transform -> ctl.parallelize
                         |-> branch A -> sink A
                         |-> branch B -> sink B
                         |-> branch C -> sink C
```

Notes:

- Upstream is snapshotted once.
- Branches run in isolated execution contexts.
- Any branch failure should fail the parallel node.
- External side effects still need careful design. Parallel writes to the same target are risky.

## Checkpoint Pattern

Use checkpoints around expensive or risky boundaries.

```text
source -> expensive_transform -> ctl.checkpoint -> external_sink
```

Benefits:

- Re-run downstream work without re-reading/recomputing upstream.
- Inspect intermediate data.
- Create handoff artifacts between child jobs.

Prefer Parquet checkpoints for typed local artifacts.

## Bootstrap Pattern

Use `code.shell` for CLI setup, then native graph nodes for data.

```text
code.shell(check_or_init_repo)
  -> code.shell(start_or_verify_service)
  -> ctl.runjob(load_from_service)
```

For Dolt-style workflows:

```text
code.shell(dolt bootstrap)
  -> src.mysql(read Dolt SQL server)
  -> transforms
  -> local sink
```

Rules:

- Shell should set up or verify environment.
- Data movement should use source/sink nodes when possible.
- Capture shell stdout/stderr with downstream logging/assertions.

## Failure Handling

| Need | Pattern |
|---|---|
| Run cleanup/notification on later failure | `ctl.try(fallbackPipelineRef)` before risky section |
| Retry flaky stage | Advanced node props: `retryAttempts`, `retryBackoffMs` |
| Stop on bad branch | `ctl.die` |
| Save rejects | `ctl.deadletter` |
| Log counts | `xf.count -> ctl.log` |

## Agent Rules

- Use child jobs for process boundaries, not for normal row-level transforms.
- Use durable artifacts for handoff between child jobs.
- Keep side-effecting branches explicit and late in the workflow.
- Use `ctl.foreach` only when each row genuinely needs isolated child execution.
- Use `ctl.parallelize` for independent branches, not for sharing mutable external targets.
- Add `ctl.checkpoint` before expensive runtime/API/sink stages.
- Treat `ctl.try` as fallback-on-failure, not full try/catch continuation.

## TODO

- Add real master-job workflow JSON once saved examples exist.
- Document workspace-relative artifact path conventions.
- Add tested `ctl.foreach` substitution example.
- Add tested `ctl.parallelize` branch example.
