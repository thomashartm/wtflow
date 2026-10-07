mod app;
mod clipboard;
mod filter;
mod runtime;
mod settings;
mod tui;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use wtflow_core::{labels, lint, yaml, Flow};
use wtflow_extract::Cx;
mod clear;
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
    command: Option<Command>,
    /// Project working directory (relative paths resolve here)
    #[arg(long, global = true)]
    project: Option<PathBuf>,
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
    /// Open the persistent terminal workspace
    Tui {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    /// Analyze an entrypoint and save the configured flow and exports
    Analyze {
        /// Starting point as FILE#SYMBOL
        #[arg(long)]
        entry: String,
        #[arg(long)]
        name: Option<String>,
        /// Maximum call depth (defaults to project analysis.depth, then 32)
        #[arg(long)]
        depth: Option<usize>,
        /// Call resolution mode (defaults to project analysis.resolver)
        #[arg(long, value_enum)]
        resolver: Option<ResolverMode>,
        /// Save exports without opening a browser
        #[arg(long)]
        no_open: bool,
        /// Saved YAML and context directory, relative to project root
        #[arg(long)]
        flows_dir: Option<PathBuf>,
        #[command(flatten)]
        output: settings::OutputArgs,
    },
    /// Export a saved flow without reanalyzing source
    Export {
        flow: PathBuf,
        #[arg(long)]
        open: bool,
        #[command(flatten)]
        output: settings::OutputArgs,
    },
    /// Show project settings or set a dotted key to a YAML value
    Config {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        #[arg(long, requires = "value")]
        key: Option<String>,
        #[arg(long, requires = "key", allow_hyphen_values = true)]
        value: Option<String>,
        #[arg(long)]
        json: bool,
    },
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
        /// Noninteractive languages, separated by commas (omit for guided setup)
        #[arg(long, value_delimiter = ',')]
        lang: Vec<String>,
        /// Project owner used by noninteractive setup
        #[arg(long)]
        owner: Option<String>,
        /// Project directory for .wtflow/config.yaml (defaults to the current directory)
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    /// Reset local wtflow data, asking whether to keep the project configuration
    Clear {
        /// Confirm cleanup without prompting; keeps config unless --remove-config
        #[arg(long)]
        yes: bool,
        #[arg(long, requires = "yes")]
        remove_config: bool,
        /// Project directory to clear (defaults to the current directory)
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
        #[arg(long)]
        depth: Option<usize>,
        /// How to follow calls: auto prefers indexes, scip requires an index, heuristic uses syntax
        #[arg(long, value_enum)]
        resolver: Option<ResolverMode>,
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
    /// Set a single step label without changing the flow structure
    LabelStep {
        flow: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long, allow_hyphen_values = true)]
        text: String,
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
        #[arg(long, value_enum)]
        lang: Option<Language>,
        /// Show source code alongside labels
        #[arg(long, num_args = 0..=1, default_missing_value = "true", require_equals = true)]
        detail: Option<bool>,
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
    runtime::checkpoint()?;
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
        runtime::emit(runtime::Event::Output(text.to_owned()));
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
    if let Some(root) = cli.project {
        std::env::set_current_dir(root).context("open project")?;
    }
    wtflow_core::schema::initialize()?;
    match cli.command {
        Some(Command::Tui { dir }) => {
            tui::run(&dir)?;
            Ok(0)
        }
        None if std::io::IsTerminal::is_terminal(&std::io::stdin())
            && std::io::IsTerminal::is_terminal(&std::io::stdout()) =>
        {
            tui::run(Path::new("."))?;
            Ok(0)
        }
        None => {
            use clap::CommandFactory;
            Cli::command().print_help()?;
            Ok(0)
        }
        Some(command) => app::execute(command, progress::enabled(cli.no_progress)),
    }
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
