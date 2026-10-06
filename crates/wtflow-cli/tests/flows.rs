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

#[test]
fn searched_entry_is_saved_and_tests_are_available_without_flooding_the_picker() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".wtflow.yaml"), "collapse: true\n").unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    for n in 1..=12 {
        fs::write(
            root.path().join(format!("src/order{n:02}.ts")),
            format!(
                "@Controller('orders') class Order{n} {{ @Post('{n}') create() {{ send(); }} }}"
            ),
        )
        .unwrap();
    }
    fs::write(
        root.path().join("src/z.spec.ts"),
        "@Controller('fixture') class Fixture { @Get() run() { test(); } }",
    )
    .unwrap();
    let listing = browse(root.path(), &[], "q\n");
    let text = String::from_utf8(listing.stdout).unwrap();
    assert!(text.contains("12 matches | page 1/2"));
    assert!(text.contains("Tests: hidden (1)"));
    assert!(!text.contains("http POST /orders/9"));
    assert!(!text.contains("http GET /fixture"));
    let picked = browse(root.path(), &[], "/order12.ts\n12\n");
    assert!(
        picked.status.success(),
        "{}",
        String::from_utf8_lossy(&picked.stderr)
    );
    let saved = fs::read_dir(root.path().join(".wtflow/flows"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "yaml"))
        .unwrap();
    let flow = wtflow_core::yaml::load(&fs::read_to_string(saved).unwrap(), "test").unwrap();
    assert_eq!(flow.entry.file, "src/order12.ts");
    let test_pick = browse(root.path(), &[], "n\nt\n/fixture\n13\n");
    assert!(
        test_pick.status.success(),
        "{}",
        String::from_utf8_lossy(&test_pick.stderr)
    );
    let text = String::from_utf8(test_pick.stdout).unwrap();
    assert!(text.contains("http GET /fixture [test]"));
    assert!(text.contains("Saved flow:"));
}

#[test]
fn choosing_a_flow_follows_past_two_calls_and_refreshes_old_summaries() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".wtflow.yaml"), "collapse: false\n").unwrap();
    fs::write(root.path().join("app.ts"), "@Controller('orders') class App { @Post() run() { return this.a(); } a() { return this.b(); } b() { return this.c(); } c() { publish('complete'); return result; } }").unwrap();
    let cx = wtflow_extract::Cx::load(root.path()).unwrap();
    let mut old = cx.extract("app.ts", "App.run", Some("orders"), 2).unwrap();
    let id = old.steps[0].id.clone();
    wtflow_core::labels::apply(
        &mut old,
        &std::collections::BTreeMap::from([(id, "Process the order".into())]),
    )
    .unwrap();
    let path = root.path().join("orders.flow.yaml");
    fs::write(&path, wtflow_core::yaml::emit(&old).unwrap()).unwrap();
    let output = browse(root.path(), &["--no-open"], "1\n");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let flow = wtflow_core::yaml::load(&fs::read_to_string(&path).unwrap(), "test").unwrap();
    assert_eq!(flow.entry.depth, wtflow_extract::DEFAULT_DEPTH);
    assert_eq!(flow.flow, "orders");
    assert_eq!(flow.steps[0].label.as_deref(), Some("Process the order"));
    let mut nodes = vec![];
    wtflow_core::visit(&flow.steps, &mut nodes);
    assert!(nodes.iter().any(|n| n.code == "publish('complete')"));
    let html = fs::read_to_string(path.with_extension("html")).unwrap();
    assert!(html.contains("Expand all"));
    assert!(html.contains("publish(&#39;complete&#39;)"));
    assert!(html.contains("app.ts:1"));
    assert!(!html.contains("<script src="));
    assert!(path.with_extension("md").exists());
    let output = Command::new(env!("CARGO_BIN_EXE_wtflow"))
        .current_dir(root.path())
        .args(["extract", "--entry", "app.ts#App.run"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let extracted =
        wtflow_core::yaml::load(&String::from_utf8(output.stdout).unwrap(), "test").unwrap();
    assert_eq!(flow.fingerprint, extracted.fingerprint);
}

#[test]
fn default_discovery_uses_project_stores_without_exposing_golden_fixtures() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".wtflow.yaml"), "collapse: true\n").unwrap();
    for dir in [
        ".wtflow/flows",
        "docs/flows",
        "testdata/golden",
        "fixtures",
        "other-project/.wtflow/flows",
    ] {
        fs::create_dir_all(root.path().join(dir)).unwrap();
    }
    fs::write(
        root.path().join(".wtflow/flows/real.flow.yaml"),
        FLOW.replace("flow: sample", "flow: ActualApplication"),
    )
    .unwrap();
    fs::write(
        root.path().join("docs/flows/documented.flow.yaml"),
        FLOW.replace("flow: sample", "flow: DocumentedFlow"),
    )
    .unwrap();
    for path in [
        "testdata/golden/core.flow.yaml",
        "fixtures/fake.flow.yaml",
        "other-project/.wtflow/flows/unrelated.flow.yaml",
    ] {
        fs::write(
            root.path().join(path),
            FLOW.replace("flow: sample", "flow: ShouldNotAppear"),
        )
        .unwrap();
    }
    let output = browse(root.path(), &[], "q\n");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Project: "));
    assert!(text.contains("ActualApplication"));
    assert!(text.contains("DocumentedFlow"));
    assert!(!text.contains("ShouldNotAppear"));
    assert!(!text.contains("testdata/golden"));
    // Deliberately browsing a fixture directory is still possible.
    let output = browse(root.path(), &["--dir", "testdata/golden"], "q\n");
    assert!(String::from_utf8_lossy(&output.stdout).contains("ShouldNotAppear"));
}

