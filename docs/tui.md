# Terminal workspace

Run `wtflow tui` in a project, or `wtflow tui --dir /path/to/project`.
With an interactive stdin and stdout, `wtflow` without a command also opens the
workspace. Redirected use prints help; existing subcommands remain available.
`wtflow --project /path/to/project COMMAND` sets the working directory for any
command, including the TUI.

## Daily workflow

1. In **Project**, initialize configuration and build the index. Select languages
   and enable **force** when a rebuild is needed. Jobs show their phase and elapsed
   time; **Activity** shows output, diagnostics, and live indexer logs.
2. In **Entrypoints**, press `r` to discover routes and handlers. Search with `/`;
   `F2` includes tests. Select an entry and press Enter for analysis options.
   Press `a` to provide a `FILE#SYMBOL` manually.
3. Press `F5` to run. Analysis opens the saved flow in the terminal. Use Right or
   Enter to expand a step, Left to collapse it or return to its parent, and `*`
   to expand everything. Branch names remain visible. The details pane shows
   source, call targets, documentation, return types, and step IDs.
4. Press `l` on a step to edit its label, `c` to check a flow, `r` to update from
   source, or `e` to export. Export reads the saved flow without reanalysis.
   Enable **open** in the export form to launch the HTML view.

These are static code paths, not runtime traces. Review analysis notes for
unresolved calls, external boundaries, recursion, and traversal limits.

## Navigation

Section shortcuts sit in a compact row at the bottom. In Project, the Details
panel keeps project and index status at its bottom while the selected action
description appears above.

| Control | Action |
|---|---|
| Tab / Shift+Tab, or 1–6 | Change section |
| Up/Down, j/k, Page Up/Down | Select and scroll |
| Mouse hover or click / wheel | Select list items and show their description in Details / scroll |
| Ctrl+P | Open all CLI operations |
| `/`, then Enter | Filter entrypoints or flows |
| `d` | Show details on a narrow terminal |
| Alt+Up/Down | Scroll details; Page Up/Down also works in the narrow details view |
| Esc | Close a form, leave a flow, or clear the search |
| `?` | Keyboard help |
| Ctrl+C | Cancel the active job |
| Drag with the left mouse button | Select visible text anywhere; release to copy |
| Alt+Y / Ctrl+Shift+C / Ctrl+Insert | Copy the focused field, search, activity line, or details |
| Ctrl+V / Shift+Insert, or terminal Paste | Paste into the current text field or search |
| `q` | Quit when no job is running |

Forms use Tab to select a field, Space to toggle flags, and Ctrl+U to clear a
value. `F5` (or Ctrl+Enter) runs the operation. `Ctrl+Y` copies the equivalent CLI
command using the system clipboard helper; it is also retained in Activity.
Outside forms, Ctrl+Y copies the focused text. Cmd+V on macOS and the terminal's
Paste action also work through bracketed paste. Pasting on Entrypoints or Flows
starts a search; read-only screens and toggle fields ignore pasted text. Search
converts line breaks to spaces, while form values preserve multiline YAML and
labels. Paste appends to the value; Ctrl+U clears it first when replacing it.
Pasted text never submits a form or triggers keyboard shortcuts.

Drag selection works across lists, details, Activity, and popups, and copies the
visible text on release. Keyboard copy preserves the full value even when the
display truncates it. The standalone picker uses the terminal's native mouse
selection and the same keyboard clipboard shortcuts. Clipboard helpers are
`pbcopy`/`pbpaste` on macOS, `wl-copy`/`wl-paste`, `xclip`, or `xsel` on Linux,
and PowerShell on Windows. An unavailable clipboard is reported without closing
the TUI; terminal Paste remains available. Ctrl+C continues to cancel work.

Multi-file positional fields accept shell-style quotes around paths containing
spaces. Values are passed as arguments and never evaluated by a shell.

The layout adapts down to 40 columns by 10 rows. At narrow widths, `d` switches
between the list and details. Short tab labels stand for Project, Entrypoints,
Flows, Settings, Logs/Activity, and Actions.

## Project defaults and per-run options

