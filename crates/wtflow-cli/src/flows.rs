use anyhow::{Context, Result};
use std::{
    collections::BTreeSet,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
};
use wtflow_core::Flow;
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
    crate::out!("Project: {}", config.root.display());
    let mut progress = crate::progress::Progress::start(show_progress, "Finding saved flows...");
    let paths = saved_paths(&directory, &config)?;
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
        crate::err!("warning: skipped flow {warning}");
    }
    for warning in &source_warnings {
        crate::err!("warning: {warning}");
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
        crate::out!("No recognized entrypoints or saved flows found in {}. Use `wtflow flows --dir /path/to/project` to choose a project.", config.root.display());
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
        crate::out!("Source unavailable; opening the saved snapshot.");
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
    cx: Cx,
    warnings: Vec<String>,
    entry: &EntryPoint,
    path: &Path,
    show_progress: bool,
    open: bool,
) -> Result<()> {
    let _ = show_progress;
    let args = crate::settings::OutputArgs::default();
    let entry_spec = format!(
        "{}#{}",
        cx.config.root.join(&entry.file).display(),
        entry.symbol
    );
    crate::settings::analyze(
        crate::settings::AnalysisSource {
            entry: &entry_spec,
            prepared: Some((cx, warnings)),
            flows_dir: None,
        },
        None,
        None,
        None,
        &args,
        true,
        Some(path),
    )?;
    if open && io::stdin().is_terminal() && io::stdout().is_terminal() {
        let view = path.with_extension("html");
        if view.exists() {
            open_view(&view)?;
        }
    }
    Ok(())
}

fn saved_path(root: &Path, entry: &EntryPoint) -> PathBuf {
    let dir = RepositoryConfig::load(root)
        .map(|c| c.config.output.flows_dir)
        .unwrap_or_else(|_| ".wtflow/flows".into());
    crate::settings::flow_path(root, &dir, &entry.file, &entry.symbol)
}

fn render(path: &Path, flow: &Flow, root: &Path, show_progress: bool, open: bool) -> Result<()> {
    let _ = (flow, root, show_progress);
    crate::settings::export(
        path,
        &crate::settings::OutputArgs::default(),
        open && io::stdin().is_terminal() && io::stdout().is_terminal(),
    )
}

pub(crate) fn open_view(path: &Path) -> Result<()> {
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

/// Shared saved-flow inventory for both frontends.
pub fn saved_paths(directory: &Path, config: &RepositoryConfig) -> Result<BTreeSet<PathBuf>> {
    let mut paths = BTreeSet::new();
    // Normal browsing reads the project's stores, never every YAML in its tree.
    // Explicit subdirectories still support legacy documents outside docs/flows.
    let searches = if directory == config.root.as_path() {
        vec![
            (directory.to_owned(), 1),
            (config.root.join("docs/flows"), usize::MAX),
        ]
    } else {
        vec![(directory.to_owned(), usize::MAX)]
    };
    for (search, depth) in searches.into_iter().chain([
        (
            config.root.join(&config.config.output.flows_dir),
            usize::MAX,
        ),
        (config.root.join(".wtflow/flows"), usize::MAX),
    ]) {
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
    Ok(paths)
}
