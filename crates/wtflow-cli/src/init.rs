use anyhow::{Context, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{self, BufRead, Write},
    path::Path,
};
use wtflow_extract::config::{RepositoryConfig, CONFIG_PATH, LEGACY_CONFIG_PATH};

struct Questions<R, W> {
    input: R,
    output: W,
}

impl<R: BufRead, W: Write> Questions<R, W> {
    fn ask(&mut self, question: &str, default: &str) -> Result<String> {
        loop {
            write!(self.output, "{question}")?;
            if !default.is_empty() {
                write!(self.output, " [{default}]")?;
            }
            write!(self.output, ": ")?;
            self.output.flush()?;
            let mut answer = String::new();
            anyhow::ensure!(
                self.input.read_line(&mut answer)? != 0,
                "input ended; initialization cancelled without writing a configuration"
            );
            let answer = answer.trim();
            if !answer.is_empty() {
                return Ok(answer.to_owned());
            }
            if !default.is_empty() {
                return Ok(default.to_owned());
            }
            writeln!(self.output, "Please enter a value.")?;
        }
    }

    fn yes_no(&mut self, question: &str) -> Result<bool> {
        loop {
            match self.ask(question, "no")?.to_lowercase().as_str() {
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => writeln!(self.output, "Please answer yes or no.")?,
            }
        }
    }
}

pub fn run(dir: &Path) -> Result<()> {
    configure(dir, io::stdin().lock(), io::stdout().lock())
        .with_context(|| format!("{}:1: initialize project", dir.join(CONFIG_PATH).display()))
}

