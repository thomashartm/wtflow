# wtflow — What the flow?

A Rust CLI for deterministic source-derived flow documents, lint, and Mermaid.
SCIP provides call resolution and tree-sitter provides control flow. Labels may
be edited separately; structural changes require extraction. No LLM calls.

Requires Rust 1.80 or newer to build. The repository toolchain pins 1.80.1 so the
minimum supported version is tested directly. Build with `cargo build --locked`;
run `cargo test --workspace --locked` and `cargo clippy --workspace --all-targets
--locked -- -D warnings`.

Implementation proceeds through the acceptance gates in [MILESTONES.md](MILESTONES.md).
The CLI supports heuristic extraction, checks and rendering. Dependency versions and Cargo.lock are committed.
When refreshing the lockfile, use modern Cargo with
`--config 'resolver.incompatible-rust-versions="fallback"'`, then verify on 1.80.1.

## Format

Schemas live in `schema/`. YAML is emitted by a handwritten canonical emitter.
Empty lists use `[]`; other sequences use block notation, except aggregate state
and boundaries. Fingerprints hash structural fields only. Labels and source line
locations do not affect the fingerprint.

## Limits

Static analysis cannot recover arbitrary runtime dependency injection (custom
providers or CDI qualifiers), dynamic calls, nested I/O inside call arguments,
or implicit exception propagation. A SCIP index must be fresh for every entry
or inlined file. Stale files fall back to heuristics and must be reported.

No Go reference was available at bootstrap; see [PARITY.md](PARITY.md).

## Usage

```
wtflow entrypoints testdata/ts
wtflow extract --entry testdata/ts/src/reconciliation/service.ts#ReconciliationService.reconcile --resolver heuristic -o docs/reconcile.flow.yaml
wtflow check --source docs/reconcile.flow.yaml
wtflow todo --json docs/reconcile.flow.yaml
wtflow label docs/reconcile.flow.yaml labels.yaml
wtflow update docs/reconcile.flow.yaml
wtflow render --lang en -o docs/reconcile.mmd docs/reconcile.flow.yaml
wtflow schema --json
```

Run source checks from the source repository or keep flow files beneath its root.
`label` refuses edited structure and unknown IDs without changing the file.
Output writes replace files atomically. Exit codes: 0 success, 1 lint failure,
2 usage or I/O failure. `check --strict` also fails on warnings.

## Indexing and resolution

Enable the desired languages in `.wtflow.yaml`, install project dependencies,
then run `wtflow index`. `--lang ts,java,py` selects languages and `--force`
rebuilds unchanged indexes. TypeScript/Python use Node and their official npm
indexers; Java requires scip-java, a JDK and a working Gradle or Maven build.
These tools are used only by `index`. The resulting `*.scip` files and `meta.yaml`
can be committed so extraction and tests work without them.

Prefer `--resolver auto`: SCIP answers first and heuristics handle unresolved or
stale files. `--resolver scip` requires an index; freshness fallback still applies.
`--resolver heuristic` is useful for reference comparisons. W120 identifies
stale entry/inlined files; `check --source` treats it as an error. The header
records `scip`, `heuristic`, or `mixed`, and SCIP-resolved nodes carry symbols.
Use `debug-resolve FILE:LINE:COL` to inspect a one-based UTF-8 byte position.

Build the local indexer image with:

```
docker build -f docker/indexers.Dockerfile -t wtflow-indexers:local .
docker run --rm -v "$PWD:/src" wtflow-indexers:local wtflow index
```

The suggested `ghcr.io/aderiserp/wtflow-indexers` name is a publication target;
this repository does not publish it automatically. Python projects can supply
an indexer `--environment environment.json` argument to avoid environment
introspection through pip, as the committed fixture does.

## Automation

CI runs formatting, clippy, tests on the MSRV and stable, deterministic golden
checks, Docker Mermaid syntax checks, and release builds for Linux x86_64/arm64
(musl) and macOS x86_64/arm64. Artifacts are uploaded to the workflow run; no
release or container is published automatically. The [consumer example](examples/consumer-workflow.yml)
shows index refresh, source validation and render-diff checks; adapt its tool
installation prerequisite to your repository.

Use the [flow-docs skill](skills/flow-docs/SKILL.md) for assisted documentation.
Local performance measurements and their scope are in [docs/performance.md](docs/performance.md).

## Labeling context hooks

`wtflow todo --json --context FLOW.flow.yaml` emits node context, available SCIP
callee signatures/documentation, ancestor IDs and adjacent sibling IDs. It loads
an optional `glossary.yaml` from the repository root and includes it in each
packet. `--all` includes labeled nodes. This command is offline and does not
modify the flow. See the [labels.cache design](docs/labels-cache.md); no cache
storage or LLM integration is implemented.
