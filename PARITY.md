# Parity record

No Go prototype was found under `/Users/thomas/projects` during bootstrap. The
Rust fixtures and expected documents will therefore be authored from the build
specification; they do not establish byte parity against an unavailable prototype.

## Format clarification

The user approved `[]` for empty lists. Nonempty lists use block notation except
for aggregate state reads/writes and boundaries. This is necessary because YAML
has no empty block-sequence notation.

## M2 authored renderer snapshots

The English/German branch snapshots and switch/try/parallel snapshot establish
expected output from the specification, not Go parity. Catch and finally nodes
are outside the try-body subgraph so the dotted error edge does not point into
its own containing subgraph. Duplicate frontier edges are emitted once.

## M3 specification-based extraction

The four new goldens (TypeScript, Python, Java controls and Camel events) were
written from explicit IR expectations before the adapters. Their authoring
source is `crates/wtflow-core/examples/author_m3_goldens.rs`. Tests include a
helper masking fingerprint/resolution headers for future Go comparisons.

No Go golden could be compared. The expected fingerprint is the Rust v1 digest;
resolution is `heuristic`. Source snippets normalize whitespace outside quoted
literals, omit trailing statement semicolons, and preserve literal contents.
A call directly in a return expression is visited before the terminal, allowing
controller-to-service inlining. I/O nested in a call's arguments remains outside
the analysis. Statement-free catch bodies are represented as `(error ignored)`.
Camel metadata calls (`from`, `routeId`) are reflected in flow headers; only
behavioral DSL operations become steps.
