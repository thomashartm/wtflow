---
name: flow-docs
description: Extract, label, check, and render source-derived flow documentation with wtflow. Use for repository flow documents, not general architecture diagrams.
---

Use the repository's `.wtflow/config.yaml` for ownership, call rules and enabled indexers.
Run `wtflow index` before extracting or updating flows. Indexing needs the enabled
language's official indexer and project dependencies; extraction/check/render read
the resulting SCIP files and never launch indexers. If indexing fails, report the
failure. Use heuristic-only output only when it fits the user's requested scope,
and identify the resolution limitation.

Discover entries with `wtflow entrypoints [--json] DIR`. Prefer
`wtflow extract --entry FILE#SYMBOL --resolver auto --depth 2 -o FLOW.flow.yaml`.
Use `wtflow update FLOW...` for existing documents or `extract --merge OLD` to
preserve labels where both ID and code remain unchanged.

Structure belongs to the AST. Never edit IDs, code, branches, boundaries, symbols,
source references or fingerprints to make a diagram look better. For clearer
wording, inspect `wtflow todo --json --context FLOW` for available SCIP documentation and
repository glossary, prepare a flat `id: label` mapping,
and apply it with `wtflow label FLOW LABELS.yaml`. Unknown IDs and fingerprint
mismatches must be resolved before labeling; do not recompute a fingerprint to
hide a manual structural edit.

Run `wtflow check --source FLOW...`, then render with
`wtflow render --lang en -o FLOW.mmd FLOW.flow.yaml` (or `de` and `.md`). Report
errors and warnings, specifically I002 unresolved-call counts, W113 ambiguous
dispatch, and W120 stale indexes. Use `--verbose` to identify unresolved nodes.
Do not describe a static dispatch set as proof of runtime DI wiring.

Inspect the resulting flow and diff. Keep generated structure, separately applied
labels, and rendered diagrams together in the requested repository location.
Do not run LLM labeling calls, publish artifacts, or modify source behavior as
part of documenting a flow unless the user requests that additional work.