#[test]
fn fixture_source_entrypoints_are_hidden_in_application_picker() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".wtflow.yaml"), "collapse: true\n").unwrap();
    fs::create_dir_all(root.path().join("testdata/demo")).unwrap();
    fs::write(
        root.path().join("testdata/demo/controller.ts"),
        "@Controller('fixture') class Fixture { @Get() run() { test(); } }",
    )
    .unwrap();
    let output = browse(root.path(), &[], "q\n");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Tests: hidden (1)"));
    assert!(!text.contains("http GET /fixture"));
}

#[test]
fn saved_analysis_does_not_hide_other_entrypoints() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".wtflow.yaml"), "collapse: false\n").unwrap();
    fs::write(root.path().join("app.ts"), "@Controller('orders') class App { @Post() create() { this.save(); } @Get() list() { this.read(); }\n/** Save an order. */\nsave(): boolean { return true; } read(): string { return 'order'; } }").unwrap();
    assert!(browse(root.path(), &["--no-open"], "1\n").status.success());
    let listing = browse(root.path(), &[], "q\n");
    let text = String::from_utf8(listing.stdout).unwrap();
    assert!(text.contains("All flows | 2 matches"), "{text}");
    assert!(text.contains("http POST /orders [saved]"));
    assert!(text.contains("http GET /orders"));
    assert!(browse(root.path(), &["--no-open"], "2\n").status.success());
    let files: Vec<_> = fs::read_dir(root.path().join(".wtflow/flows"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(
        files
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e == "yaml"))
            .count(),
        2
    );
    assert_eq!(
        files
            .iter()
            .filter(|p| p.to_string_lossy().ends_with(".context.json"))
            .count(),
        2
    );
    assert!(files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "html"))
        .any(|p| fs::read_to_string(p).unwrap().contains("Save an order.")));
}

#[test]
fn filter_flag_supports_patterns_and_can_be_cleared_in_the_picker() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".wtflow.yaml"), "collapse: false\n").unwrap();
    fs::write(root.path().join("app.ts"), "@Controller('orders') class App { @Post() create() { send(); } @Get() list() { read(); } }").unwrap();
    let output = browse(root.path(), &["--filter", "APP.CRE*"], "q\n");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("1 matches"));
    assert!(text.contains("http POST /orders"));
    assert!(!text.contains("http GET /orders"));
    let output = browse(root.path(), &["--filter", "not-found"], "/\n2\n");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("No matches"));
    assert!(text.contains("Saved flow:"));
    let output = Command::new(env!("CARGO_BIN_EXE_wtflow"))
        .current_dir(root.path())
        .args(["entrypoints", "--json", "--filter", "GET l?st"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let entries: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 1);
    assert_eq!(entries[0]["symbol"], "App.list");
    let inventory: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.path().join(".wtflow/entrypoints.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(inventory.as_array().unwrap().len(), 2);
}
