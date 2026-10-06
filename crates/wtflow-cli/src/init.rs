use anyhow::{Context, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{self, BufRead, Write},
    path::Path,
};

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
    configure(dir, io::stdin().lock(), io::stdout().lock()).with_context(|| {
        format!(
            "{}:1: initialize project",
            dir.join(".wtflow.yaml").display()
        )
    })
}

fn configure(dir: &Path, input: impl BufRead, output: impl Write) -> Result<()> {
    let root = std::env::current_dir()?.join(dir);
    anyhow::ensure!(
        !root.exists() || root.is_dir(),
        "{} is not a directory",
        dir.display()
    );
    let path = root.join(".wtflow.yaml");
    match fs::symlink_metadata(&path) {
        Ok(_) => anyhow::bail!("configuration already exists; edit it instead of running init"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
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
    fs::create_dir_all(&root)?;
    // create_new also protects configurations created while the questions were open.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .context("create .wtflow.yaml (existing files are never overwritten)")?;
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
        dir.join(".wtflow.yaml").display()
    )?;
    writeln!(questions.output, "Next, install this project's dependencies, then run `wtflow index` and `wtflow entrypoints .` from that directory.")?;
    Ok(())
}
