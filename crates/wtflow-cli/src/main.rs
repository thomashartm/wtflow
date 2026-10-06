use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use wtflow_core::{labels, lint, yaml, Flow};
use wtflow_extract::Cx;
#[derive(Parser)]
#[command(
    name = "wtflow",
    version,
    about = "What the flow? Deterministic source-derived flow documents."
)]
struct Cli {
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
    Entrypoints {
        #[arg(long)]
        json: bool,
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    Extract {
        #[arg(long)]
        entry: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value_t = 2)]
        depth: usize,
        #[arg(long, value_enum, default_value = "auto")]
        resolver: ResolverMode,
        #[arg(long)]
        merge: Option<PathBuf>,
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
    Update {
        #[arg(required = true)]
        flows: Vec<PathBuf>,
    },
    Todo {
        flow: PathBuf,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    Label {
        flow: PathBuf,
        labels: String,
    },
    Check {
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        source: bool,
        #[arg(long)]
        verbose: bool,
        #[arg(required = true)]
        flows: Vec<PathBuf>,
    },
    Render {
        flow: PathBuf,
        #[arg(long, value_enum, default_value = "en")]
        lang: Language,
        #[arg(long)]
        detail: bool,
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
    Schema {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        config: bool,
    },
    DebugAst {
        file: PathBuf,
        #[arg(long)]
        range: Option<String>,
    },
    DebugResolve {
        position: String,
    },
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
fn reextract(path: &Path, flow: &Flow) -> Result<Flow> {
    let entry = locate(path, flow)?;
    let cx = Cx::load(&entry)?;
    cx.extract(
        &flow.entry.file,
        &flow.entry.symbol,
        Some(&flow.flow),
        flow.entry.depth,
    )
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
    match Cli::parse().command {
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
        Command::Entrypoints { dir, json } => {
            let cx = Cx::load(&dir)?;
            let entries = cx.entrypoints();
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
            anyhow::ensure!(
                !matches!(resolver, ResolverMode::Scip),
                "SCIP resolver is not implemented until M6; use --resolver heuristic"
            );
            let (path, symbol) = entry
                .rsplit_once('#')
                .context("--entry must be FILE#SYMBOL")?;
            let path = Path::new(path)
                .canonicalize()
                .with_context(|| format!("{path}:1: entry"))?;
            let cx = Cx::load(&path)?;
            let relative = path
                .strip_prefix(&cx.config.root)?
                .to_string_lossy()
                .replace('\\', "/");
            let mut flow = cx.extract(&relative, symbol, name.as_deref(), depth)?;
            if let Some(old) = merge {
                labels::carry(&load(&old)?, &mut flow)?;
            }
            write(output.as_deref(), &yaml::emit(&flow)?)?;
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
        Command::Todo { flow, all, json } => {
            let f = load(&flow)?;
            f.verify_fingerprint()?;
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
                let old = load(&path)?;
                old.verify_fingerprint()?;
                let mut new = reextract(&path, &old)?;
                let changed = new.fingerprint != old.fingerprint;
                let kept = labels::carry(&old, &mut new)?;
                updates.push((path, yaml::emit(&new)?, changed, kept));
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
                let text = read(&path)?;
                let mut context = lint::Context {
                    verbose,
                    source,
                    ..lint::Context::default()
                };
                if source {
                    if let Ok(flow) = yaml::load(&text, &path.display().to_string()) {
                        context.source_changed =
                            reextract(&path, &flow)?.fingerprint != flow.fingerprint;
                    }
                }
                let diagnostics = lint::document(&text, &path.display().to_string(), &context);
                failed |= lint::fails(&diagnostics, strict);
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
            write(output.as_deref(), &text)?;
        }
        Command::DebugAst { file, range } => {
            let text = read(&file)?;
            let source =
                wtflow_extract::source::SourceFile::parse(file.to_string_lossy().into(), text)?;
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
            dump(node, &source, 0);
        }
        Command::DebugResolve { position } => {
            let (fileline, col) = position
                .rsplit_once(':')
                .context("position must be FILE:LINE:COL")?;
            let (file, line) = fileline
                .rsplit_once(':')
                .context("position must be FILE:LINE:COL")?;
            let file = Path::new(file).canonicalize()?;
            let cx = Cx::load(&file)?;
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
            println!("{}", serde_json::to_string_pretty(&resolution)?);
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
