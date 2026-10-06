use std::{
    path::Path,
    process::{Command, Output},
};
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wtflow"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}
fn ok(root: &Path, args: &[&str]) -> String {
    let out = run(root, args);
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".wtflow.yaml"), "collapse: false\n").unwrap();
    std::fs::write(
        dir.path().join("entry.ts"),
        "function run(x: number) {\n  send(x);\n  if (x > 10) { return; }\n}\n",
    )
    .unwrap();
    ok(
        dir.path(),
        &[
            "extract",
            "--entry",
            "entry.ts#run",
            "-o",
            "docs/run.flow.yaml",
        ],
    );
    dir
}
#[test]
fn labels_preserve_fingerprint_and_unknown_ids_are_atomic() {
    let dir = fixture();
    let path = dir.path().join("docs/run.flow.yaml");
    let before = std::fs::read_to_string(&path).unwrap();
    let f = wtflow_core::yaml::load(&before, "test").unwrap();
    std::fs::write(dir.path().join("labels.yaml"), "send_x: Send item\n").unwrap();
    ok(dir.path(), &["label", "docs/run.flow.yaml", "labels.yaml"]);
    let after = std::fs::read_to_string(&path).unwrap();
    let labeled = wtflow_core::yaml::load(&after, "test").unwrap();
    assert_eq!(f.fingerprint, labeled.fingerprint);
    assert_eq!(labeled.steps[0].label.as_deref(), Some("Send item"));
    std::fs::write(
        dir.path().join("labels.yaml"),
        "send_x: Overwrite\nnonexistent: Invalid\n",
    )
    .unwrap();
    assert_eq!(
        run(dir.path(), &["label", "docs/run.flow.yaml", "labels.yaml"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), after);
    std::fs::write(&path, after.replace("code: send(x)", "code: send(y)")).unwrap();
    let result = run(dir.path(), &["check", "docs/run.flow.yaml"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stdout).contains("E003"));
    assert_eq!(
        run(dir.path(), &["label", "docs/run.flow.yaml", "labels.yaml"])
            .status
            .code(),
        Some(2)
    );
}
#[test]
fn source_change_update_and_unchanged_label_retention() {
    let dir = fixture();
    std::fs::write(dir.path().join("labels.yaml"), "send_x: Send item\n").unwrap();
    ok(dir.path(), &["label", "docs/run.flow.yaml", "labels.yaml"]);
    ok(dir.path(), &["check", "--source", "docs/run.flow.yaml"]);
    let source = dir.path().join("entry.ts");
    std::fs::write(
        &source,
        std::fs::read_to_string(&source)
            .unwrap()
            .replace("x > 10", "x > 20"),
    )
    .unwrap();
    let result = run(dir.path(), &["check", "--source", "docs/run.flow.yaml"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stdout).contains("E005"));
    assert_eq!(
        ok(dir.path(), &["update", "docs/run.flow.yaml"]),
        "structure changed=true, labels kept=1\n"
    );
    ok(dir.path(), &["check", "--source", "docs/run.flow.yaml"]);
    let f = wtflow_core::yaml::load(
        &std::fs::read_to_string(dir.path().join("docs/run.flow.yaml")).unwrap(),
        "test",
    )
    .unwrap();
    assert_eq!(f.steps[0].label.as_deref(), Some("Send item"));
}
#[test]
fn schema_todo_render_and_usage() {
    let dir = fixture();
    let todos: serde_json::Value =
        serde_json::from_str(&ok(dir.path(), &["todo", "--json", "docs/run.flow.yaml"])).unwrap();
    assert_eq!(todos.as_array().unwrap().len(), 3);
    let schema: serde_json::Value =
        serde_json::from_str(&ok(dir.path(), &["schema", "--json", "--config"])).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    ok(
        dir.path(),
        &[
            "render",
            "--lang",
            "de",
            "-o",
            "out.md",
            "docs/run.flow.yaml",
        ],
    );
    assert!(std::fs::read_to_string(dir.path().join("out.md"))
        .unwrap()
        .starts_with("```mermaid\n"));
    assert!(ok(
        dir.path(),
        &["debug-ast", "entry.ts", "--range", "2:3-2:10"]
    )
    .contains("send"));
    assert_eq!(
        run(dir.path(), &["extract", "--entry", "bad"])
            .status
            .code(),
        Some(2)
    );
}
