//! Project defaults and artifact generation shared by CLI and TUI.
use crate::*;
use wtflow_extract::config::{OutputConfig, RepositoryConfig};

#[derive(clap::Args, Clone, Default)]
pub struct OutputArgs {
    /// Directory for rendered exports (defaults to the saved flow directory)
    #[arg(long)]
    pub export_dir: Option<PathBuf>,
    /// Export formats, separated by commas
    #[arg(long, value_delimiter = ',', value_parser = ["html", "md", "mmd", "lint"])]
    pub formats: Option<Vec<String>>,
    #[arg(long, value_parser = ["en", "de"])]
    pub lang: Option<String>,
    /// Show source alongside labels; accepts an explicit true or false
    #[arg(long, num_args = 0..=1, default_missing_value = "true", require_equals = true)]
    pub detail: Option<bool>,
    #[arg(long, value_parser = ["TD", "LR"])]
    pub direction: Option<String>,
    #[arg(long, value_parser = ["default", "light", "dark"])]
    pub theme: Option<String>,
    /// Expand all calls initially in HTML
    #[arg(long, num_args = 0..=1, default_missing_value = "true", require_equals = true)]
    pub expanded: Option<bool>,
    /// Explicit filename for a single export format
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}
impl OutputArgs {
    pub fn resolve(&self, base: &OutputConfig) -> OutputConfig {
        let mut value = base.clone();
        if let Some(v) = &self.export_dir {
            value.export_dir = Some(v.clone());
        }
        if let Some(v) = &self.formats {
            value.formats = v.clone();
        }
        if let Some(v) = &self.lang {
            value.lang = v.clone();
        }
        if let Some(v) = self.detail {
            value.detail = v;
        }
        if let Some(v) = &self.direction {
            value.direction = v.clone();
        }
        if let Some(v) = &self.theme {
            value.theme = v.clone();
        }
        if let Some(v) = self.expanded {
            value.expanded = v;
        }
        value
    }
}
pub struct Analysis {
    pub flow: Flow,
    pub context: wtflow_core::context::FlowContext,
    pub notes: Vec<String>,
    pub root: PathBuf,
}
pub fn extract(
    entry: &str,
    name: Option<&str>,
    depth: Option<usize>,
    resolver: Option<ResolverMode>,
    merge: Option<&Path>,
) -> Result<Analysis> {
    runtime::phase("Preparing call resolution...");
    let (file, symbol) = entry
        .rsplit_once('#')
        .context("--entry must be FILE#SYMBOL")?;
    let file = Path::new(file)
        .canonicalize()
        .with_context(|| format!("{file}:1: entry"))?;
    let cx = Cx::load(&file)?;
    extract_using(cx, &file, symbol, name, depth, resolver, merge)
}
fn extract_using(
    mut cx: Cx,
    file: &Path,
    symbol: &str,
    name: Option<&str>,
    depth: Option<usize>,
    resolver: Option<ResolverMode>,
    merge: Option<&Path>,
) -> Result<Analysis> {
    let mode = resolver.unwrap_or(match cx.config.config.analysis.resolver.as_str() {
        "scip" => ResolverMode::Scip,
        "heuristic" => ResolverMode::Heuristic,
        _ => ResolverMode::Auto,
    });
    if !matches!(mode, ResolverMode::Heuristic) {
        cx.enable_scip(matches!(mode, ResolverMode::Scip))?;
    }
    runtime::checkpoint()?;
    runtime::phase("Following calls and branches...");
    let relative = file
        .strip_prefix(&cx.config.root)?
        .to_string_lossy()
        .replace('\\', "/");
    let report = cx.extract_report(
        &relative,
        symbol,
        name,
        depth.unwrap_or(cx.config.config.analysis.depth),
    )?;
    let mut flow = report.flow;
    if let Some(path) = merge {
        labels::carry(&load(path)?, &mut flow)?;
    }
    let mut notes = report.notes;
    notes.extend(
        cx.stale_for(&flow)
            .into_iter()
            .map(|f| format!("warning W120 - stale index for {f}")),
    );
    let context = wtflow_extract::context::flow_context(&cx, &flow);
    runtime::checkpoint()?;
    Ok(Analysis {
        flow,
        context,
        notes,
        root: cx.config.root.clone(),
    })
}

