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

## M6 SCIP goldens and index compatibility

The M3 heuristic goldens remain as regression references. New `*-scip.flow.yaml`
files capture the official-indexer variants:

- TypeScript control: the declared `send` function is now resolved; `do` becomes
  `call`, its ID becomes `call_send`, and target/symbol identify its SCIP
  declaration. There is no function body to inline. No other structure changes.
- Python and Java control: the call's SCIP symbol is added. Structure, IDs,
  source spans, and display targets are unchanged.
- Every SCIP variant changes the resolution header to `scip`. Fingerprints
  change because the symbol (and for TypeScript the kind/ID/target) is structural.
- The new dispatch golden covers an alias pointing through a barrel, and an
  interface with two implementations. SCIP inlines `normalize`, records the
  TypeScript standard-library package boundary for `trim`, and emits an ordered
  dispatch switch with ExactMatcher/FuzzyMatcher cases and W113. Heuristics
  cannot resolve the barrel or interface declaration.

Real committed indexes were produced by scip-typescript 0.4.0, scip-python 0.6.6,
and the scip-java 0.13.1 artifact. That Java artifact reports
`scip-java version 0.0.0-SNAPSHOT`; metadata preserves its actual version output.
The Python fixture supplies an explicit empty package environment; dependency
symbols for FastAPI are therefore not claimed. Java's newer typed range fields
are decoded from standard protobuf fields 8/9 because current scip bindings
require Rust 1.81; the permitted scip 0.5.2 bindings retain Rust 1.80 support.

Missing encoding follows the requested TypeScript UTF-16/otherwise UTF-8 rule.
Indexes from package-local `.wtflow/index` directories are rebased to repository
relative paths. Definitions and relationships are collected on load; document
call-site tables are decoded and cached on first lookup. Stale entry and target
files fall back to heuristics, omit SCIP symbols, and report W120.

## M8 lint and Mermaid golden artifacts

Added `.lint.txt` and `.mmd` companions for every existing flow golden, produced
by the CLI and reviewed with deterministic diff checks. Added `.mmd` exports of
the three M2 renderer snapshots so Docker syntax validation also covers error
handlers, transactions, parallel execution, and German labels. These additions
do not change any flow structure or existing snapshot. All twelve Mermaid files
were accepted by the minlag/mermaid-cli container using its bundled Puppeteer
configuration.

## Final requirements review

Finite C-style `for` statements use `for_each`; conditionless `for` loops use
`while`. Named Flask methods and schedule arguments are extracted from their
annotations. Symbol-only rules require a SCIP answer and cannot accidentally
match a heuristic display name. These corrections add regression coverage and
do not alter the existing goldens.

### Call text containing comments and multiline literals

Call/condition text now removes AST-recognized comments before whitespace
normalization. This prevents apostrophes in comments from being interpreted as
string delimiters, and prevents flattened `//` comments from swallowing later
arguments. Actual newlines inside literals are displayed as `\n`/`\r` so single
step code remains schema-valid; collapsed `do` blocks retain their block format.
Source locations and definition documentation still refer to the original code.
Comment wording changes no longer change these nodes' fingerprints. Existing
goldens do not contain the affected syntax and are unchanged.

## CLI and TUI parity

The terminal workspace and CLI submit the same typed commands to the shared
application dispatcher. TUI action forms derive options and validation from
Clap. Configuration, extraction, artifact generation, and saved-flow discovery
are shared. Catalog coverage and fixture artifact/diagnostic comparisons run in
`cargo test -p wtflow-cli`; terminal layout, cancellation, and CLI compatibility
have additional regression tests. New domain capabilities must remain available
through both frontends. See [the workspace guide](docs/tui.md#maintaining-clitui-parity).
