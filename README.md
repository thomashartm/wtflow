<p align="center">
  <img src="docs/assets/wtflow-logo.png" alt="wtflow turns tangled code into clear flows" width="640">
</p>

# wtflow — What the flow?

**Turn code into a flow you can read, discuss, and keep up to date.**

Pick a function. wtflow follows its steps, branches, and calls, then creates a
flow document and diagram. It works with TypeScript, Python, and Java, including
Apache Camel routes.

Use it to understand unfamiliar code, explain a process to your team, or spot
problems such as unreachable steps and errors that get silently ignored.

## What you get

- A readable flow document you can keep alongside your code.
- An interactive browser view, plus a Mermaid diagram for sharing.
- Checks that flag possible logic problems and outdated documentation.
- Your own labels for steps, so a diagram can speak your team's language.

wtflow reads your code without running it. It does not send it to an AI service.

## Try it

With Rust installed, run this from the wtflow repository:

```sh
make release
```

Browse the included reconciliation example:

```sh
./wtflow flows --dir testdata/ts
```

Choose a starting point. wtflow follows internal calls and opens a browser view.
To use `wtflow` from any project, run this once from the wtflow repository:

```sh
make install
```

This installs it into `~/.local/bin`. If that folder is not on your PATH, the
command prints the line to add to your shell settings. Run `make install` again
whenever you want to install a newer build.

If `make` reports “No rule to make target 'release'”, check that your terminal is
in the **wtflow repository**. To build and install from any directory, use
`make -C /path/to/wtflow install`, replacing the path with your wtflow checkout.

Then run `wtflow entrypoints .` in your project to find places to begin.
The [setup guide and command reference](docs/reference.md) explain how to prepare
your project so wtflow can follow calls between files.

## Making sense of a TypeScript project

Start with one question, such as **“What happens when someone places an order?”**
A small flow is easier to understand than a diagram of the whole application.
With `wtflow` installed, open a terminal in the project's root folder.

**1. Prepare the project.** Install its dependencies using its usual package
manager (`npm ci` for an npm project with a lockfile). Then let wtflow guide you
through setup:

```sh
wtflow init
```

It asks which languages to follow, who owns the project, and whether individual
folders have different owners. Press Enter to accept a suggestion. It creates
`.wtflow.yaml`; an existing config is left untouched. To set up another folder,
use `wtflow init --dir /path/to/project`.

Choose TypeScript, then build the project's code index:

```sh
wtflow index --lang ts
```

This needs Node.js and `npx`. It builds a map of the project's code so wtflow can
follow calls between files more accurately. A spinner shows that indexing is
running. Its detailed output goes into `.wtflow/logs/`, and the command prints
the log reference when it finishes, including when something goes wrong.

**2. Find where the action starts.** List the routes and handlers wtflow recognizes:

```sh
wtflow entrypoints .
```

Look for the route or event related to your question. For placing an order, that
might be `POST /orders`. The list is saved in `.wtflow/entrypoints.json`. If nothing
is listed, find the relevant function or class method in the code yourself;
wtflow does not recognize every framework's entry points.

**3. Analyze and draw that one flow.** Run:

```sh
wtflow flows
```

Choose an entrypoint from the list. All starting points stay visible;
previously analyzed ones are marked **[saved]**.
wtflow follows its internal calls and opens an interactive view in your browser.
It saves everything in `.wtflow/flows/`. If you already have saved flows, choose
one to refresh it from source and open it, or choose any other starting point.

Follow the flow from top to bottom. Where are decisions
made? What can stop the process? Which calls reach another service? Use the source
references under **Source & details** to find the relevant code. Review analysis notes, especially
calls wtflow could not follow or calls with several possible destinations.

**4. Follow the interesting parts.** Click a section to expand its calls, or use
**Expand all**. **Overview** collapses the detail again. No depth flags or extra
commands are needed. External calls, unresolved calls, and recursion remain
visible; any safety limits are explained in the analysis notes.
Calls show their return type and existing source documentation when available.
**For and while loops stand out in orange**, with a **LOOP** heading and a
**Repeat body** section. Add your own explanation under **Your purpose note**.
Notes are saved in your browser; **Export notes as labels** downloads a portable
copy. Apply it with `wtflow label PATH/TO/FLOW.flow.yaml labels.yaml`.
Keep each flow focused on one question, and save the useful ones alongside
the project so the next person has a starting point.

## Browse and draw saved flows

To see saved flows or create your first one, run:

```sh
wtflow flows
```

Use **↑ / ↓** to move and **Enter** to open a flow. Just start typing to search
by route, function, or file. **Page Up / Page Down** scroll through long lists.
**Esc** clears your search, then leaves the picker. **F2** shows or hides test
entrypoints. Saved and new starting points appear together in one list.

To start with a smaller list, filter by route, method name, or filename:

```sh
wtflow flows --filter 'reconcile'
wtflow flows --filter '*Controller.create*'
wtflow entrypoints src/ --filter 'POST *orders*'
```

Search ignores letter case. `*` matches any text, `?` matches one character,
and space-separated terms must all match. The same patterns work when typing
in the picker; **Esc** clears the filter.

The project path is shown at the top. Saved flows come from that project's
`.wtflow/flows/`, `docs/flows/`, and root folder; test goldens are not swept into
the list. To browse another project from anywhere, use
`wtflow flows --dir /path/to/project`.

New analyses live in `.wtflow/flows/`: the `.flow.html` browser view, `.flow.yaml`
document, `.flow.md` Mermaid diagram, and `.flow.lint.txt` notes. The browser view
works offline, without plugins. Use `--no-open` to save it without launching a
browser. Use `wtflow flows --dir /path/to/project` for another
project; existing flows in folders such as `docs/flows` are still recognized.

## Keep it useful

After changing your code, refresh the project index with `wtflow index`, then run
`wtflow update` on your flow file. Labels stay attached to unchanged steps.
Use `wtflow label` to add your own wording; change the code to change the flow.

A diagram is a guide, not a record of a running program. Calls chosen at runtime
may be missed or have several possible destinations. Keep the index fresh and
review warnings before relying on a flow.

## Go further

- [Setup, commands, and known limits](docs/reference.md)
- [Build releases with the Makefile](docs/reference.md#automation)
- [Use wtflow with a coding assistant](skills/flow-docs/SKILL.md)
