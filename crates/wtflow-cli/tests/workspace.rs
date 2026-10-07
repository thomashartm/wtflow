use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wtflow"))
        .arg("--project")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}
fn ok(root: &Path, args: &[&str]) -> String {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
#[test]
fn configure_analyze_export_label_and_clear_preserve_project_contract() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("entry.ts"), "function run() { send(); }\n").unwrap();
    ok(root, &["init", "--lang", "ts", "--owner", "team"]);
    let original = fs::read_to_string(root.join(".wtflow/config.yaml")).unwrap();
    ok(
        root,
        &["config", "--key", "output.flows_dir", "--value", "saved"],
    );
    ok(
        root,
        &[
            "config",
            "--key",
            "output.export_dir",
            "--value",
            "docs/diagrams",
        ],
    );
    ok(
        root,
        &[
            "config",
            "--key",
            "output.formats",
            "--value",
            "[html, mmd]",
        ],
    );
    ok(
        root,
        &[
            "config",
            "--key",
            "analysis.resolver",
            "--value",
            "heuristic",
        ],
    );
    let config = fs::read(root.join(".wtflow/config.yaml")).unwrap();
    assert!(String::from_utf8_lossy(&config).contains(original.lines().next().unwrap()));
    assert!(!run(
        root,
        &["config", "--key", "output.theme", "--value", "invalid"]
    )
    .status
    .success());
    assert_eq!(config, fs::read(root.join(".wtflow/config.yaml")).unwrap());
    ok(root, &["analyze", "--entry", "entry.ts#run"]);
    let flow = fs::read_dir(root.join("saved"))
        .unwrap()
        .map(Result::unwrap)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "yaml"))
        .unwrap();
    let flow_arg = flow.to_str().unwrap();
    assert_eq!(
        ok(root, &["render", "--detail", flow_arg]),
        ok(root, &["render", flow_arg, "--detail"])
    );
    assert!(run(root, &["render", "--detail=false", flow_arg])
        .status
        .success());
    let before = wtflow_core::yaml::load(&fs::read_to_string(&flow).unwrap(), "test").unwrap();
    ok(
        root,
        &[
            "label-step",
            flow_arg,
            "--id",
            &before.steps[0].id,
            "--text",
            "Send message",
        ],
    );
    ok(root, &["analyze", "--entry", "entry.ts#run"]);
    let after = wtflow_core::yaml::load(&fs::read_to_string(&flow).unwrap(), "test").unwrap();
    assert_eq!(before.fingerprint, after.fingerprint);
    assert_eq!(after.steps[0].label.as_deref(), Some("Send message"));
    // Export uses saved analysis even after source is removed.
    fs::remove_file(root.join("entry.ts")).unwrap();
    ok(
        root,
        &[
            "export",
            flow_arg,
            "--formats",
            "mmd",
            "--output",
            "custom/out.mmd",
            "--direction",
            "LR",
            "--theme",
            "dark",
        ],
    );
    let diagram = fs::read_to_string(root.join("custom/out.mmd")).unwrap();
    assert!(diagram.contains("flowchart LR") && diagram.contains("dark"));
    assert!(!run(
        root,
        &["export", flow_arg, "--formats", "mmd", "--output", flow_arg]
    )
    .status
    .success());
    assert_eq!(
        after,
        wtflow_core::yaml::load(&fs::read_to_string(&flow).unwrap(), "test").unwrap()
    );
    ok(root, &["clear", "--yes"]);
    assert!(flow.exists());
    assert!(root.join("docs/diagrams").exists());
    assert!(root.join(".wtflow/config.yaml").exists());
}
#[test]
fn no_subcommand_in_a_pipe_shows_help_and_tui_requires_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    assert!(ok(dir.path(), &[]).contains("Usage:"));
    let out = run(dir.path(), &["tui"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("interactive terminal"));
}
