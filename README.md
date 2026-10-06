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
