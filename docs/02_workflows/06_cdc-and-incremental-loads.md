# CDC and Incremental Loads

Status: skeleton.

Purpose: document incremental, CDC, SCD, row-hash, and audit-stamp workflow patterns.

Expected coverage:

- `xf.incremental` high-water mark.
- `src.ducklake.changes`.
- `src.ducklake.diff`.
- `xf.cdc.diff`, `xf.cdc.scd1`, `xf.cdc.scd2`, `xf.cdc.scd3`, `xf.cdc.upsert`.
- `xf.row_hash` and `xf.audit`.
- Upsert sinks.

Relevant node docs:

- `docs/01_nodes/01_source-node-contracts.md`
- `docs/01_nodes/02_transform-node-contracts.md`
- `docs/01_nodes/03_sink-node-contracts.md`

TODO:

- Add timestamp watermark example.
- Add DuckLake CDC example.
- Add SCD2 dimension example.