**Settings** edits `.wtflow/config.yaml` (or an existing legacy config).
Enter on a setting opens its key and YAML value; F5 saves the validated default.
Advanced configuration supports all schema keys, including modules, rules,
ignored calls, indexer arguments, and `index.python.project_name`.
Ordinary block-style edits preserve unrelated YAML sections and their comments;
the edited section is normalized. Flow-style mappings may be reserialized.

An analysis or export form changes options for that run only. Omitted fields
inherit project settings; explicit CLI flags and form values override them.
Boolean overrides use `--detail=false` or `--expanded=false`; bare `--detail`
and `--expanded` enable the setting.

```yaml
analysis:
  resolver: auto
  depth: 32
output:
  flows_dir: .wtflow/flows
  export_dir: docs/flows
  formats: [html, md, mmd, lint]
  lang: en
  detail: false
  direction: TD
  theme: default
  expanded: false
  open: false
```

- `flows_dir` holds YAML, call context, and analysis notes. `export_dir` defaults
  to the saved flow's directory. Relative configured paths use the project root.
- Formats are HTML, Mermaid Markdown (`md`), raw Mermaid (`mmd`), and lint text.
- `lang` (`en`/`de`) and `direction` (`TD`/`LR`) control Mermaid connectors/layout.
- `detail` includes source in Mermaid and initially opens source details in HTML.
- `theme` supports `default`, `light`, and `dark`. `expanded` initially opens all
  HTML call containers. These settings do not alter the analysis or fingerprint.
- `open` controls browser opening after `analyze`; `--no-open` overrides it.
  `export --open` explicitly opens the generated HTML.

Equivalent CLI examples:

```sh
wtflow init --lang ts --owner orders
wtflow index --lang ts
wtflow entrypoints .
wtflow config --key output.export_dir --value docs/flows
wtflow config --key output.formats --value '[html, md]'
wtflow analyze --entry src/orders.ts#OrdersController.create --no-open
wtflow analyze --entry src/orders.ts#OrdersController.create --flows-dir saved
wtflow export saved/order.flow.yaml --formats mmd --direction LR -o docs/order.mmd
wtflow label-step saved/order.flow.yaml --id send_order --text 'Send order'
```

`-o/--output` on analyze/export requires one export format. Saved YAML remains
separate. Existing `extract -o` and `render -o` keep their document/stdout behavior;
`extract` and `render` also read applicable project defaults.

## Jobs and cleanup

Indexing runs in a worker and streams logs into Activity. The full log is retained
under `.wtflow/logs`. On Unix, cancellation terminates the indexer process group,
including child processes. Rebuilds write staged indexes first; a failed or
cancelled indexer leaves previous indexes intact. Source analysis is cooperative:
cancellation takes effect at its next safe checkpoint. The interface stays usable
while waiting. Completed output files may remain after a late cancellation.

Project → Clear requires enabling **yes** in its form. It deletes only data
inside `.wtflow`, keeping `config.yaml` unless **remove-config** is enabled.
Configured flow stores and exports outside `.wtflow` remain. The equivalent CLI
is `wtflow clear --yes [--remove-config]`; interactive `wtflow clear` still prompts.

## Maintaining CLI/TUI parity

`Command` and Clap definitions are the operation catalog. TUI forms derive every
operation's options, choices, help, and validation from that catalog, and submit
typed requests to `app::execute`, the same dispatcher used by the CLI. The TUI
never launches a wtflow subprocess or parses CLI output. The `flows` action opens
the workspace browser; `tui` itself selects the frontend.

New capabilities must use the shared dispatcher and configuration schema. Add a
primary-screen shortcut where useful; the Actions form is available automatically.
A change is incomplete if one frontend cannot express its domain options.
Navigation, terminal progress, and stdin/clipboard transport are frontend concerns.
For labels, the terminal provides file import and single-step editing; the CLI
also accepts piped label maps.

The catalog coverage test checks operation/option exposure. Fixture tests compare
CLI-parsed and TUI-form requests for matching artifacts and diagnostics. Release
checks include the workspace suite, Clippy, and a real terminal walkthrough with
resize, failure, and cancellation.
