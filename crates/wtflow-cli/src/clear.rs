use anyhow::{Context, Result};
use std::{
    fs,
    io::{self, BufRead, Write},
    path::Path,
};
use wtflow_extract::config::RepositoryConfig;

pub fn run(dir: &Path) -> Result<()> {
    clear(dir, io::stdin().lock(), io::stdout().lock())
}

fn clear(dir: &Path, mut input: impl BufRead, mut output: impl Write) -> Result<()> {
    anyhow::ensure!(dir.is_dir(), "{}:1: not a directory", dir.display());
    let root = RepositoryConfig::discover_root(dir)?;
    let store = root.join(".wtflow");
    let metadata = match fs::symlink_metadata(&store) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            writeln!(output, "Nothing to clear in {}", store.display())?;
            return Ok(());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("{}:1: inspect directory", store.display()))
        }
    };
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "{}:1: refusing to clear a symlink or non-directory",
        store.display()
    );
    writeln!(output, "Clear {}", store.display())?;
    writeln!(output, "This removes saved flows (including their labels), diagrams, indexes, logs, and cached analysis.")?;
    let keep = loop {
        write!(output, "Keep config.yaml? [Y/n]: ")?;
        output.flush()?;
        let mut answer = String::new();
        if input.read_line(&mut answer)? == 0 {
            writeln!(output, "\nCancelled; nothing was cleared.")?;
            return Ok(());
        }
        match answer.trim().to_lowercase().as_str() {
            "" | "y" | "yes" => break true,
            "n" | "no" => break false,
            "q" | "quit" => {
                writeln!(output, "Cancelled; nothing was cleared.")?;
                return Ok(());
            }
            _ => writeln!(output, "Please answer yes or no, or q to cancel.")?,
        }
    };
    let mut paths = fs::read_dir(&store)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<io::Result<Vec<_>>>()?;
    paths.sort();
    for path in paths {
        if keep && path.file_name().is_some_and(|name| name == "config.yaml") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        // Do not follow links to anything outside the local store.
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        }
        .with_context(|| format!("{}:1: clear", path.display()))?;
    }
    writeln!(
        output,
        "Cleared {}{}",
        store.display(),
        if keep && store.join("config.yaml").exists() {
            "; kept config.yaml."
        } else {
            "."
        }
    )?;
    Ok(())
}

pub fn unattended(dir: &Path, keep: bool) -> Result<()> {
    crate::runtime::checkpoint()?;
    let mut output = Vec::new();
    clear(
        dir,
        std::io::Cursor::new(if keep { "y\n" } else { "n\n" }),
        &mut output,
    )?;
    crate::out_raw!("{}", String::from_utf8_lossy(&output));
    Ok(())
}
