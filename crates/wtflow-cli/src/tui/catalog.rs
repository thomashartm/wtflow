//! Forms are derived from the CLI contract, including validation and help.
use crate::{Cli, Command};
use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};

pub struct Field {
    pub id: String,
    pub long: Option<String>,
    pub value: String,
    pub help: String,
    pub toggle: bool,
    pub multiple: bool,
    pub required: bool,
    pub require_equals: bool,
}
pub struct Form {
    pub name: String,
    pub about: String,
    pub fields: Vec<Field>,
    pub selected: usize,
    pub offset: usize,
    pub error: String,
}
pub fn commands() -> Vec<(String, String)> {
    Cli::command()
        .get_subcommands()
        .filter(|c| c.get_name() != "tui")
        .map(|c| {
            (
                c.get_name().to_owned(),
                c.get_about().map(ToString::to_string).unwrap_or_default(),
            )
        })
        .collect()
}
impl Form {
    pub fn new(name: &str) -> Self {
        let mut cli = Cli::command();
        cli.build();
        let command = cli.find_subcommand(name).expect("catalog command");
        let fields = command
            .get_arguments()
            .filter(|a| {
                !matches!(
                    a.get_id().as_str(),
                    "help" | "version" | "no_progress" | "project"
                )
            })
            .map(|arg| {
                let toggle = matches!(
                    arg.get_action(),
                    clap::ArgAction::SetTrue | clap::ArgAction::SetFalse
                );
                let values = arg
                    .get_value_parser()
                    .possible_values()
                    .map(|vs| {
                        vs.map(|v| v.get_name().to_owned())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                Field {
                    id: arg.get_id().to_string(),
                    long: arg.get_long().map(str::to_owned),
                    value: if toggle {
                        "false".into()
                    } else {
                        arg.get_default_values()
                            .iter()
                            .map(|s| s.to_string_lossy())
                            .collect::<Vec<_>>()
                            .join(" ")
                    },
                    help: format!(
                        "{}{}",
                        arg.get_help().map(ToString::to_string).unwrap_or_default(),
                        if values.is_empty() {
                            String::new()
                        } else {
                            format!(" Choices: {values}")
                        }
                    ),
                    toggle,
                    multiple: arg.get_num_args().is_some_and(|n| n.max_values() > 1)
                        && arg.get_value_delimiter().is_none(),
                    required: arg.is_required_set(),
                    require_equals: arg.is_require_equals_set(),
                }
            })
            .collect();
        Self {
            name: name.into(),
            about: command
                .get_about()
                .map(ToString::to_string)
                .unwrap_or_default(),
            fields,
            selected: 0,
            offset: 0,
            error: String::new(),
        }
    }
    pub fn set(&mut self, id: &str, value: impl Into<String>) {
        if let Some(field) = self.fields.iter_mut().find(|f| f.id == id) {
            field.value = value.into();
        }
    }
    pub fn args(&self) -> Result<Vec<String>> {
        let mut args = vec![self.name.clone()];
        for field in &self.fields {
            if field.toggle {
                if field.value == "true" {
                    args.push(format!("--{}", field.long.as_deref().context("flag name")?));
                }
            } else if !field.value.is_empty() {
                if let Some(long) = &field.long {
                    if field.require_equals {
                        args.push(format!("--{long}={}", field.value));
                        continue;
                    }
                    args.push(format!("--{long}"));
                }
                if field.multiple {
                    args.extend(
                        shell_words::split(&field.value)
                            .context("quote paths containing spaces")?,
                    );
                } else {
                    args.push(field.value.clone());
                }
            }
        }
        Ok(args)
    }
    pub fn request(&self) -> Result<Command> {
        let args = self.args()?;
        let cli = Cli::try_parse_from(std::iter::once("wtflow".to_owned()).chain(args))?;
        let command = cli.command.context("missing operation")?;
        match &command {
            Command::Index { lang, .. } => anyhow::ensure!(
                !lang.is_empty(),
                "Select at least one language to index. Space or a click checks a language."
            ),
            Command::Init { lang, .. } => anyhow::ensure!(
                !lang.is_empty(),
                "Select languages for setup (ts, java, py)."
            ),
            Command::Clear { yes, .. } => anyhow::ensure!(
                *yes,
                "Enable yes to confirm deleting generated data. Exports outside .wtflow are kept."
            ),
            Command::Label { labels, .. } => anyhow::ensure!(
                labels != "-",
                "Choose a labels file, or use Label step in the flow explorer."
            ),
            _ => {}
        }
        Ok(command)
    }
    pub fn equivalent(&self, root: &std::path::Path) -> String {
        let mut args = vec![
            "wtflow".into(),
            "--project".into(),
            root.to_string_lossy().into_owned(),
        ];
        args.extend(self.args().unwrap_or_default());
        shell_words::join(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_cli_operation_and_option_has_a_form() {
        let mut cli = Cli::command();
        cli.build();
        for command in cli
            .get_subcommands()
            .filter(|c| c.get_name() != "tui" && c.get_name() != "help")
        {
            let form = Form::new(command.get_name());
            let expected: Vec<_> = command
                .get_arguments()
                .filter(|a| {
                    !matches!(
                        a.get_id().as_str(),
                        "help" | "version" | "no_progress" | "project"
                    )
                })
                .map(|a| a.get_id().as_str())
                .collect();
            assert_eq!(
                expected,
                form.fields
                    .iter()
                    .map(|f| f.id.as_str())
                    .collect::<Vec<_>>()
            );
        }
    }
    #[test]
    fn form_roundtrip_preserves_spaces_and_explicit_false() {
        let mut form = Form::new("analyze");
        form.set("entry", "src/my file.ts#run");
        form.set("detail", "false");
        form.set("formats", "html,md");
        match form.request().unwrap() {
            Command::Analyze { entry, output, .. } => {
                assert_eq!(entry, "src/my file.ts#run");
                assert_eq!(output.detail, Some(false));
                assert_eq!(output.formats.unwrap(), ["html", "md"]);
            }
            _ => panic!(),
        }
    }
}

#[cfg(test)]
mod parity {
    use super::*;
    use crate::runtime::{self, Event, Observer};
    use std::{
        collections::BTreeMap,
        fs,
        sync::{atomic::AtomicBool, mpsc, Arc},
    };
    fn execute(command: Command) -> (i32, Vec<String>) {
        let (events, receiver) = mpsc::channel();
        let observer = Observer {
            events,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let code = runtime::observe(observer, || crate::app::execute(command, false)).unwrap();
        let output = receiver
            .into_iter()
            .filter_map(|e| match e {
                Event::Output(s) | Event::Diagnostic(s) => Some(s),
                _ => None,
            })
            .collect();
        (code, output)
    }
    #[test]
    fn tui_and_cli_requests_produce_identical_artifacts_and_diagnostics() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        fs::create_dir(root.join(".wtflow")).unwrap();
        fs::write(root.join(".wtflow/config.yaml"), "collapse: false\nanalysis:\n  resolver: heuristic\noutput:\n  flows_dir: saved flows\n  export_dir: exported\n  formats: [html, md, mmd, lint]\n  theme: dark\n  direction: LR\n").unwrap();
        fs::write(
            root.join("entry.ts"),
            "function run(x: number) { if(x) { send(x); } else { return; } }\n",
        )
        .unwrap();
        let mut form = Form::new("analyze");
        form.set("entry", format!("{}#run", root.join("entry.ts").display()));
        form.set("detail", "true");
        let tui_result = execute(form.request().unwrap());
        fn artifacts(root: &std::path::Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
            walkdir::WalkDir::new(root)
                .into_iter()
                .map(Result::unwrap)
                .filter(|e| e.file_type().is_file())
                .map(|e| {
                    (
                        e.path().strip_prefix(root).unwrap().to_owned(),
                        fs::read(e.path()).unwrap(),
                    )
                })
                .collect()
        }
        let before = artifacts(root);
        let cli = Cli::try_parse_from(std::iter::once("wtflow".into()).chain(form.args().unwrap()))
            .unwrap();
        let cli_result = execute(cli.command.unwrap());
        assert_eq!(tui_result, cli_result);
        assert_eq!(before, artifacts(root));
        assert!(before
            .keys()
            .any(|p| p.starts_with("saved flows") && p.extension().is_some_and(|e| e == "yaml")));
        assert!(before
            .keys()
            .any(|p| p.starts_with("exported") && p.extension().is_some_and(|e| e == "html")));
        let mmd = before
            .iter()
            .find(|(p, _)| p.extension().is_some_and(|e| e == "mmd"))
            .unwrap()
            .1;
        assert!(String::from_utf8_lossy(mmd).contains("flowchart LR"));
    }
    #[test]
    fn cancellation_before_execution_does_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let (events, _receiver) = mpsc::channel();
        let result = runtime::observe(
            Observer {
                events,
                cancelled: Arc::new(AtomicBool::new(true)),
            },
            || {
                crate::app::execute(
                    Command::Init {
                        dir: dir.path().to_owned(),
                        lang: vec!["ts".into()],
                        owner: None,
                    },
                    false,
                )
            },
        );
        assert!(result.unwrap_err().to_string().contains("cancelled"));
        assert!(!dir.path().join(".wtflow").exists());
    }
}
