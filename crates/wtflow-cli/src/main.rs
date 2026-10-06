mod filter;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use wtflow_core::{labels, lint, yaml, Flow};
use wtflow_extract::Cx;
mod flows;
mod index;
mod init;
mod picker;
mod progress;
use progress::Progress;
#[derive(Parser)]
#[command(
    name = "wtflow",
    version,
    about = "What the flow? Turn code into readable flows and diagrams."
)]
struct Cli {
    /// Hide the terminal activity indicator
    #[arg(long, global = true)]
    no_progress: bool,
    #[command(subcommand)]
    command: Command,
}
#[derive(Clone, Copy, ValueEnum)]
enum ResolverMode {
    Auto,
    Scip,
    Heuristic,
}
#[derive(Clone, Copy, ValueEnum)]
enum Language {
    En,
    De,
}
#[derive(Subcommand)]
enum Command {
    /// Browse saved flows, or analyze an entrypoint and save its diagram
    Flows {
        /// Filter route, function, or file names (case-insensitive; * and ? wildcards)
        #[arg(long, value_name = "PATTERN")]
        filter: Option<String>,
        /// Project or directory to browse; includes the project's .wtflow/flows store
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        /// Save the interactive HTML view without opening a browser
        #[arg(long)]
        no_open: bool,
    },
    /// Create a project configuration by answering a few questions
    Init {
        /// Directory for .wtflow.yaml (defaults to the current directory)
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    /// Build or refresh the project index to follow calls between files
    Index {
        /// Languages to index, separated by commas: ts, java, py
        #[arg(long, value_delimiter = ',')]
        lang: Vec<String>,
        /// Rebuild indexes even when source files have not changed
        #[arg(long)]
        force: bool,
    },
    /// Find routes, event handlers, and other places where a flow begins
    Entrypoints {
        /// Filter route, function, or file names (case-insensitive; * and ? wildcards)
        #[arg(long, value_name = "PATTERN")]
        filter: Option<String>,
        /// Print entries as JSON
        #[arg(long)]
        json: bool,
        /// Directory to search
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    /// Create a flow document from a function, method, or Camel route
    Extract {
        /// Starting point as FILE#SYMBOL, for example src/service.ts#Service.run
        #[arg(long)]
        entry: String,
        /// Name for the flow document
        #[arg(long)]
        name: Option<String>,
        /// Follow internal calls by default; set a smaller depth for a shorter summary
        #[arg(long, default_value_t = wtflow_extract::DEFAULT_DEPTH)]
        depth: usize,
        /// How to follow calls: auto prefers indexes, scip requires an index, heuristic uses syntax
        #[arg(long, value_enum, default_value = "auto")]
        resolver: ResolverMode,
        /// Keep labels from an existing flow where the step ID and code are unchanged
        #[arg(long)]
        merge: Option<PathBuf>,
        /// Write the flow to a file instead of standard output
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
    /// Refresh flow documents from source and keep labels on unchanged steps
    Update {
        /// Flow YAML files to refresh
        #[arg(required = true)]
        flows: Vec<PathBuf>,
    },
    /// List flow steps that still need a human-readable label
    Todo {
        /// Flow YAML file to inspect
        flow: PathBuf,
        /// Include steps that already have labels
        #[arg(long)]
        all: bool,
        /// Print steps as JSON
        #[arg(long)]
        json: bool,
        /// Include callee documentation, neighboring steps, and glossary; requires --json
        #[arg(long, requires = "json")]
        context: bool,
    },
    /// Apply your own step labels without changing the flow structure
    Label {
        /// Flow YAML file to update
        flow: PathBuf,
        /// YAML file mapping step IDs to labels, or - to read standard input
        labels: String,
    },
    /// Check flows for logic problems and unwanted structural edits
    Check {
        /// Treat warnings as failures
        #[arg(long)]
        strict: bool,
        /// Also check for source changes and stale indexes
        #[arg(long)]
        source: bool,
        /// List individual unresolved calls
        #[arg(long)]
        verbose: bool,
        /// Flow YAML files to check
        #[arg(required = true)]
        flows: Vec<PathBuf>,
    },
    /// Draw a flow as a Mermaid diagram or Markdown document
    Render {
        /// Flow YAML file to draw
        flow: PathBuf,
        /// Language for diagram connectors: en (English) or de (German)
        #[arg(long, value_enum, default_value = "en")]
        lang: Language,
        /// Show source code alongside labels
        #[arg(long)]
        detail: bool,
        /// Output file (.mmd or .md); omit to print Mermaid to standard output
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
    /// Print the schema describing valid flow or configuration files
    Schema {
        /// Print the schema as JSON instead of YAML
        #[arg(long)]
        json: bool,
        /// Print the project configuration schema instead of the flow schema
        #[arg(long)]
        config: bool,
    },
    /// Show how wtflow parses a source file (development aid)
    DebugAst {
        /// Source file to inspect
        file: PathBuf,
        /// Limit the tree to L:C-L:C using one-based lines and UTF-8 byte columns
        #[arg(long)]
        range: Option<String>,
    },
    /// Show which definition a call points to (development aid)
    DebugResolve {
        /// Call position as FILE:LINE:COL using one-based lines and UTF-8 byte columns
        position: String,
    },
    /// Print the installed wtflow version
    Version,
}
fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("{}:1: read", path.display()))
}
fn load(path: &Path) -> Result<Flow> {
    yaml::load(&read(path)?, &path.display().to_string())
}
fn write(path: Option<&Path>, text: &str) -> Result<()> {
    if let Some(path) = path {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)?;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let suffix = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp = parent.join(format!(".wtflow-{}-{suffix}.tmp", std::process::id()));
        let result = (|| -> Result<()> {
            let mut f = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
            if let Ok(meta) = std::fs::metadata(path) {
                std::fs::set_permissions(&temp, meta.permissions())?;
            }
            std::fs::rename(&temp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result.with_context(|| format!("{}:1: write", path.display()))
    } else {
        std::io::stdout().lock().write_all(text.as_bytes())?;
        Ok(())
    }
}
fn locate(flow_path: &Path, flow: &Flow) -> Result<PathBuf> {
    let absolute = flow_path.canonicalize()?;
    let cwd = std::env::current_dir()?;
    for base in absolute.ancestors().skip(1).chain(cwd.ancestors()) {
        let candidate = base.join(&flow.entry.file);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "{}:1: cannot locate source {} from flow location or current directory",
        flow_path.display(),
        flow.entry.file
    )
}
fn reextract(path: &Path, flow: &Flow) -> Result<(Flow, Vec<String>)> {
    let entry = locate(path, flow)?;
    let mut cx = Cx::load(&entry)?;
    if flow.resolution != wtflow_core::ResolutionMode::Heuristic {
        cx.enable_scip(false)?;
    }
    let new = cx.extract(
        &flow.entry.file,
        &flow.entry.symbol,
        Some(&flow.flow),
        flow.entry.depth,
    )?;
    let stale = cx.stale_for(&new);
    Ok((new, stale))
}
fn point(text: &str, value: &str) -> Result<usize> {
    let (line, col) = value.split_once(':').context("position must be LINE:COL")?;
    let line: usize = line.parse()?;
    let col: usize = col.parse()?;
    anyhow::ensure!(line > 0 && col > 0, "positions are one-based");
    let mut start = 0;
    for (i, s) in text.split_inclusive('\n').enumerate() {
        if i + 1 == line {
            let offset = col - 1;
            anyhow::ensure!(
                offset <= s.trim_end_matches('\n').len() && s.is_char_boundary(offset),
                "column outside line or UTF-8 boundary"
            );
            return Ok(start + offset);
        }
        start += s.len();
    }
    anyhow::bail!("line outside file")
}
fn run() -> Result<i32> {
    let cli = Cli::parse();
    let show_progress = progress::enabled(cli.no_progress);
    wtflow_core::schema::initialize()?;
    match cli.command {
        Command::Flows {
            dir,
            no_open,
            filter,
        } => flows::run(
            &dir,
            show_progress,
            !no_open,
            filter.as_deref().unwrap_or(""),
        )?,
        Command::Init { dir } => init::run(&dir)?,
        Command::Index { lang, force } => index::run(&lang, force, show_progress)?,
        Command::Version => println!("wtflow {}", env!("CARGO_PKG_VERSION")),
        Command::Schema { json, config } => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&wtflow_core::schema::value(config)?)?
                );
            } else {
                print!(
                    "{}",
                    if config {
                        wtflow_core::schema::CONFIG
                    } else {
                        wtflow_core::schema::FLOW
                    }
                );
            }
        }
        Command::Entrypoints { dir, json, filter } => {
            let mut progress = Progress::start(show_progress, "Finding entrypoints...");
            let (cx, warnings) = Cx::load_entrypoints(&dir)?;
            let entries = cx.entrypoints();
            flows::save_entries(&cx.config.root, &entries)?;
            let filter = filter::Filter::new(filter.as_deref().unwrap_or(""));
            let entries: Vec<_> = entries
                .into_iter()
                .filter(|e| filter.matches(&[&e.trigger, &e.symbol, &e.file]))
                .collect();
            progress.finish();
            for warning in warnings {
                eprintln!("warning: {warning}");
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                for e in entries {
                    println!("{}#{} {}", e.file, e.symbol, e.trigger);
                }
            }
        }
        Command::Extract {
            entry,
            name,
            depth,
            resolver,
            merge,
            output,
        } => {
            let mut progress = Progress::start(show_progress, "Extracting flow...");
            let (path, symbol) = entry
                .rsplit_once('#')
                .context("--entry must be FILE#SYMBOL")?;
            let path = Path::new(path)
                .canonicalize()
                .with_context(|| format!("{path}:1: entry"))?;
            let mut cx = Cx::load(&path)?;
            if !matches!(resolver, ResolverMode::Heuristic) {
                cx.enable_scip(matches!(resolver, ResolverMode::Scip))?;
            }
            let relative = path
                .strip_prefix(&cx.config.root)?
                .to_string_lossy()
                .replace('\\', "/");
            let extraction = cx.extract_report(&relative, symbol, name.as_deref(), depth)?;
            let mut flow = extraction.flow;
            let stale = cx.stale_for(&flow);
            if let Some(old) = merge {
                labels::carry(&load(&old)?, &mut flow)?;
            }
            let text = yaml::emit(&flow)?;
            progress.finish();
            for note in extraction.notes {
                eprintln!("note: {note}");
            }
            for file in stale {
                eprintln!("warning W120 - stale index for {file}");
            }
            write(output.as_deref(), &text)?;
        }
        Command::Label {
            flow,
            labels: input,
        } => {
            let mut f = load(&flow)?;
            let text = if input == "-" {
                let mut text = String::new();
                std::io::stdin().read_to_string(&mut text)?;
                text
            } else {
                read(Path::new(&input))?
            };
            let patch: BTreeMap<String, String> =
                serde_yaml_ng::from_str(&text).context("labels: invalid flat id-to-label map")?;
            labels::apply(&mut f, &patch)?;
            write(Some(&flow), &yaml::emit(&f)?)?;
        }
        Command::Todo {
            flow,
            all,
            json,
            context,
        } => {
            let f = load(&flow)?;
            f.verify_fingerprint()?;
            if context {
                let mut progress = Progress::start(show_progress, "Gathering step context...");
                let entry = locate(&flow, &f)?;
                let mut cx = Cx::load(&entry)?;
                cx.enable_scip(false)?;
                let stale = cx.stale_for(&f);
                let text =
                    serde_json::to_string_pretty(&wtflow_extract::context::packets(&cx, &f, all)?)?;
                progress.finish();
                for file in stale {
                    eprintln!("warning W120 - stale index for {file}");
                }
                println!("{text}");
                return Ok(0);
            }
            let todo = labels::todo(&f, all);
            if json {
                println!("{}", serde_json::to_string_pretty(&todo)?);
            } else {
                for n in todo {
                    println!(
                        "{} {} {} {}",
                        n.id,
                        n.kind.as_str(),
                        n.path,
                        n.code.replace('\n', " ")
                    );
                }
            }
        }
        Command::Update { flows } => {
            let mut updates = vec![];
            for path in flows {
                let mut progress = Progress::start(show_progress, "Updating flow...");
                let old = load(&path)?;
                old.verify_fingerprint()?;
                let (mut new, stale) = reextract(&path, &old)?;
                let changed = new.fingerprint != old.fingerprint;
                let kept = labels::carry(&old, &mut new)?;
                updates.push((path, yaml::emit(&new)?, changed, kept));
                progress.finish();
                for file in stale {
                    eprintln!("warning W120 - stale index for {file}");
                }
            }
            for (path, text, changed, kept) in updates {
                write(Some(&path), &text)?;
                println!("structure changed={changed}, labels kept={kept}");
            }
        }
        Command::Check {
            flows,
            strict,
            source,
            verbose,
        } => {
            let mut failed = false;
            for path in flows {
                let mut progress = Progress::start(show_progress, "Checking flow...");
                let text = read(&path)?;
                let mut context = lint::Context {
                    verbose,
                    source,
                    ..lint::Context::default()
                };
                if source {
                    if let Ok(flow) = yaml::load(&text, &path.display().to_string()) {
                        let (new, stale) = reextract(&path, &flow)?;
                        context.source_changed = new.fingerprint != flow.fingerprint;
                        context.stale_files = stale;
                    }
                } else if let Ok(flow) = yaml::load(&text, &path.display().to_string()) {
                    if let Ok(entry) = locate(&path, &flow) {
                        let config = wtflow_extract::config::RepositoryConfig::discover(&entry)?;
                        if let Some(meta) = wtflow_resolve::metadata::Metadata::load(&config.root)?
                        {
                            let mut paths =
                                std::collections::BTreeSet::from([flow.entry.file.clone()]);
                            let mut nodes = vec![];
                            wtflow_core::visit(&flow.steps, &mut nodes);
                            for node in nodes {
                                if let Some((file, _)) = node.src.rsplit_once(':') {
                                    paths.insert(file.into());
                                }
                            }
                            context.stale_files = paths
                                .into_iter()
                                .filter(|file| {
                                    std::fs::read(config.root.join(file))
                                        .map_or(true, |bytes| !meta.fresh(file, &bytes))
                                })
                                .collect();
                        }
                    }
                }
                let diagnostics = lint::document(&text, &path.display().to_string(), &context);
                failed |= lint::fails(&diagnostics, strict);
                progress.finish();
                for d in diagnostics {
                    println!("{d}");
                }
            }
            return Ok(i32::from(failed));
        }
        Command::Render {
            flow,
            lang,
            detail,
            output,
        } => {
            let mut progress = Progress::start(show_progress, "Rendering diagram...");
            let f = load(&flow)?;
            f.verify_fingerprint()?;
            let options = wtflow_render::Options {
                lang: match lang {
                    Language::En => wtflow_render::Language::En,
                    Language::De => wtflow_render::Language::De,
                },
                detail,
            };
            let graph = wtflow_render::render(&f, &options)?;
            let text = if output
                .as_ref()
                .and_then(|p| p.extension())
                .is_some_and(|s| s == "md")
            {
                format!("```mermaid\n{graph}```\n")
            } else {
                graph
            };
            progress.finish();
            write(output.as_deref(), &text)?;
        }
        Command::DebugAst { file, range } => {
            let mut progress = Progress::start(show_progress, "Parsing source...");
            let text = read(&file)?;
            let source = wtflow_extract::source::SourceFile::parse_unchecked(
                file.to_string_lossy().into(),
                text,
            )?;
            let node = if let Some(range) = range {
                let (start, end) = range.split_once('-').context("range must be L:C-L:C")?;
                let start = point(&source.text, start)?;
                let end = point(&source.text, end)?;
                anyhow::ensure!(start <= end, "range is reversed");
                source
                    .tree
                    .root_node()
                    .descendant_for_byte_range(start, end)
                    .context("range outside syntax tree")?
            } else {
                source.tree.root_node()
            };
            fn dump(n: tree_sitter::Node<'_>, f: &wtflow_extract::SourceFile, depth: usize) {
                println!(
                    "{}{} [{}:{}-{}:{}] {}",
                    "  ".repeat(depth),
                    n.kind(),
                    n.start_position().row + 1,
                    n.start_position().column + 1,
                    n.end_position().row + 1,
                    n.end_position().column + 1,
                    wtflow_extract::source::normalized(f.text(n))
                );
                for c in wtflow_extract::source::children(n) {
                    dump(c, f, depth + 1);
                }
            }
            progress.finish();
            dump(node, &source, 0);
        }
        Command::DebugResolve { position } => {
            let mut progress = Progress::start(show_progress, "Resolving call...");
            let (fileline, col) = position
                .rsplit_once(':')
                .context("position must be FILE:LINE:COL")?;
            let (file, line) = fileline
                .rsplit_once(':')
                .context("position must be FILE:LINE:COL")?;
            let file = Path::new(file).canonicalize()?;
            let mut cx = Cx::load(&file)?;
            cx.enable_scip(false)?;
            let relative = file
                .strip_prefix(&cx.config.root)?
                .to_string_lossy()
                .replace('\\', "/");
            let source = cx.files.get(&relative).context("source not found")?;
            let pos = point(&source.text, &format!("{line}:{col}"))?;
            let n = source
                .tree
                .root_node()
                .descendant_for_byte_range(pos, pos)
                .context("position outside syntax tree")?;
            let resolution = cx.resolver().resolve(
                &relative,
                wtflow_resolve::ByteRange {
                    start: n.start_byte(),
                    end: n.end_byte(),
                },
            );
            let text = serde_json::to_string_pretty(&resolution)?;
            progress.finish();
            println!("{text}");
        }
    }
    Ok(0)
}
fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(2);
        }
    }
}
