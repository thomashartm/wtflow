use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};
const FLOW: &str = include_str!("../../../testdata/golden/core.flow.yaml");

fn browse(root: &Path, args: &[&str], selection: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wtflow"))
        .current_dir(root)
        .arg("flows")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    command
        .stdin
        .take()
        .unwrap()
        .write_all(selection.as_bytes())
        .unwrap();
    command.wait_with_output().unwrap()
}

#[test]
fn lists_saved_flows_in_order_and_renders_only_the_selected_document() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".wtflow.yaml"), "collapse: true\n").unwrap();
    fs::create_dir_all(root.path().join("docs/nested")).unwrap();
    fs::create_dir_all(root.path().join("docs/node_modules")).unwrap();
    for path in [
        "docs/nested/z.flow.yaml",
        "docs/a.flow.yaml",
        "docs/node_modules/hidden.flow.yaml",
    ] {
        fs::write(root.path().join(path), FLOW).unwrap();
    }
    fs::write(root.path().join("docs/bad.flow.yaml"), "invalid: flow\n").unwrap();
    let output = browse(
        root.path(),
        &["--dir", "docs", "--no-progress"],
        "0\n999\n2\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.find("docs/a.flow.yaml").unwrap() < stdout.find("docs/nested/z.flow.yaml").unwrap()
    );
    assert!(!stdout.contains("node_modules"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("skipped flow docs/bad.flow.yaml"));
    assert!(!root.path().join("docs/a.flow.md").exists());
    let diagram = fs::read_to_string(root.path().join("docs/nested/z.flow.md")).unwrap();
    let flow = wtflow_core::yaml::load(FLOW, "test").unwrap();
    let expected = wtflow_render::render(&flow, &wtflow_render::Options::default()).unwrap();
    assert_eq!(diagram, format!("```mermaid\n{expected}```\n"));
    assert_eq!(
        fs::read_to_string(root.path().join("docs/nested/z.flow.yaml")).unwrap(),
        FLOW
    );
}

#[test]
fn first_run_saves_analysis_and_later_runs_find_it_from_subdirectories() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".wtflow.yaml"), "collapse: true\n").unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/orders.ts"),
        "@Controller('orders')\nexport class OrdersController { @Post() create() { send(); } }\n",
    )
    .unwrap();
    fs::write(root.path().join("src/unsupported.ts"), "function {").unwrap();
    let created = browse(root.path(), &[], "1\n");
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    assert!(String::from_utf8_lossy(&created.stdout).contains("Saved flow: .wtflow/flows/"));
    let entries: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.path().join(".wtflow/entrypoints.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(entries[0]["file"], "src/orders.ts");
    let saved = fs::read_dir(root.path().join(".wtflow/flows"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "yaml"))
        .unwrap();
    assert!(saved.with_extension("md").is_file());
    assert!(fs::read_to_string(saved.with_extension("lint.txt"))
        .unwrap()
        .contains("src/unsupported.ts"));
    let before = fs::read_to_string(&saved).unwrap();
    let output = browse(&root.path().join("src"), &[], "1\n");
    assert!(output.status.success());
    assert_eq!(fs::read_to_string(&saved).unwrap(), before);
    let mut flow = wtflow_core::yaml::load(&before, "test").unwrap();
    let id = flow.steps[0].id.clone();
    wtflow_core::labels::apply(
        &mut flow,
        &std::collections::BTreeMap::from([(id, "Send order".into())]),
    )
    .unwrap();
    fs::write(&saved, wtflow_core::yaml::emit(&flow).unwrap()).unwrap();
    let refreshed = browse(root.path(), &[], "n\n1\n");
    assert!(
        refreshed.status.success(),
        "{}",
        String::from_utf8_lossy(&refreshed.stderr)
    );
    let updated = wtflow_core::yaml::load(&fs::read_to_string(&saved).unwrap(), "test").unwrap();
    assert_eq!(updated.steps[0].label.as_deref(), Some("Send order"));
    assert_eq!(updated.fingerprint, flow.fingerprint);
}

#[test]
fn quitting_and_empty_directories_do_not_create_diagrams() {
    let root = tempfile::tempdir().unwrap();
    let empty = browse(root.path(), &[], "");
    assert!(empty.status.success());
    assert!(String::from_utf8_lossy(&empty.stdout).contains("No recognized entrypoints"));
    fs::write(root.path().join("one.flow.yaml"), FLOW).unwrap();
    for selection in ["q\n", ""] {
        assert!(browse(root.path(), &[], selection).status.success());
        assert!(!root.path().join("one.flow.md").exists());
    }
}
