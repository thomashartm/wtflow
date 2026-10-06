use clap::{Parser, Subcommand};
#[derive(Parser)]
#[command(name = "wtflow", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    DebugAst {
        file: std::path::PathBuf,
        #[arg(long)]
        range: Option<String>,
    },
    Version,
}
fn run() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Version => println!("wtflow {}", env!("CARGO_PKG_VERSION")),
        Command::DebugAst { file, range } => {
            let text = std::fs::read_to_string(&file)?;
            let source =
                wtflow_extract::source::SourceFile::parse(file.to_string_lossy().into(), text)?;
            if range.is_some() {
                anyhow::bail!("--range will be available with M4 CLI");
            }
            fn dump(
                n: tree_sitter::Node<'_>,
                f: &wtflow_extract::source::SourceFile,
                depth: usize,
            ) {
                println!(
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
            dump(source.tree.root_node(), &source, 0);
        }
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e:#}");
        std::process::exit(2);
    }
}
