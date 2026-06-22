# Workflow JSON and Graph Shape

This note documents how to reason about the workflow graph shape. It is intentionally implementation-oriented for agents.

Authoritative code to inspect:

- Frontend types: `frontend/src/pipeline-types.ts`
- Persistence/workspace handling: `frontend/src/persistence.ts`, `frontend/src/workspace.ts`
- Planner graph compiler: `crates/duckdb-engine/src/plan/graph.rs`
- Planner stage builder: `crates/duckdb-engine/src/plan/mod.rs`
- SQL/runtime builders: `crates/duckdb-engine/src/plan/builders.rs`

## Core Concepts

| Concept | Meaning |
|---|---|
| Node | A component instance with an id, label, `component_id`, props, and optional schema. |
| Component id | The node type, e.g. `src.csv`, `xf.filter`, `qa.contract`, `snk.parquet`. |
| Edge | A connection from one node output to another node input. |
| Port | Named input/output branch. Common input is `main`; reject output commonly maps to `<node>__reject`. |
| Props | Node-specific configuration consumed by the planner/runtime. |
| Schema | Optional declared columns used by preview, validation, and some readers. |
| Stage | The planner's executable representation of a node. |

## Graph Shape Rules

- Most sources have no input edges.
- Most transforms have one `main` input and one main output.
- Joins, lookup, reconciliation, SCD, and reference-integrity nodes need a second/reference input.
- Validators may expose a reject branch.
- Sinks require a `main` input and are normally terminal.
- Control nodes may pass through, branch, run side effects, or create placeholders.

## Ports

| Port pattern | Meaning |
|---|---|
| `main` input | Primary input relation for a transform/sink/control node. |
| lookup/reference input | Secondary input for joins, lookup, QA reference checks, SCD, reconciliation. |
| main output | Normal downstream relation. |
| reject output | Invalid/unmatched rows, usually materialized as `<node>__reject`. |
| switch outputs | `ctl.switch` creates one relation per branch/default. |

Agent rule: when a workflow fails with missing input or wrong columns, check ports before changing node props.

## Props Are Runtime Contracts

The UI manifest gives a useful form contract, but the planner/runtime branch is the executable contract.

Before emitting raw workflow JSON for a node, inspect:

1. `manifest-synth.ts` for intended fields.
2. `plan/builders.rs` for SQL-backed nodes.
3. `plan/mod.rs` for runtime-backed nodes.
4. `plan/specs.rs` for runtime spec fields.

Examples of known prop-name caveats:

- Some source UI fields are generic while runtime branches expect specific names.
- `src.git` runtime currently expects a local `repo`, while the UI shape exposes URL-like fields.
- `src.xml` runtime expects `rowPath`, while the UI shape has older XML naming.
- `src.email` runtime expects `user`/`mailbox`, while the UI labels may differ.

## Schema Inference and Validation

The planner performs lightweight schema propagation:

- Exact pass-through transforms can preserve column validation.
- `xf.drop`, `xf.rename`, and `xf.project` can derive new column sets.
- Shape-changing nodes usually return unknown schema to avoid false errors.

Unknown schema is not failure. It means downstream validation may be weaker.

## Agent Workflow-Building Process

1. Choose source nodes from `01_source-node-contracts.md`.
2. Add normalization transforms from `02_transform-node-contracts.md`.
3. Add validation/contract nodes from `04_quality-node-contracts.md`.
4. Add sinks from `03_sink-node-contracts.md`.
5. Add `xf.count`, `xf.log`, or `qa.describe` branches for debug visibility.
6. Verify side-effect nodes are after quality gates.
7. Verify raw props against planner/runtime code before finalizing.

## Minimal Graph Pattern

```text
src.csv -> xf.project -> qa.contract -> snk.parquet
```

Useful debug branch:

```text
xf.project -> xf.count -> ctl.log
```

Reject branch:

```text
qa.notnull reject -> ctl.deadletter
```

## TODO

- Add a real workflow JSON example from a saved local pipeline.
- Document exact node/edge JSON schema once we inspect saved pipeline files.
- Add examples for reject-port wiring and lookup/reference-port wiring.
