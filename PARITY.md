# Parity record

No Go prototype was found under `/Users/thomas/projects` during bootstrap. The
Rust fixtures and expected documents will therefore be authored from the build
specification; they do not establish byte parity against an unavailable prototype.

## Format clarification

The user approved `[]` for empty lists. Nonempty lists use block notation except
for aggregate state reads/writes and boundaries. This is necessary because YAML
has no empty block-sequence notation.
