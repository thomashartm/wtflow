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
