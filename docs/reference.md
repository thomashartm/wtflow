# wtflow technical reference

For an overview and a first example, see the [README](../README.md).

A Rust CLI for deterministic source-derived flow documents, lint, and Mermaid.
SCIP provides call resolution and tree-sitter provides control flow. Labels may
be edited separately; structural changes require extraction. No LLM calls.

Requires Rust 1.80 or newer to build. The repository toolchain pins 1.80.1 so the
minimum supported version is tested directly. Build with `cargo build --locked`;
run `cargo test --workspace --locked` and `cargo clippy --workspace --all-targets
--locked -- -D warnings`.

Implementation proceeds through the acceptance gates in [MILESTONES.md](../MILESTONES.md).
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

No Go reference was available at bootstrap; see [PARITY.md](../PARITY.md).

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
`entrypoints DIR` scans only that directory and its children, while source paths
remain relative to the project's configuration root. Source scans skip build
output such as `dist/` and `build/`, dependencies, and TypeScript `.d.ts`
declaration files.
During entrypoint discovery, files that cannot be parsed are skipped with a
warning on standard error; the resulting list may be incomplete. Extraction
still treats source parsing failures as errors.
The TypeScript parser handles import types in type annotations, `using` as a
call argument, and NUL characters inside string literals through targeted,
AST-scoped compatibility projections. Original source text and byte positions
are preserved. Other syntax errors still fail validation, including missing
punctuation. `debug-ast` shows the original parser tree, including error nodes,
to help diagnose unsupported syntax.
`label` refuses edited structure and unknown IDs without changing the file.
Output writes replace files atomically. Exit codes: 0 success, 1 lint failure,
2 usage or I/O failure. `check --strict` also fails on warnings.

Long-running commands show a spinner on an interactive stderr terminal. It clears
before results or warnings are printed, including on errors. Use `--no-progress`
before or after the command name to disable it. Redirected stderr and `TERM=dumb`
automatically disable animation; JSON, YAML, and Mermaid output stay unchanged.

`entrypoints` saves its latest directory scan to `.wtflow/entrypoints.json` in
the configuration root, as well as printing its existing text or JSON output.
Discovery parses source without preparing call resolution or loading index metadata.
Call resolution is prepared after selecting a flow, reusing those parsed files.
Source and index scans stop at nested Git repositories and worktrees, including
agent worktrees under `.claude/worktrees`. To inspect one, run wtflow inside that
checkout or select it explicitly with `--dir`.

`wtflow flows [--dir DIR]` normally reads `.wtflow/flows/`, `docs/flows/`, and
`.flow.yaml` files directly in the project root. It does not recursively search
the entire project or include test goldens. An explicitly selected subdirectory
can contain legacy documents; nested fixture directories are skipped. The
current project path is displayed above the picker. It always discovers entrypoints
and combines them with saved analyses in one list, marking saved entries **[saved]**.
Choose any entry to analyze or refresh it and open the result. New analyses
use automatic resolution and follow internal calls up to a safety depth of 32,
with at most 512 call expansions. Recursion and both safety limits leave visible
call nodes and explanations in the saved notes. `extract` uses the same default;
an explicit `--depth` requests a shorter summary (or a different depth limit).
Stable filenames use the entry file and symbol. Refreshing a saved flow upgrades
older shallow summaries and preserves labels on unchanged steps. If source is
unavailable, the saved snapshot can still be opened.
Flow YAML, Mermaid Markdown, a standalone HTML explorer, and lint/discovery notes
are saved together in `.wtflow/flows/`. In an interactive terminal the HTML view
opens automatically in the default browser. `--no-open` and redirected input or
output disable launching. The HTML view is offline, needs no plugins, escapes
source-derived text, and supports expanding calls and inspecting source details.
Call documentation, definition locations, and return types are stored separately
in `.flow.context.json`, bound to the flow fingerprint. They do not change the
canonical YAML or fingerprint. Missing return types are shown as unavailable.
For/while loops have orange borders, explicit LOOP headings, and Repeat body sections.
Personal purpose notes are browser-local drafts associated with the node ID and
unchanged code. Export them as `labels.yaml`, then apply with
`wtflow label FLOW labels.yaml` to persist them in the flow document.
In a terminal, the picker uses arrow keys and Enter, live search as you type,
Page Up/Down, and a list sized to the window. F2 toggles tests. Escape clears a
search, then exits; Ctrl-C exits immediately. Ctrl-U clears search.
The terminal is restored on exit and I/O errors.
With redirected input/output or TERM=dumb, the line-based fallback remains:
numbers select, `>`/`<` page, `/text` searches, `t` toggles tests, and `q` exits.
Test/fixture directories and conventional test filenames are
hidden initially. The full inventory and `entrypoints` command
still include tests.

