# wtflow — What the flow?

**Turn code into a flow you can read, discuss, and keep up to date.**

Pick a function. wtflow follows its steps, branches, and calls, then creates a
flow document and diagram. It works with TypeScript, Python, and Java, including
Apache Camel routes.

Use it to understand unfamiliar code, explain a process to your team, or spot
problems such as unreachable steps and errors that get silently ignored.

## What you get

- A readable flow document you can keep alongside your code.
- A diagram you can show in Markdown using Mermaid.
- Checks that flag possible logic problems and outdated documentation.
- Your own labels for steps, so a diagram can speak your team's language.

wtflow reads your code without running it. It does not send it to an AI service.

## Try it

With Rust installed, run this from the wtflow repository:

```sh
make release
```

Create a flow from the included reconciliation example, check it, and draw it:

```sh
./wtflow extract --entry testdata/ts/src/reconciliation/service.ts#ReconciliationService.reconcile -o reconcile.flow.yaml
./wtflow check reconcile.flow.yaml
./wtflow render -o reconcile.md reconcile.flow.yaml
```

Open `reconcile.md` in a Markdown viewer that supports Mermaid to see the diagram.
To use `wtflow` from any project, run this once from the wtflow repository:

```sh
make install
```

This installs it into `~/.local/bin`. If that folder is not on your PATH, the
command prints the line to add to your shell settings. Run `make install` again
whenever you want to install a newer build.

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

If you have no saved flows yet, choose an entrypoint from the numbered list.
wtflow analyzes it and saves the flow, diagram, and analysis notes in
`.wtflow/flows/`. If you already have saved flows, choose one to draw it again,
or enter `n` to analyze another entrypoint.

Open the Markdown diagram and follow it from top to bottom. Where are decisions
made? What can stop the process? Which calls reach another service? Use the source
locations in the flow YAML to jump back to the code. Review warnings, especially
calls wtflow could not follow or calls with several possible destinations.

**4. Follow the interesting parts.** If a service call hides the detail you need,
create a second flow starting at that method, or re-extract with `--depth 3`.
Add plain-language step labels with `wtflow label` as you learn what the code
means. Keep each flow focused on one question, and save the useful ones alongside
the project so the next person has a starting point.

## Browse and draw saved flows

To see saved flows or create your first one, run:

```sh
wtflow flows
```

Choose a number to draw a saved flow, `n` to analyze another entrypoint, or `q`
to leave. New analyses live in `.wtflow/flows/`: the `.flow.yaml` document, its
`.flow.md` diagram, and `.flow.lint.txt` notes. Open diagrams in a Markdown viewer
that supports Mermaid. Use `wtflow flows --dir /path/to/project` for another
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
