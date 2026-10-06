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
fn entrypoints_respect_requested_directory_and_keep_repository_paths() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".wtflow.yaml"), "collapse: true\n").unwrap();
    for folder in ["src/nested", "src-other", "unrelated"] {
        std::fs::create_dir_all(dir.path().join(folder)).unwrap();
    }
    let controller =
        "@Controller('orders')\nexport class OrdersController { @Post() create() {} }\n";
    for file in [
        "src/orders.ts",
        "src/nested/orders.ts",
        "src-other/orders.ts",
    ] {
        std::fs::write(dir.path().join(file), controller).unwrap();
    }
    std::fs::write(dir.path().join("unrelated/broken.ts"), "function {").unwrap();
    let output = ok(dir.path(), &["entrypoints", "--json", "src/"]);
    let entries: serde_json::Value = serde_json::from_str(&output).unwrap();
    let files: Vec<_> = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["file"].as_str().unwrap())
        .collect();
    assert_eq!(files, ["src/nested/orders.ts", "src/orders.ts"]);
    let cached: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join(".wtflow/entrypoints.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(cached, entries);
    assert_eq!(output, ok(dir.path(), &["entrypoints", "--json", "src/"]));
    assert_eq!(
        output,
        ok(
            dir.path(),
            &["entrypoints", "--no-progress", "--json", "src/"]
        )
    );
    assert!(run(dir.path(), &["entrypoints", "--json", "src/"])
        .stderr
        .is_empty());
    // Discovery reports skipped files without losing entries in valid files.
    let all = run(dir.path(), &["entrypoints", "--json", "."]);
    assert!(all.status.success());
    assert!(String::from_utf8_lossy(&all.stderr).contains("unrelated/broken.ts"));
    assert!(
        String::from_utf8_lossy(&all.stderr).contains("entrypoints from this file were skipped")
    );
    let entries: serde_json::Value = serde_json::from_slice(&all.stdout).unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 3);
    // Extraction remains strict about syntax errors, so flows are not silently incomplete.
    assert!(wtflow_extract::Cx::load(&dir.path().join("src/orders.ts")).is_err());
}

#[test]
fn source_scans_skip_build_output_and_typescript_declarations() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".wtflow.yaml"), "collapse: true\n").unwrap();
    for folder in ["src/types", "dist/nested", "build"] {
        std::fs::create_dir_all(dir.path().join(folder)).unwrap();
    }
    std::fs::write(
        dir.path().join("src/orders.ts"),
        "@Controller('orders')\nexport class OrdersController { @Post() create() { send(); } }\n",
    )
    .unwrap();
    // Generated NestJS declarations can contain syntax unsupported by the grammar.
    let declaration = "declare const Base: import(\"@nestjs/common\").Type<Partial<Input>>;\n";
    std::fs::write(dir.path().join("src/types/orders.d.ts"), declaration).unwrap();
    std::fs::write(dir.path().join("dist/nested/orders.d.ts"), declaration).unwrap();
    for file in ["dist/broken.ts", "build/broken.ts"] {
        std::fs::write(dir.path().join(file), "function {").unwrap();
    }
    let entries: serde_json::Value =
        serde_json::from_str(&ok(dir.path(), &["entrypoints", "--json", "."])).unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 1);
    assert_eq!(entries[0]["file"], "src/orders.ts");
    let cx = wtflow_extract::Cx::load(&dir.path().join("src/orders.ts")).unwrap();
    assert_eq!(
        cx.files.keys().map(String::as_str).collect::<Vec<_>>(),
        ["src/orders.ts"]
    );
    let output = ok(
        dir.path(),
        &[
            "extract",
            "--entry",
            "src/orders.ts#OrdersController.create",
            "--resolver",
            "heuristic",
        ],
    );
    let flow = wtflow_core::yaml::load(&output, "test").unwrap();
    assert_eq!(flow.entry.file, "src/orders.ts");
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
#[test]
fn committed_index_modified_source_warns_and_falls_back() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ts");
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::create_dir_all(dir.path().join(".wtflow/index")).unwrap();
    for file in [
        "src/control.ts",
        ".wtflow/index/typescript.scip",
        ".wtflow/index/meta.yaml",
    ] {
        std::fs::copy(fixture.join(file), dir.path().join(file)).unwrap();
    }
    std::fs::write(dir.path().join(".wtflow.yaml"), "collapse: true\n").unwrap();
    let initial = run(
        dir.path(),
        &[
            "extract",
            "--entry",
            "src/control.ts#control",
            "-o",
            "control.flow.yaml",
        ],
    );
    assert!(initial.status.success());
    assert!(!String::from_utf8_lossy(&initial.stderr).contains("W120"));
    let source = dir.path().join("src/control.ts");
    std::fs::write(
        &source,
        std::fs::read_to_string(&source)
            .unwrap()
            .replace("skip", "ignore"),
    )
    .unwrap();
    let changed = run(
        dir.path(),
        &[
            "extract",
            "--entry",
            "src/control.ts#control",
            "-o",
            "control.flow.yaml",
        ],
    );
    assert!(changed.status.success());
    assert!(
        String::from_utf8_lossy(&changed.stderr).contains("W120 - stale index for src/control.ts")
    );
    let flow = wtflow_core::yaml::load(
        &std::fs::read_to_string(dir.path().join("control.flow.yaml")).unwrap(),
        "test",
    )
    .unwrap();
    assert_eq!(flow.resolution, wtflow_core::ResolutionMode::Heuristic);
    let checked = run(dir.path(), &["check", "--source", "control.flow.yaml"]);
    assert_eq!(checked.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&checked.stdout).contains("error W120"));
}
#[test]
fn debug_resolve_uses_committed_indexes_for_all_languages() {
    let fixtures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    for (lang, position, prefix) in [
        ("ts", "src/control.ts:4:5", "scip-typescript"),
        ("py", "control.py:5:9", "scip-python"),
        ("java", "src/main/java/demo/Control.java:6:7", "scip-java"),
    ] {
        let out = ok(&fixtures.join(lang), &["debug-resolve", position]);
        let resolution: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(
            resolution["Def"]["symbol"]
                .as_str()
                .unwrap()
                .starts_with(prefix),
            "{out}"
        );
    }
}
#[test]
fn context_cli_is_offline_and_does_not_modify_flow() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("glossary.yaml"),
        "version: 'v1'\nterms: {item: invoice}\n",
    )
    .unwrap();
    let path = dir.path().join("docs/run.flow.yaml");
    let before = std::fs::read(&path).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_wtflow"))
        .current_dir(dir.path())
        .env("PATH", "")
        .args(["todo", "--json", "--context", "docs/run.flow.yaml"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let packets: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(packets[0]["glossary"]["version"], "v1");
    assert!(packets[0]["callee"].is_null());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        run(dir.path(), &["todo", "--context", "docs/run.flow.yaml"])
            .status
            .code(),
        Some(2)
    );
}