fn configure(dir: &Path, input: impl BufRead, mut output: impl Write) -> Result<()> {
    let root = std::env::current_dir()?.join(dir);
    anyhow::ensure!(
        !root.exists() || root.is_dir(),
        "{} is not a directory",
        dir.display()
    );
    let path = root.join(CONFIG_PATH);
    match fs::symlink_metadata(&path) {
        Ok(_) => anyhow::bail!("configuration already exists; edit it instead of running init"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let legacy = root.join(LEGACY_CONFIG_PATH);
    match fs::symlink_metadata(&legacy) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file(),
                "{}: existing config is not a regular file; move it manually",
                legacy.display()
            );
            RepositoryConfig::load(&root)?;
            fs::create_dir_all(root.join(".wtflow"))?;
            // Linking then unlinking preserves bytes and permissions and refuses
            // to overwrite a config created concurrently. The old file remains
            // intact if creating the destination fails.
            fs::hard_link(&legacy, &path).with_context(|| {
                format!("{}: migrate config without overwriting", path.display())
            })?;
            fs::remove_file(&legacy).with_context(|| {
                format!(
                    "{}: config copied to {}; remove legacy file",
                    legacy.display(),
                    path.display()
                )
            })?;
            writeln!(
                output,
                "Moved {} to {}",
                dir.join(LEGACY_CONFIG_PATH).display(),
                dir.join(CONFIG_PATH).display()
            )?;
            return Ok(());
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let project = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("project");
    let mut questions = Questions { input, output };
    writeln!(questions.output, "Set up wtflow in {}", dir.display())?;
    writeln!(
        questions.output,
        "Press Enter to accept a suggested value. No indexers will run during setup."
    )?;

    let mut detected = Vec::new();
    for (language, markers) in [
        ("ts", &["tsconfig.json", "package.json"][..]),
        ("java", &["pom.xml", "build.gradle", "build.gradle.kts"][..]),
        (
            "py",
            &[
                "pyproject.toml",
                "requirements.txt",
                "setup.py",
                "setup.cfg",
            ][..],
        ),
    ] {
        if markers.iter().any(|marker| root.join(marker).is_file()) {
            detected.push(language);
        }
    }
    let languages = loop {
        let answer = questions.ask(
            "Languages to follow (ts, java, py; separate multiple choices with commas)",
            &detected.join(","),
        )?;
        let mut chosen = BTreeSet::new();
        let mut valid = true;
        for language in answer.split(',') {
            match language.trim().to_lowercase().as_str() {
                "ts" | "typescript" => {
                    chosen.insert("typescript");
                }
                "java" => {
                    chosen.insert("java");
                }
                "py" | "python" => {
                    chosen.insert("python");
                }
                _ => valid = false,
            }
        }
        if valid && !chosen.is_empty() {
            break chosen;
        }
        writeln!(
            questions.output,
            "Choose ts, java, or py, separated by commas."
        )?;
    };
    let owner = questions.ask("Owner or service name for this project", project)?;
    let python_name = if languages.contains("python") {
        Some(questions.ask("Python project name for the index", project)?)
    } else {
        None
    };
    let mut modules = BTreeMap::from([(".".to_owned(), owner)]);
    writeln!(
        questions.output,
        "Folder owners help identify calls between services or teams."
    )?;
    while questions.yes_no("Assign a different owner to a folder?")? {
        let module = loop {
            let candidate = questions.ask(
                "Folder path relative to the project (for example src/payments)",
                "",
            )?;
            let value = serde_json::json!({"modules": [{"path": candidate, "owner": "owner"}]});
            if wtflow_core::schema::validate(&value, true).is_err() {
                writeln!(
                    questions.output,
                    "Use a relative path with forward slashes and no '..' segments."
                )?;
                continue;
            }
            let normalized = candidate
                .split('/')
                .filter(|part| !part.is_empty() && *part != ".")
                .collect::<Vec<_>>()
                .join("/");
            let candidate = if normalized.is_empty() {
                ".".to_owned()
            } else {
                normalized
            };
            if modules.contains_key(&candidate) {
                writeln!(
                    questions.output,
                    "That path already has an owner; choose another folder."
                )?;
            } else {
                break candidate;
            }
        };
        let owner = questions.ask("Owner or service name for this folder", "")?;
        modules.insert(module, owner);
    }

    let mut text = String::from(
        "# Project settings for wtflow. Add custom call rules here as needed.\nmodules:\n",
    );
    for (module, owner) in modules {
        text.push_str(&format!(
            "  - path: {}\n    owner: {}\n",
            serde_json::to_string(&module)?,
            serde_json::to_string(&owner)?
        ));
    }
    text.push_str("collapse: true\nindex:\n");
    for language in ["typescript", "java", "python"] {
        if !languages.contains(language) {
            continue;
        }
        text.push_str(&format!("  {language}:\n    enabled: true\n"));
        if language == "typescript" {
            text.push_str("    args:\n      - \"--infer-tsconfig\"\n");
        }
        if language == "python" {
            if let Some(name) = &python_name {
                text.push_str(&format!(
                    "    project_name: {}\n",
                    serde_json::to_string(name)?
                ));
            }
        }
    }
    let value = serde_yaml_ng::from_str(&text)?;
    wtflow_core::schema::validate(&value, true)?;
    let _: wtflow_extract::config::Config = serde_json::from_value(value)?;
    fs::create_dir_all(root.join(".wtflow"))?;
    // create_new also protects configurations created while the questions were open.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .context("create .wtflow/config.yaml (existing files are never overwritten)")?;
    if let Err(error) = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(error.into());
    }
    writeln!(
        questions.output,
        "Created {}",
        dir.join(CONFIG_PATH).display()
    )?;
    writeln!(questions.output, "Next, install this project's dependencies, then run `wtflow index` and `wtflow entrypoints .` from that directory.")?;
    Ok(())
}

/// Reuses the guided initializer and its validation without terminal input.
pub fn unattended(dir: &Path, languages: &[String], owner: Option<&str>) -> Result<()> {
    let project = dir
        .canonicalize()?
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let owner = owner.unwrap_or(&project);
    anyhow::ensure!(!owner.contains(['\n', '\r']), "owner must be one line");
    let python = languages.iter().any(|v| v == "py" || v == "python");
    for lang in languages {
        anyhow::ensure!(
            matches!(
                lang.as_str(),
                "ts" | "typescript" | "java" | "py" | "python"
            ),
            "unsupported language: {lang}"
        );
    }
    let input = format!(
        "{}\n{owner}\n{}no\n",
        languages.join(","),
        if python {
            format!("{project}\n")
        } else {
            String::new()
        }
    );
    let mut output = Vec::new();
    configure(dir, std::io::Cursor::new(input), &mut output)?;
    crate::out_raw!("{}", String::from_utf8_lossy(&output));
    Ok(())
}