pub struct AnalysisSource<'a> {
    pub entry: &'a str,
    pub prepared: Option<(Cx, Vec<String>)>,
    pub flows_dir: Option<&'a Path>,
}
pub fn analyze(
    source: AnalysisSource<'_>,
    name: Option<&str>,
    depth: Option<usize>,
    resolver: Option<ResolverMode>,
    args: &OutputArgs,
    no_open: bool,
    saved: Option<&Path>,
) -> Result<PathBuf> {
    let (file, symbol) = source
        .entry
        .rsplit_once('#')
        .context("--entry must be FILE#SYMBOL")?;
    let config = RepositoryConfig::discover(Path::new(file))?;
    let mut options = args.resolve(&config.config.output);
    if let Some(dir) = source.flows_dir {
        options.flows_dir = dir.to_owned();
    }
    anyhow::ensure!(
        args.output.is_none() || options.formats.len() == 1,
        "--output requires exactly one export format"
    );
    let relative = Path::new(file)
        .canonicalize()?
        .strip_prefix(&config.root)?
        .to_string_lossy()
        .replace('\\', "/");
    let path = saved
        .map(Path::to_owned)
        .unwrap_or_else(|| flow_path(&config.root, &options.flows_dir, &relative, symbol));
    let old = path.exists().then(|| load(&path)).transpose()?;
    if let Some(old) = &old {
        anyhow::ensure!(
            old.entry.file == relative && old.entry.symbol == symbol,
            "saved flow filename collision"
        );
    }
    let (cx, warnings) = if let Some(prepared) = source.prepared {
        prepared
    } else {
        let (catalog, warnings) = Cx::load_entrypoints(&config.root)?;
        (catalog.into_context()?, warnings)
    };
    let mut analyzed = extract_using(
        cx,
        &Path::new(file).canonicalize()?,
        symbol,
        name.or_else(|| old.as_ref().map(|f| f.flow.as_str())),
        depth,
        resolver,
        old.as_ref().map(|_| path.as_path()),
    )?;
    analyzed.notes.extend(warnings);
    let mut notes = analyzed.notes;
    notes.extend(
        lint::check(&analyzed.flow, &lint::Context::default())
            .into_iter()
            .map(|d| d.to_string()),
    );
    let report = if notes.is_empty() {
        String::new()
    } else {
        format!("{}\n", notes.join("\n"))
    };
    runtime::phase("Saving flow and analysis context...");
    write(Some(&path), &yaml::emit(&analyzed.flow)?)?;
    write(
        Some(&path.with_extension("context.json")),
        &serde_json::to_string_pretty(&analyzed.context)?,
    )?;
    write(Some(&path.with_extension("lint.txt")), &report)?;
    crate::out!(
        "Saved flow: {}",
        path.strip_prefix(&config.root).unwrap_or(&path).display()
    );
    for note in notes {
        crate::err!("{note}");
    }
    export_with(
        &path,
        &analyzed.flow,
        &analyzed.root,
        &options,
        args.output.as_deref(),
        options.open && !no_open,
    )?;
    runtime::emit(runtime::Event::FlowSaved(path.clone()));
    Ok(path)
}
pub fn flow_path(root: &Path, dir: &Path, file: &str, symbol: &str) -> PathBuf {
    let slug: String = symbol
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
    let hash = wtflow_resolve::metadata::hash(format!("{file}\0{symbol}").as_bytes());
    root.join(dir)
        .join(format!("{slug}-{}.flow.yaml", &hash[..12]))
}
pub fn export(path: &Path, args: &OutputArgs, open: bool) -> Result<()> {
    let flow = load(path)?;
    let root = locate(path, &flow)
        .ok()
        .and_then(|p| RepositoryConfig::discover_root(&p).ok())
        .unwrap_or(RepositoryConfig::discover_root(&std::env::current_dir()?)?);
    let config = RepositoryConfig::load(&root)?;
    export_with(
        path,
        &flow,
        &root,
        &args.resolve(&config.config.output),
        args.output.as_deref(),
        open,
    )
}
fn export_with(
    path: &Path,
    flow: &Flow,
    root: &Path,
    options: &OutputConfig,
    explicit: Option<&Path>,
    open: bool,
) -> Result<()> {
    flow.verify_fingerprint()?;
    anyhow::ensure!(
        explicit.is_none() || options.formats.len() == 1,
        "--output requires exactly one export format"
    );
    let base = options
        .export_dir
        .as_ref()
        .map(|dir| root.join(dir).join(path.file_name().unwrap_or_default()))
        .unwrap_or_else(|| path.to_owned());
    let context = path.with_extension("context.json");
    let context = if context.is_file() {
        serde_json::from_str(&read(&context)?)?
    } else {
        wtflow_core::context::FlowContext::default()
    };
    let notes = path.with_extension("lint.txt");
    let report = if notes.is_file() {
        read(&notes)?
    } else {
        let lines = lint::check(flow, &lint::Context::default())
            .into_iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>();
        if lines.is_empty() {
            String::new()
        } else {
            lines.join("\n") + "\n"
        }
    };
    let mut browser = None;
    for format in &options.formats {
        runtime::phase(&format!("Rendering {format}..."));
        let extension = if format == "lint" {
            "lint.txt"
        } else {
            format.as_str()
        };
        let destination = explicit
            .map(|p| root.join(p))
            .unwrap_or_else(|| base.with_extension(extension));
        anyhow::ensure!(
            destination.canonicalize().ok() != Some(path.canonicalize()?),
            "export would overwrite the saved flow"
        );
        let text = match format.as_str() {
            "html" => {
                browser = Some(destination.clone());
                wtflow_render::html::render_styled(
                    flow,
                    &report,
                    &context,
                    &options.theme,
                    options.expanded,
                    options.detail,
                )
            }
            "lint" => report.clone(),
            _ => {
                let diagram = diagram(flow, options)?;
                if format == "md" {
                    format!("```mermaid\n{diagram}```\n")
                } else {
                    diagram
                }
            }
        };
        write(Some(&destination), &text)?;
        crate::out!("Export: {}", destination.display());
    }
    if open {
        if let Some(path) = browser {
            crate::flows::open_view(&path)?;
        }
    }
    Ok(())
}
pub fn diagram(flow: &Flow, options: &OutputConfig) -> Result<String> {
    let graph = wtflow_render::render(
        flow,
        &wtflow_render::Options {
            detail: options.detail,
            lang: if options.lang == "de" {
                wtflow_render::Language::De
            } else {
                wtflow_render::Language::En
            },
        },
    )?;
    let graph = graph.replacen(
        "flowchart TD",
        &format!("flowchart {}", options.direction),
        1,
    );
    Ok(if options.theme == "default" {
        graph
    } else {
        format!(
            "%%{{init: {{\"theme\": \"{}\"}}}}%%\n{graph}",
            if options.theme == "light" {
                "default"
            } else {
                "dark"
            }
        )
    })
}

