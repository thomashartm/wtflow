//! All external indexer execution is confined to this module and the index command.
use anyhow::{Context, Result};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
    process::{Command, Stdio},
};
use wtflow_extract::{
    config::{Indexer, RepositoryConfig},
    source::{self, Language},
};
use wtflow_resolve::metadata::{hash, Metadata};
const DOCKER: &str = "docker run --rm -v $PWD:/src ghcr.io/aderiserp/wtflow-indexers wtflow index";
struct Job {
    name: &'static str,
    program: &'static str,
    prefix: Vec<&'static str>,
    args: Vec<String>,
    lang: Language,
}
struct IndexLog {
    reference: String,
    file: File,
}
impl IndexLog {
    fn create(root: &Path) -> Result<Self> {
        fs::create_dir_all(root.join(".wtflow/logs"))?;
        for number in 1..=u64::MAX {
            let reference = format!(".wtflow/logs/index-{number:06}.log");
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join(&reference))
            {
                Ok(file) => return Ok(Self { reference, file }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error).context("create index log"),
            }
        }
        anyhow::bail!("no available index log number")
    }
}
fn execute(
    root: &Path,
    job: &Job,
    args: &[String],
    capture: bool,
    log: &mut IndexLog,
) -> Result<String> {
    writeln!(
        log.file,
        "\n$ {} {} {}",
        job.program,
        job.prefix.join(" "),
        args.join(" ")
    )?;
    log.file.flush()?;
    let mut command = Command::new(job.program);
    command.current_dir(root).args(&job.prefix).args(args);
    crate::runtime::checkpoint()?;
    let mut captured = tempfile::tempfile()?;
    command.stdout(Stdio::from(if capture {
        captured.try_clone()?
    } else {
        log.file.try_clone()?
    }));
    command.stderr(Stdio::from(log.file.try_clone()?));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    use std::io::{Read, Seek};
    let mut live = File::open(root.join(&log.reference))?;
    live.seek(std::io::SeekFrom::End(0))?;
    let mut child = command.spawn().with_context(|| {
        format!(
            "missing tool {} for {}; Docker alternative: {DOCKER}",
            job.program, job.name
        )
    })?;
    let status = loop {
        let mut bytes = Vec::new();
        live.read_to_end(&mut bytes)?;
        if !bytes.is_empty() {
            crate::runtime::emit(crate::runtime::Event::Log(
                String::from_utf8_lossy(&bytes).into_owned(),
            ));
        }
        if crate::runtime::cancelled() {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            #[cfg(not(unix))]
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Operation cancelled; indexer stopped");
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let mut bytes = Vec::new();
    live.read_to_end(&mut bytes)?;
    if !bytes.is_empty() {
        crate::runtime::emit(crate::runtime::Event::Log(
            String::from_utf8_lossy(&bytes).into_owned(),
        ));
    }
    anyhow::ensure!(
        status.success(),
        "{} failed with {status}; see index log. Docker alternative: {DOCKER}",
        job.name
    );
    if capture {
        captured.rewind()?;
        let mut text = String::new();
        captured.read_to_string(&mut text)?;
        log.file.write_all(text.as_bytes())?;
        Ok(text.trim().to_owned())
    } else {
        Ok(String::new())
    }
}
fn languages(config: &RepositoryConfig) -> [(Language, &'static str, Option<&Indexer>); 3] {
    [
        (Language::Ts, "ts", config.config.index.typescript.as_ref()),
        (Language::Java, "java", config.config.index.java.as_ref()),
        (Language::Py, "py", config.config.index.python.as_ref()),
    ]
}

fn defaults(config: &RepositoryConfig, files: &[std::path::PathBuf]) -> Vec<&'static str> {
    languages(config)
        .into_iter()
        .filter_map(|(lang, short, setting)| {
            (setting.map_or(true, |s| s.enabled)
                && files.iter().any(|p| Language::from_path(p) == Some(lang)))
            .then_some(short)
        })
        .collect()
}

/// The TUI uses the same language defaults as an index command without --lang.
/// Inspect filenames only; indexing must not require successful source parsing.
pub fn default_languages(root: &Path) -> Result<Vec<&'static str>> {
    let config = RepositoryConfig::load(root)?;
    Ok(defaults(&config, &source::paths(root)?))
}

pub fn run(langs: &[String], force: bool, show_progress: bool) -> Result<()> {
    let config = RepositoryConfig::discover(&std::env::current_dir()?)?;
    let mut log = IndexLog::create(&config.root)?;
    let result = run_logged(&config, langs, force, show_progress, &mut log);
    let saved = match &result {
        Ok(()) => writeln!(log.file, "\nResult: success"),
        Err(error) => writeln!(log.file, "\nResult: failed\n{error:#}"),
    }
    .and_then(|()| log.file.sync_all());
    crate::err!("Index log: {} (relative to project root)", log.reference);
    saved.context("finish index log")?;
    result
}
fn run_logged(
    config: &RepositoryConfig,
    langs: &[String],
    force: bool,
    show_progress: bool,
    log: &mut IndexLog,
) -> Result<()> {
    let mut progress =
        crate::progress::Progress::start(show_progress, "Preparing project index...");
    let root = &config.root;
    let files = source::paths(root)?;
    let mut jobs = vec![];
    let defaults = defaults(config, &files);
    for (lang, short, setting) in languages(config) {
        let name = lang.name();
        let selected = if langs.is_empty() {
            defaults.contains(&short)
        } else {
            langs.iter().any(|s| s == short || s == name)
        };
        if !selected
            || !files
                .iter()
                .any(|file| Language::from_path(file) == Some(lang))
        {
            continue;
        }
        let fallback = Indexer {
            enabled: true,
            args: if lang == Language::Ts {
                vec!["--infer-tsconfig".into()]
            } else {
                Vec::new()
            },
            project_name: None,
        };
        let setting = setting.unwrap_or(&fallback);
        let output = format!(".wtflow/index/{name}.scip");
        let (program, prefix, mut args) = match lang {
            Language::Ts => (
                "npx",
                vec!["--yes", "@sourcegraph/scip-typescript"],
                vec!["index".into()],
            ),
            Language::Java => ("scip-java", vec![], vec!["index".into()]),
            Language::Py => (
                "npx",
                vec!["--yes", "@sourcegraph/scip-python"],
                vec![
                    "index".into(),
                    ".".into(),
                    "--project-name".into(),
                    setting.project_name.clone().unwrap_or_else(|| {
                        root.file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("project")
                            .to_owned()
                    }),
                ],
            ),
        };
        args.extend(setting.args.clone());
        args.extend(["--output".into(), output]);
        jobs.push(Job {
            name,
            program,
            prefix,
            args,
            lang,
        });
    }
    anyhow::ensure!(
        !jobs.is_empty(),
        "no languages selected for indexing: no supported source files found, or detected languages are disabled in {}; select a language with --lang or in the index form",
        config.path.display()
    );
    let mut meta = Metadata::load(root)?.unwrap_or_default();
    let mut selected = BTreeMap::new();
    for path in &files {
        if jobs
            .iter()
            .any(|j| Some(j.lang) == Language::from_path(path))
        {
            let bytes = fs::read(path)
                .with_context(|| format!("{}:1: read for indexing", path.display()))?;
            selected.insert(
                path.strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/"),
                hash(&bytes),
            );
        }
    }
    if !force
        && selected.iter().all(|(p, h)| meta.files.get(p) == Some(h))
        && jobs.iter().all(|j| {
            root.join(format!(".wtflow/index/{}.scip", j.name))
                .is_file()
        })
    {
        progress.finish();
        writeln!(log.file, "Index is up to date; no indexers were run.")?;
        crate::err!("index is up to date");
        return Ok(());
    }
    std::fs::create_dir_all(root.join(".wtflow/index"))?;
    progress.finish();
    let staging = tempfile::tempdir_in(root.join(".wtflow/index"))?;
    for job in &jobs {
        let _progress =
            crate::progress::Progress::start(show_progress, &format!("Indexing {}...", job.name));
        let version = execute(root, job, &["--version".into()], true, log)?;
        let staged = staging.path().join(format!("{}.scip", job.name));
        let mut args = job.args.clone();
        *args.last_mut().context("index output argument")? = staged.to_string_lossy().into_owned();
        execute(root, job, &args, false, log)?;
        anyhow::ensure!(
            staged.metadata().is_ok_and(|m| m.len() > 0),
            "{} did not produce its SCIP index",
            job.name
        );
        meta.indexers.insert(
            job.name.into(),
            version.lines().last().unwrap_or("unknown").into(),
        );
    }
    crate::runtime::checkpoint()?;
    for job in &jobs {
        fs::rename(
            staging.path().join(format!("{}.scip", job.name)),
            root.join(format!(".wtflow/index/{}.scip", job.name)),
        )?;
    }
    meta.files.retain(|p, _| {
        !jobs
            .iter()
            .any(|j| source::Language::from_path(Path::new(p)) == Some(j.lang))
    });
    meta.files.extend(selected);
    let git = |args: &[&str]| {
        Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    meta.commit = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    meta.dirty = git(&["status", "--porcelain"]).map_or(true, |s| !s.is_empty());
    super::write(Some(&root.join(".wtflow/index/meta.yaml")), &meta.emit()?)?;
    Ok(())
}

#[cfg(all(test, unix))]
mod cancellation_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    };
    #[test]
    fn cancelling_stops_the_indexer_and_its_children() {
        let root = tempfile::tempdir().unwrap();
        let mut log = IndexLog::create(root.path()).unwrap();
        let job = Job {
            name: "fixture",
            program: "/bin/sh",
            prefix: vec![],
            args: vec![],
            lang: Language::Ts,
        };
        let (events, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let waiter = std::thread::spawn(move || loop {
            match receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
            {
                crate::runtime::Event::Log(text) if text.contains("ready") => {
                    stop.store(true, Ordering::Relaxed);
                    break;
                }
                _ => {}
            }
        });
        let started = std::time::Instant::now();
        let result =
            crate::runtime::observe(crate::runtime::Observer { events, cancelled }, || {
                execute(
                    root.path(),
                    &job,
                    &[
                        "-c".into(),
                        "(sleep 2; touch survived) & echo ready; wait".into(),
                    ],
                    false,
                    &mut log,
                )
            });
        waiter.join().unwrap();
        assert!(result.unwrap_err().to_string().contains("cancelled"));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        std::thread::sleep(std::time::Duration::from_millis(2100));
        assert!(!root.path().join("survived").exists());
    }
}
