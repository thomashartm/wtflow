# Acceptance ledger

Each milestone must pass its checks and be committed before the next begins.

- M1: core IR, canonical YAML, strict loader, fingerprints, IDs, embedded schemas.
- M2: lint rules and Mermaid rendering with unit and snapshot coverage.
- M3: heuristic extraction for TS, Python, Java, Camel; authored fixtures/goldens.
- M4: CLI, labels/update/todo, source checking, process integration tests.
- M5: official indexer orchestration, Docker image, metadata and freshness.
- M6: SCIP resolution, encoding/dispatch tests, committed indexes and goldens.
- M7: criterion benchmarks and measured performance acceptance.
- M8: CI, release builds, consumer workflow, flow-docs skill.
- M9: context packets and glossary, label-cache design; no LLM calls.
- M10: optional service map; not part of the required implementation.

## Evidence

Work in progress. A milestone is complete only when its acceptance evidence is
recorded here and its commit exists.

### M1

Implemented all five workspace crates (four later-stage crates are skeletons),
IR and document-order traversal, canonical emitter/strict loader, ordered
structural fingerprint serializer, deterministic IDs, and both embedded Draft
2020-12 schemas. The user approved empty-list `[]` notation.

Acceptance: eight core tests, including a committed canonical golden, exact
fingerprint JSON, label/source invariance, invalid kind/child-list rejection,
strict config validation, scalar quoting, multiline code and ID uniqueness.
`cargo test --workspace --locked`, formatting, and clippy with warnings denied
pass using Rust 1.80.1 on macOS arm64. No prototype parity is claimed.
