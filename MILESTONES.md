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

### M2

Implemented lint diagnostics E000-E006, W101-W107/W110/W112/W113/W120, and
I001/I002, including severity/document-order sorting and strict/source modes.
The source/index status is supplied by the caller until M4/M5 wire live checks.
Mermaid supports all 15 kinds, localized edges, safe text, branch frontiers,
loop break/continue, switch fallthrough, group-local returns, try cleanup and
transaction styling. Three reviewed insta snapshots cover these combinations.

Acceptance: 15 tests pass across the workspace, including every diagnostic code;
formatting and clippy with warnings denied pass on Rust 1.80.1. Mermaid syntax
validation via Docker remains the explicit M8 gate.

### M3

Added specification-authored fixtures with package/build metadata and four flow
goldens before adapters; the authoring example remains in the tree. Added
immutable parallel parsing, repository config/rule handling, TS/Python/Java
adapters, shared AST walker, Camel chain linearization, syntactic resolver and
entrypoint detection. `debug-ast` was introduced early as the required grammar
inspection aid. Go byte parity cannot be established without the prototype.

Acceptance: all 20 workspace tests pass on Rust 1.80.1. Extraction checks cover
four exact authored goldens (plus masking helper), cross-module DI, transactions,
Python elif and multiple handlers, Camel linked routes, W101-W106 fixture,
parallel/callbacks/loops, recursion guard and controller return-call inlining.
Formatting and clippy with warnings denied pass. Intentional normalization and
reference limitations are recorded in PARITY.md.

### M4

Added entrypoints/extract/update/todo/label/check/render/schema/version commands,
range-aware debug AST and heuristic debug resolution. Source checks re-extract
from the header, and output/label writes use atomic replacement. Update and merge
carry labels only when both ID and code match.

Acceptance: 23 workspace tests pass, including three process tests using only
std::process and tempfile. They verify unchanged label fingerprints, unknown-ID
atomicity, E003 after code tampering, E005 after threshold changes, update label
retention, JSON todo/schema, Markdown rendering and debug ranges. Formatting and
clippy with warnings denied pass on Rust 1.80.1.

### M5

Added the index-only external-tool runner, language selection/force/cache behavior,
canonical metadata and per-file freshness checks. Added a Dockerfile with Node,
Temurin 21, coursier-installed scip-java, Maven/Gradle and the Rust binary.

Acceptance: ran `wtflow index --lang ts` against the fixture using the official
scip-typescript 0.4.0 indexer, and committed its binary index and metadata. The
process test copies that index, modifies the source, verifies heuristic output
and W120, and verifies `check --source` promotes W120 to an error. All 24 workspace
tests, formatting and clippy pass on Rust 1.80.1. Dockerfile image construction
has not yet been verified; SCIP lookup itself is the next milestone.

### M6

Added ScipResolver and ChainResolver, exact non-definition occurrence lookup,
UTF-8/16/32 conversion/defaults, local/external definitions, implementation
relationships, lazy call tables, symbol documentation, package-index merging,
and freshness-aware fallback. Auto/scip extraction and debug-resolve now read
SCIP. Recorded all M3-to-SCIP golden differences in PARITY.md.

Acceptance: 32 workspace tests pass, including real-index barrel/alias dispatch,
W113, symbol documentation, all three language goldens/debug-resolve, stale
inlined targets, merged package indexes, exact/definition-role exclusion and
all encoding/default cases. All three official indexers ran successfully and
their indexes are committed. Formatting and clippy pass on Rust 1.80.1.

### M7

Added reproducible criterion corpora and benchmarks. Optimized freshness checks
from once per occurrence to once per document. Measured entrypoint scan at
43.080–46.823 ms, 50 MB index load at 50.041–50.463 ms, and depth-3 extraction
after load at 28.931–29.641 microseconds. All three requested limits pass on
Apple M4 Pro/macOS/Rust 1.80.1. See docs/performance.md for corpus and cache scope.
All 32 workspace tests, formatting and clippy pass after the optimization.

### M8

Added MSRV/stable GitHub checks, four release-build targets, deterministic golden
scripts, Docker Mermaid validation, the consumer workflow example, and the
flow-docs skill. Used skill-creator guidance; its Python initializer/validator
were not used under the project's no-Python requirement. The compact frontmatter
and workflow were reviewed directly.

Acceptance: actionlint accepts both workflows; shell syntax and golden diffs
pass. All 32 tests, clippy and formatting pass on stable as well as MSRV.
All 12 Mermaid artifacts render successfully in minlag/mermaid-cli. Native
macOS arm64 and x86_64 release binaries build and run. The indexer Docker image
builds on Linux arm64 and emits byte-identical dispatch output to the native CLI.
No remote exists, so hosted CI/release-matrix execution and artifact publication
are not claimed. The workflow defines Linux x86_64/arm64 musl and macOS
x86_64/arm64 builds without publishing a release or image.