pub fn config(dir: &Path, key: Option<&str>, value: Option<&str>, json: bool) -> Result<()> {
    let config = RepositoryConfig::discover(dir)?;
    let original = if config.path.exists() {
        read(&config.path)?
    } else {
        String::new()
    };
    let mut document: serde_json::Value = if original.is_empty() {
        serde_json::json!({})
    } else {
        serde_yaml_ng::from_str(&original)?
    };
    if let (Some(key), Some(value)) = (key, value) {
        let parts: Vec<_> = key.split('.').collect();
        anyhow::ensure!(parts.iter().all(|p| !p.is_empty()), "invalid setting key");
        let mut cursor = &mut document;
        for part in &parts[..parts.len() - 1] {
            let map = cursor
                .as_object_mut()
                .context("setting parent is not an object")?;
            cursor = map.entry(*part).or_insert_with(|| serde_json::json!({}));
        }
        let value: serde_json::Value =
            serde_yaml_ng::from_str(value).context("value must be valid YAML")?;
        cursor
            .as_object_mut()
            .context("setting parent is not an object")?
            .insert(parts[parts.len() - 1].into(), value);
        wtflow_core::schema::validate(&document, true)?;
        // Validate compiled regexes too, before replacing the current file.
        validate_rules(&document)?;
        let text = replace_section(&original, &document, parts[0])?;
        write(Some(&config.path), &text)?;
        crate::out!("Saved {} in {}", key, config.path.display());
    } else {
        crate::out_raw!(
            "{}",
            if json {
                serde_json::to_string_pretty(&document)? + "\n"
            } else {
                serde_yaml_ng::to_string(&document)?
            }
        );
    }
    Ok(())
}
fn validate_rules(document: &serde_json::Value) -> Result<()> {
    // Reuse the same loader, including semantic validation, in an isolated directory.
    let temporary = tempfile::tempdir()?;
    std::fs::create_dir(temporary.path().join(".wtflow"))?;
    std::fs::write(
        temporary.path().join(".wtflow/config.yaml"),
        serde_yaml_ng::to_string(document)?,
    )?;
    RepositoryConfig::load(temporary.path())?;
    Ok(())
}

// Preserve unrelated YAML sections and comments. Validate the replacement's
// meaning before using it; flow-style mappings and aliases need reserialization.
fn replace_section(original: &str, document: &serde_json::Value, key: &str) -> Result<String> {
    let section = serde_yaml_ng::to_string(&serde_json::json!({key: document[key]}))?;
    let lines: Vec<_> = original.split_inclusive('\n').collect();
    let start = lines.iter().position(|line| {
        !line.starts_with(char::is_whitespace)
            && line
                .split_once(':')
                .is_some_and(|(name, _)| name.trim_matches(['\'', '"']) == key)
    });
    let mut candidate = String::new();
    if let Some(start) = start {
        let end = (start + 1..lines.len())
            .find(|&i| {
                !lines[i].trim().is_empty()
                    && !lines[i].starts_with(char::is_whitespace)
                    && !lines[i].starts_with('#')
            })
            .unwrap_or(lines.len());
        candidate.extend(lines[..start].iter().copied());
        candidate.push_str(&section);
        candidate.extend(lines[end..].iter().copied());
    } else {
        candidate.push_str(original);
        if !candidate.is_empty() && !candidate.ends_with('\n') {
            candidate.push('\n');
        }
        candidate.push_str(&section);
    }
    if serde_yaml_ng::from_str::<serde_json::Value>(&candidate)
        .ok()
        .as_ref()
        == Some(document)
    {
        Ok(candidate)
    } else {
        Ok(serde_yaml_ng::to_string(document)?)
    }
}
