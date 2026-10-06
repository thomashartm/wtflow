use anyhow::{Context, Result};
use std::{
    collections::BTreeSet,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
};
use wtflow_core::{labels, lint, Flow};
use wtflow_extract::{config::RepositoryConfig, Cx, EntryPoint};

pub fn save_entries(root: &Path, entries: &[EntryPoint]) -> Result<()> {
    crate::write(
        Some(&root.join(".wtflow/entrypoints.json")),
        &format!("{}\n", serde_json::to_string_pretty(entries)?),
    )
}

fn choose(input: &mut impl BufRead, count: usize, allow_new: bool) -> Result<Option<usize>> {
    loop {
        eprint!(
            "Choose a number{}, or q to quit: ",
            if allow_new {
                ", n to analyze a new flow"
            } else {
                ""
            }
        );
        io::stderr().flush()?;
        let mut answer = String::new();
        if input.read_line(&mut answer)? == 0 || answer.trim().eq_ignore_ascii_case("q") {
            return Ok(None);
        }
        if allow_new && answer.trim().eq_ignore_ascii_case("n") {
            return Ok(Some(count));
        }
        if let Ok(number) = answer.trim().parse::<usize>() {
            if number > 0 && number <= count {
                return Ok(Some(number - 1));
            }
        }
        eprintln!(
            "Enter a number from 1 to {count}{} or q.",
            if allow_new { ", n," } else { "," }
        );
    }
}

pub fn run(directory: &Path, show_progress: bool) -> Result<()> {
    anyhow::ensure!(
        directory.is_dir(),
        "{}:1: not a directory",
        directory.display()
    );
    let config = RepositoryConfig::discover(directory)?;
    let mut progress = crate::progress::Progress::start(show_progress, "Finding saved flows...");
    let mut paths = BTreeSet::new();
    // Include the project's managed store even when browsing from a source subdirectory.
    for search in [directory.to_owned(), config.root.join(".wtflow/flows")] {
        if !search.is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&search)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                !matches!(
                    entry.file_name().to_str(),
                    Some(
                        ".git"
                            | ".wtflow"
                            | "node_modules"
                            | "target"
                            | "dist"
                            | "build"
                            | ".venv"
                            | ".gradle"
                    )
                )
            })
        {
            let entry =
                entry.with_context(|| format!("{}:1: scan flow documents", search.display()))?;
            if entry.file_type().is_file()
                && entry.file_name().to_string_lossy().ends_with(".flow.yaml")
            {
                paths.insert(entry.path().canonicalize()?);
            }
        }
    }
    let mut flows = Vec::new();
    let mut warnings = Vec::new();
    for path in paths {
        let loaded: Result<Flow> = (|| {
            let flow = crate::load(&path)?;
            flow.verify_fingerprint()?;
            Ok(flow)
        })();
        match loaded {
            Ok(flow) => flows.push((path, flow)),
            Err(error) => {
                warnings.push(format!("{}: {error:#}", display_path(&path, &config.root)))
            }
        }
    }
    progress.finish();
    for warning in warnings {
        eprintln!("warning: skipped flow {warning}");
    }
    let mut input = io::stdin().lock();
    if flows.is_empty() {
        println!("No saved flows yet. Choose an entrypoint to analyze and save in .wtflow/flows/.");
        return create(&config.root, &mut input, show_progress);
    }
    for (index, (path, flow)) in flows.iter().enumerate() {
        println!(
            "{}. {} ({})",
            index + 1,
            flow.flow,
            display_path(path, &config.root)
        );
        println!("   {}#{}", flow.entry.file, flow.entry.symbol);
    }
    match choose(&mut input, flows.len(), true)? {
        Some(index) if index == flows.len() => create(&config.root, &mut input, show_progress),
        Some(index) => render(
            &flows[index].0,
            &flows[index].1,
            &config.root,
            show_progress,
        ),
        None => Ok(()),
    }
}

fn display_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn create(root: &Path, input: &mut impl BufRead, show_progress: bool) -> Result<()> {
    let mut progress =
        crate::progress::Progress::start(show_progress, "Finding entrypoints to analyze...");
    let (mut cx, warnings) = Cx::load_entrypoints(root)?;
    let entries = cx.entrypoints();
    save_entries(root, &entries)?;
    progress.finish();
    for warning in &warnings {
        eprintln!("warning: {warning}");
    }
    if entries.is_empty() {
        println!("No recognized entrypoints found. Use `wtflow extract --entry FILE#SYMBOL -o .wtflow/flows/name.flow.yaml` for a function you choose from the code.");
        return Ok(());
    }
    for (index, entry) in entries.iter().enumerate() {
        println!(
            "{}. {} — {}#{}",
            index + 1,
            entry.trigger,
            entry.file,
            entry.symbol
        );
    }
    let Some(index) = choose(input, entries.len(), false)? else {
        return Ok(());
    };
    let entry = &entries[index];
    let mut progress =
        crate::progress::Progress::start(show_progress, "Analyzing and saving flow...");
    cx.enable_scip(false)?;
    let mut flow = cx.extract(&entry.file, &entry.symbol, None, 2)?;
    let path = saved_path(root, entry);
    if path.exists() {
        let old = crate::load(&path)?;
        anyhow::ensure!(
            old.entry.file == entry.file && old.entry.symbol == entry.symbol,
            "{}:1: saved flow filename collision",
            display_path(&path, root)
        );
        labels::carry(&old, &mut flow)?;
    }
    let mut diagnostics = warnings;
    for file in cx.stale_for(&flow) {
        diagnostics.push(format!("warning W120 - stale index for {file}"));
    }
    diagnostics.extend(
        lint::check(&flow, &lint::Context::default())
            .into_iter()
            .map(|diagnostic| diagnostic.to_string()),
    );
    let text = wtflow_core::yaml::emit(&flow)?;
    crate::write(Some(&path), &text)?;
    let diagnostics_path = path.with_extension("lint.txt");
    let report = if diagnostics.is_empty() {
        String::new()
    } else {
        format!("{}\n", diagnostics.join("\n"))
    };
    crate::write(Some(&diagnostics_path), &report)?;
    progress.finish();
    println!("Saved flow: {}", display_path(&path, root));
    render(&path, &flow, root, show_progress)
}

fn saved_path(root: &Path, entry: &EntryPoint) -> PathBuf {
    let slug: String = entry
        .symbol
        .chars()
        .take(60)
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let hash =
        wtflow_resolve::metadata::hash(format!("{}\0{}", entry.file, entry.symbol).as_bytes());
    root.join(".wtflow/flows")
        .join(format!("{slug}-{}.flow.yaml", &hash[..12]))
}

fn render(path: &Path, flow: &Flow, root: &Path, show_progress: bool) -> Result<()> {
    let mut progress = crate::progress::Progress::start(show_progress, "Rendering diagram...");
    let diagram = wtflow_render::render(flow, &wtflow_render::Options::default())?;
    let output = path.with_extension("md");
    crate::write(Some(&output), &format!("```mermaid\n{diagram}```\n"))?;
    progress.finish();
    println!("Diagram: {}", display_path(&output, root));
    let notes = path.with_extension("lint.txt");
    if notes.metadata().is_ok_and(|metadata| metadata.len() > 0) {
        println!(
            "Analysis notes: {} (review before relying on the diagram)",
            display_path(&notes, root)
        );
    }
    println!("Open this file in a Markdown viewer that supports Mermaid.");
    Ok(())
}
