//! Shared execution for CLI and TUI requests.
use crate::*;

pub fn execute(command: Command, show_progress: bool) -> Result<i32> {
    runtime::checkpoint()?;
    match command {
        Command::Tui { .. } => anyhow::bail!("TUI is a frontend, not a background operation"),
        Command::Analyze {
            entry,
            name,
            depth,
            resolver,
            no_open,
            flows_dir,
            output,
        } => {
            settings::analyze(
                settings::AnalysisSource {
                    entry: &entry,
                    prepared: None,
                    flows_dir: flows_dir.as_deref(),
                },
                name.as_deref(),
                depth,
                resolver,
                &output,
                no_open,
                None,
            )?;
        }
        Command::Export { flow, open, output } => settings::export(&flow, &output, open)?,
        Command::Config {
            dir,
            key,
            value,
            json,
        } => settings::config(&dir, key.as_deref(), value.as_deref(), json)?,
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
        Command::Init { dir, lang, owner } => {
            if lang.is_empty() {
                init::run(&dir)?;
            } else {
                init::unattended(&dir, &lang, owner.as_deref())?;
            }
        }
        Command::Clear {
            dir,
            yes,
            remove_config,
        } => {
            if yes {
                clear::unattended(&dir, !remove_config)?;
            } else {
                clear::run(&dir)?;
            }
        }
        Command::Index { lang, force } => index::run(&lang, force, show_progress)?,
        Command::Version => crate::out!("wtflow {}", env!("CARGO_PKG_VERSION")),
        Command::Schema { json, config } => {
            if json {
                crate::out!(
                    "{}",
                    serde_json::to_string_pretty(&wtflow_core::schema::value(config)?)?
                );
            } else {
                crate::out_raw!(
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
                crate::err!("warning: {warning}");
            }
            if json {
                crate::out!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                for e in entries {
                    crate::out!("{}#{} {}", e.file, e.symbol, e.trigger);
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
            let analyzed =
                settings::extract(&entry, name.as_deref(), depth, resolver, merge.as_deref())?;
            let text = yaml::emit(&analyzed.flow)?;
            progress.finish();
            for note in analyzed.notes {
                crate::err!("note: {note}");
            }
            write(output.as_deref(), &text)?;
        }
        Command::LabelStep { flow, id, text } => {
            let mut f = load(&flow)?;
            labels::apply(&mut f, &BTreeMap::from([(id, text)]))?;
            write(Some(&flow), &yaml::emit(&f)?)?;
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
                    crate::err!("warning W120 - stale index for {file}");
                }
                crate::out!("{text}");
                return Ok(0);
            }
            let todo = labels::todo(&f, all);
            if json {
                crate::out!("{}", serde_json::to_string_pretty(&todo)?);
            } else {
                for n in todo {
                    crate::out!(
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
                    crate::err!("warning W120 - stale index for {file}");
                }
            }
            for (path, text, changed, kept) in updates {
                write(Some(&path), &text)?;
                crate::out!("structure changed={changed}, labels kept={kept}");
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
                    crate::out!("{d}");
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
            let source = locate(&flow, &f).ok();
            let config = wtflow_extract::config::RepositoryConfig::discover(
                source.as_deref().unwrap_or(Path::new(".")),
            )?;
            let mut options = config.config.output;
            if let Some(lang) = lang {
                options.lang = match lang {
                    Language::En => "en",
                    Language::De => "de",
                }
                .into();
            }
            if let Some(detail) = detail {
                options.detail = detail;
            }
            let graph = settings::diagram(&f, &options)?;
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
                crate::out!(
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
            crate::out!("{text}");
        }
    }
    Ok(0)
}
