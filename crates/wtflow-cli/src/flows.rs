use anyhow::{Context, Result};
use std::{
    collections::BTreeSet,
    io::{self, IsTerminal},
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

pub fn run(directory: &Path, show_progress: bool, open: bool, filter: &str) -> Result<()> {
    anyhow::ensure!(
        directory.is_dir(),
        "{}:1: not a directory",
        directory.display()
    );
    let config = RepositoryConfig::discover(directory)?;
    let directory = directory.canonicalize()?;
    println!("Project: {}", config.root.display());
    let mut progress = crate::progress::Progress::start(show_progress, "Finding saved flows...");
    let mut paths = BTreeSet::new();
    // Normal browsing reads the project's stores, never every YAML in its tree.
    // Explicit subdirectories still support legacy documents outside docs/flows.
    let searches = if directory == config.root {
        vec![
            (directory.clone(), 1),
            (config.root.join("docs/flows"), usize::MAX),
        ]
    } else {
        vec![(directory.clone(), usize::MAX)]
    };
    for (search, depth) in searches
        .into_iter()
        .chain([(config.root.join(".wtflow/flows"), usize::MAX)])
    {
        if !search.is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&search)
            .max_depth(depth)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                entry.depth() == 0
                    || !matches!(
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
                                | "testdata"
                                | "fixtures"
                                | "__fixtures__"
                                | "snapshots"
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
    let mut progress =
        crate::progress::Progress::start(show_progress, "Finding all entrypoints...");
    let (cx, source_warnings) = Cx::load_entrypoints(&config.root)?;
    let entries = cx.entrypoints();
    save_entries(&config.root, &entries)?;
    progress.finish();
    for warning in &warnings {
        eprintln!("warning: skipped flow {warning}");
    }
    for warning in &source_warnings {
        eprintln!("warning: {warning}");
    }
    let saved_entries: BTreeSet<_> = flows
        .iter()
        .map(|(_, flow)| (flow.entry.file.clone(), flow.entry.symbol.clone()))
        .collect();
    let mut choices: Vec<_> = flows
        .iter()
        .map(|(path, flow)| {
            (
                EntryPoint {
                    file: flow.entry.file.clone(),
                    symbol: flow.entry.symbol.clone(),
                    trigger: flow.trigger.clone(),
                    lang: flow.entry.lang.clone(),
                },
                path.clone(),
                Some(flow),
            )
        })
        .collect();
    choices.extend(
        entries
            .into_iter()
            .filter(|entry| !saved_entries.contains(&(entry.file.clone(), entry.symbol.clone())))
            .map(|entry| {
                let path = saved_path(&config.root, &entry);
                (entry, path, None)
            }),
    );
    choices.sort_by(|a, b| (&a.0.file, &a.0.symbol, &a.1).cmp(&(&b.0.file, &b.0.symbol, &b.1)));
    if choices.is_empty() {
        println!("No recognized entrypoints or saved flows found in {}. Use `wtflow flows --dir /path/to/project` to choose a project.", config.root.display());
        return Ok(());
    }
    let items: Vec<_> = choices
        .iter()
        .map(|(entry, path, saved)| crate::picker::Item {
            title: format!(
                "{}{}",
                if entry.trigger.is_empty() {
                    saved.map(|f| f.flow.as_str()).unwrap_or(&entry.symbol)
                } else {
                    &entry.trigger
                },
                if saved.is_some() { " [saved]" } else { "" }
            ),
            symbol: entry.symbol.clone(),
            path: if saved.is_some() {
                format!("{} | {}", entry.file, display_path(path, &config.root))
            } else {
                entry.file.clone()
            },
            is_test: crate::picker::is_test_file(&entry.file),
        })
        .collect();
    let mut input = io::stdin().lock();
    let Some(index) =
        crate::picker::select(&mut input, &config.root, "All flows", &items, false, filter)?
    else {
        return Ok(());
    };
    let (entry, path, saved) = &choices[index];
    if let Some(old) = saved.filter(|_| !config.root.join(&entry.file).is_file()) {
        println!("Source unavailable; opening the saved snapshot.");
        render(path, old, &config.root, show_progress, open)
    } else {
        let mut progress =
            crate::progress::Progress::start(show_progress, "Preparing call resolution...");
        let cx = cx.into_context()?;
        progress.finish();
        analyze(cx, source_warnings, entry, path, show_progress, open)
    }
}

fn display_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn analyze(
    mut cx: Cx,
    warnings: Vec<String>,
    entry: &EntryPoint,
    path: &Path,
    show_progress: bool,
    open: bool,
) -> Result<()> {
    let mut progress = crate::progress::Progress::start(
        show_progress,
        "Following internal calls and saving flow...",
    );
    cx.enable_scip(false)?;
    let root = &cx.config.root;
    let old = if path.exists() {
        Some(crate::load(path)?)
    } else {
        None
    };
    if let Some(old) = &old {
        anyhow::ensure!(
            old.entry.file == entry.file && old.entry.symbol == entry.symbol,
            "{}:1: saved flow filename collision",
            display_path(path, root)
        );
    }
    let extracted = cx.extract_report(
        &entry.file,
        &entry.symbol,
        old.as_ref().map(|flow| flow.flow.as_str()),
        wtflow_extract::DEFAULT_DEPTH,
    )?;
    let mut flow = extracted.flow;
    if let Some(old) = old {
        labels::carry(&old, &mut flow)?;
    }
    let mut diagnostics = warnings;
    diagnostics.extend(extracted.notes);
    for file in cx.stale_for(&flow) {
        diagnostics.push(format!("warning W120 - stale index for {file}"));
    }
    diagnostics.extend(
        lint::check(&flow, &lint::Context::default())
            .into_iter()
            .map(|diagnostic| diagnostic.to_string()),
    );
    let text = wtflow_core::yaml::emit(&flow)?;
    crate::write(Some(path), &text)?;
    let diagnostics_path = path.with_extension("lint.txt");
    let report = if diagnostics.is_empty() {
        String::new()
    } else {
        format!("{}\n", diagnostics.join("\n"))
    };
    crate::write(Some(&diagnostics_path), &report)?;
    let context = wtflow_extract::context::flow_context(&cx, &flow);
    crate::write(
        Some(&path.with_extension("context.json")),
        &format!("{}\n", serde_json::to_string_pretty(&context)?),
    )?;
    progress.finish();
    println!("Saved flow: {}", display_path(path, root));
    render(path, &flow, root, show_progress, open)
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

fn render(path: &Path, flow: &Flow, root: &Path, show_progress: bool, open: bool) -> Result<()> {
    let mut progress = crate::progress::Progress::start(show_progress, "Rendering diagram...");
    let diagram = wtflow_render::render(flow, &wtflow_render::Options::default())?;
    let output = path.with_extension("md");
    crate::write(Some(&output), &format!("```mermaid\n{diagram}```\n"))?;
    let notes = path.with_extension("lint.txt");
    let report = if notes.exists() {
        crate::read(&notes)?
    } else {
        String::new()
    };
    let view = path.with_extension("html");
    let context_path = path.with_extension("context.json");
    let context: wtflow_core::context::FlowContext = if context_path.exists() {
        let context: wtflow_core::context::FlowContext =
            serde_json::from_str(&crate::read(&context_path)?)
                .with_context(|| format!("{}: invalid call context", context_path.display()))?;
        if context.fingerprint == flow.fingerprint {
            context
        } else {
            wtflow_core::context::FlowContext::default()
        }
    } else {
        wtflow_core::context::FlowContext::default()
    };
    crate::write(
        Some(&view),
        &wtflow_render::html::render_with_context(flow, &report, &context),
    )?;
    progress.finish();
    println!("Diagram: {}", display_path(&output, root));
    let notes = path.with_extension("lint.txt");
    if notes.metadata().is_ok_and(|metadata| metadata.len() > 0) {
        println!(
            "Analysis notes: {} (review before relying on the diagram)",
            display_path(&notes, root)
        );
    }
    println!("Flow view: {}", display_path(&view, root));
    if open && io::stdin().is_terminal() && io::stdout().is_terminal() {
        if let Err(error) = open_view(&view) {
            eprintln!(
                "Could not open the browser: {error:#}. Open {} directly.",
                view.display()
            );
        }
    } else {
        println!("Open the HTML file in any browser; no plugins or internet connection needed.");
    }

    Ok(())
}

fn open_view(path: &Path) -> Result<()> {
    let path = path.canonicalize()?;
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(&path).status()?;
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("explorer.exe")
        .arg(&path)
        .status()?;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let status = std::process::Command::new("xdg-open").arg(&path).status()?;
    anyhow::ensure!(status.success(), "browser opener exited with {status}");
    Ok(())
}
