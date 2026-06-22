# AI and RAG Workflows

Status: skeleton.

Purpose: document safe and repeatable AI/RAG graph patterns.

Expected coverage:

- Chunk -> embed -> vector sink.
- Vector search and full-text search.
- LLM transform sampling and cost controls.
- PII redaction and classification.
- Local vs API-backed AI transforms.

Relevant node docs:

- `docs/01_nodes/02_transform-node-contracts.md`
- `docs/01_nodes/03_sink-node-contracts.md`

TODO:

- Add chunk/embed/vector sink example.
- Add local PII redaction example.
- Add LLM transform guardrails.