`flows --filter PATTERN` starts the picker with an editable search;
`entrypoints [DIR] --filter PATTERN` filters text or `--json` output while retaining
the complete discovery inventory in `.wtflow/entrypoints.json`. Both match route,
function, and file names case-insensitively. Plain text matches substrings, `*`
matches any text, and `?` matches one character. Space-separated terms are ANDed.
Quote patterns in the shell, for example `wtflow flows --filter '*Controller.create*'`.

Interactive analysis can continue when unrelated source files cannot be parsed;
these omissions and other diagnostics are saved in the `.flow.lint.txt` notes,
which are linked when the diagram is rendered. Review them for incomplete
analysis. Explicit `extract` remains strict about parsing failures. Invalid saved
documents are reported and skipped. Enter `q` or end input to quit without
creating a flow; entrypoint discovery still saves its inventory.

## Project setup

Run `wtflow init` in the project root, or `wtflow init --dir PATH` to create the
configuration in another directory. Missing directories are created after the
questions are answered. The command suggests languages from package/build files,
asks for the project's owner or service name, and optionally assigns owners to
individual folders. Python indexing also needs a project name.

The generated `.wtflow.yaml` enables the selected indexers, uses TypeScript's
`--infer-tsconfig` option when selected, and enables step collapsing. Custom
classification rules and other advanced settings can be added afterward.
Setup validates the configuration before writing, never overwrites an existing
file or symlink, and runs no external tools. Invalid answers are prompted again;
ending input before setup completes creates no configuration.

## Indexing and resolution

Enable the desired languages in `.wtflow.yaml`, install project dependencies,
then run `wtflow index`. `--lang ts,java,py` selects languages and `--force`
rebuilds unchanged indexes. TypeScript/Python use Node and their official npm
indexers; Java requires scip-java, a JDK and a working Gradle or Maven build.
These tools are used only by `index`. The resulting `*.scip` files and `meta.yaml`
can be committed so extraction and tests work without them.

Indexing scans filenames and hashes source bytes without parsing them through
tree-sitter; the official indexer handles language syntax. Each index run saves
the indexer commands, stdout, stderr, and outcome to a fresh numbered file in
`.wtflow/logs/` and prints its project-relative reference. Failed runs and
up-to-date runs also keep a log. Indexer output is streamed directly to that file,
so it can be followed with `tail -f` while the terminal shows progress. Log files
are local diagnostics and are not part of the flow document or its fingerprint.

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

Run `make install` to build and copy the binary into `~/.local/bin`, so it can be
used outside this repository. No administrator privileges are needed. If the
directory is missing from PATH, the command prints setup instructions. Repeat
the command to update the installed copy. To choose a different destination, use
`make install INSTALL_DIR=/your/bin/directory`.

Build a release locally with `make release`. The binary is written to
`target/<host-triple>/release/wtflow`, and a git-ignored `./wtflow` symlink points
to the newly built binary. Run it with `./wtflow`. Each successful build updates
the link, including when you select a different target or build directory;
running a cross-built binary still requires a compatible host. `make dist` also creates
`dist/wtflow-<version>-<target>.tar.gz` and a matching `.sha256` checksum file.
The archive contains the standalone `wtflow` binary; these commands do not publish.

Use `TARGET` to select one of the release platforms shown by `make help`, for example:

```sh
rustup target add x86_64-apple-darwin
make dist TARGET=x86_64-apple-darwin
```

The repository's pinned Rust toolchain is used by default. Cross-building also
requires a compiler/linker for that target. Linux musl releases are built on
matching Linux architectures with `musl-tools` installed; macOS releases require
the Apple SDK and command-line tools. `CARGO_TARGET_DIR` and `DIST_DIR` can override
the build and archive directories. Verify an archive with
`cd dist && shasum -a 256 -c wtflow-<version>-<target>.tar.gz.sha256`.

CI runs formatting, clippy, tests on the MSRV and stable, deterministic golden
checks, Docker Mermaid syntax checks, and release builds for Linux x86_64/arm64
(musl) and macOS x86_64/arm64. Artifacts are uploaded to the workflow run; no
release or container is published automatically. The [consumer example](../examples/consumer-workflow.yml)
shows index refresh, source validation and render-diff checks; adapt its tool
installation prerequisite to your repository.

Use the [flow-docs skill](../skills/flow-docs/SKILL.md) for assisted documentation.
Local performance measurements and their scope are in [performance.md](performance.md).

## Labeling context hooks

`wtflow todo --json --context FLOW.flow.yaml` emits node context, available SCIP
callee signatures/documentation, ancestor IDs and adjacent sibling IDs. It loads
an optional `glossary.yaml` from the repository root and includes it in each
packet. `--all` includes labeled nodes. This command is offline and does not
modify the flow. See the [labels.cache design](labels-cache.md); no cache
storage or LLM integration is implemented.
