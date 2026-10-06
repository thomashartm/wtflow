//! All external indexer execution is confined to this module and the index command.
use anyhow::{Context, Result};
use std::{
    collections::BTreeMap,
    path::Path,
    process::{Command, Stdio},
};
use wtflow_extract::{
    config::RepositoryConfig,
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
fn execute(root: &Path, job: &Job, args: &[String], capture: bool) -> Result<String> {
    let mut command = Command::new(job.program);
    command.current_dir(root).args(&job.prefix).args(args);
    if capture {
        let result = command.output().with_context(|| {
            format!(
                "missing tool {} for {}; Docker alternative: {DOCKER}",
                job.program, job.name
            )
        })?;
        anyhow::ensure!(
            result.status.success(),
            "{} failed: {}; Docker alternative: {DOCKER}",
            job.name,
            String::from_utf8_lossy(&result.stderr)
        );
        Ok(String::from_utf8(result.stdout)?.trim().to_owned())
    } else {
        let status = command
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .with_context(|| {
                format!(
                    "missing tool {} for {}; Docker alternative: {DOCKER}",
                    job.program, job.name
                )
            })?;
        anyhow::ensure!(
            status.success(),
            "{} failed with {status}; Docker alternative: {DOCKER}",
            job.name
        );
        Ok(String::new())
    }
}
pub fn run(langs: &[String], force: bool) -> Result<()> {
    let config = RepositoryConfig::discover(&std::env::current_dir()?)?;
    let root = &config.root;
    let files = source::load(root)?;
    let mut jobs = vec![];
    for (lang, short, name, setting) in [
        (
            Language::Ts,
            "ts",
            "typescript",
            config.config.index.typescript.as_ref(),
        ),
        (
            Language::Java,
            "java",
            "java",
            config.config.index.java.as_ref(),
        ),
        (
            Language::Py,
            "py",
            "python",
            config.config.index.python.as_ref(),
        ),
    ] {
        let Some(setting) = setting.filter(|s| s.enabled) else {
            continue;
        };
        if !langs.is_empty() && !langs.iter().any(|s| s == short || s == name) {
            continue;
        }
        if !files.values().any(|f| f.lang == lang) {
            continue;
        }
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
                    setting
                        .project_name
                        .clone()
                        .context(".wtflow.yaml:1: index.python.project_name is required")?,
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
    anyhow::ensure!(!jobs.is_empty(),"no enabled indexers for languages present in this repository; configure .wtflow.yaml index");
    let mut meta = Metadata::load(root)?.unwrap_or_default();
    let mut selected = BTreeMap::new();
    for (path, file) in &files {
        if jobs.iter().any(|j| j.lang == file.lang) {
            selected.insert(path.clone(), hash(file.text.as_bytes()));
        }
    }
    if !force
        && selected.iter().all(|(p, h)| meta.files.get(p) == Some(h))
        && jobs.iter().all(|j| {
            root.join(format!(".wtflow/index/{}.scip", j.name))
                .is_file()
        })
    {
        eprintln!("index is up to date");
        return Ok(());
    }
    std::fs::create_dir_all(root.join(".wtflow/index"))?;
    for job in &jobs {
        let version = execute(root, job, &["--version".into()], true)?;
        execute(root, job, &job.args, false)?;
        anyhow::ensure!(
            root.join(format!(".wtflow/index/{}.scip", job.name))
                .metadata()
                .is_ok_and(|m| m.len() > 0),
            "{} did not produce its SCIP index",
            job.name
        );
        meta.indexers.insert(
            job.name.into(),
            version.lines().last().unwrap_or("unknown").into(),
        );
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
