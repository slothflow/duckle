# API Ingestion Patterns

Status: skeleton.

Purpose: document REST, GraphQL, SOAP/OData, and SaaS ingestion patterns.

Expected coverage:

- `src.rest` response paths.
- Cursor, offset, page, Link, and next-url pagination.
- Auth headers, bearer tokens, API keys.
- GraphQL request/response shape.
- SaaS aliases as REST/GraphQL wrappers.
- Normalizing nested JSON after ingestion.

Relevant node docs:

- `docs/01_nodes/01_source-node-contracts.md`
- `docs/01_nodes/02_transform-node-contracts.md`

TODO:

- Add generic paginated REST workflow.
- Add GraphQL workflow.
- Add SaaS wrapper examples.
