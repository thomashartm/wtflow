use std::fs;
use wtflow_extract::Cx;

#[test]
fn discovery_defers_resolution_and_reuses_parsed_sources_when_selected() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".wtflow.yaml"), "collapse: false\n").unwrap();
    fs::write(root.path().join("app.ts"), "@Controller('orders') class App { @Post() run() { this.save(); } save() { publish('saved'); } }").unwrap();
    fs::create_dir_all(root.path().join(".wtflow/index")).unwrap();
    // Discovery must not depend on index metadata or prepare call resolution.
    fs::write(root.path().join(".wtflow/index/meta.yaml"), "[invalid").unwrap();
    let (discovery, warnings) = Cx::load_entrypoints(root.path()).unwrap();
    assert!(warnings.is_empty());
    let entries = discovery.entrypoints();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].symbol, "App.run");
    fs::remove_file(root.path().join(".wtflow/index/meta.yaml")).unwrap();
    let expected = Cx::load(root.path())
        .unwrap()
        .extract("app.ts", "App.run", None, 3)
        .unwrap();
    // Selection consumes the parsed inventory; it does not parse the files again.
    fs::write(root.path().join("app.ts"), "invalid {").unwrap();
    let cx = discovery.into_context().unwrap();
    assert_eq!(cx.entrypoints(), entries);
    let actual = cx.extract("app.ts", "App.run", None, 3).unwrap();
    assert_eq!(actual.fingerprint, expected.fingerprint);
    assert_eq!(actual.steps[0].kind, wtflow_core::Kind::Group);
}

#[test]
fn indexed_heuristic_candidates_preserve_ambiguity_and_import_boundaries() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".wtflow.yaml"), "collapse: false\n").unwrap();
    fs::write(root.path().join("app.ts"), "import { Worker } from './worker'; class App { constructor(private worker: Worker) {} run() { this.worker.save(); this.local(); unknown(); } local() { finish(); } }").unwrap();
    fs::write(
        root.path().join("worker.ts"),
        "export class Worker { save() { persist(); } }",
    )
    .unwrap();
    fs::write(
        root.path().join("other.ts"),
        "class Worker { save() { wrong(); } } function unknown() { wrong(); }",
    )
    .unwrap();
    let cx = Cx::load(root.path()).unwrap();
    let flow = cx.extract("app.ts", "App.run", None, 2).unwrap();
    assert_eq!(flow.steps[0].target.as_deref(), Some("Worker.save"));
    assert!(flow.steps[0].body[0].src.starts_with("worker.ts:"));
    assert_eq!(flow.steps[1].target.as_deref(), Some("App.local"));
    assert_eq!(flow.steps[2].kind, wtflow_core::Kind::Do);
    fs::write(
        root.path().join("worker.ts"),
        "export class Worker { save() { persist(); } save() { other(); } }",
    )
    .unwrap();
    let cx = Cx::load(root.path()).unwrap();
    let flow = cx.extract("app.ts", "App.run", None, 2).unwrap();
    assert_eq!(flow.steps[0].kind, wtflow_core::Kind::Do);
}

#[test]
fn scans_stay_inside_the_project_but_nested_worktrees_can_be_selected_explicitly() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".wtflow.yaml"), "collapse: false\n").unwrap();
    let source =
        "@Controller('orders') class App { @Post() run() { this.save(); } save() { send(); } }";
    fs::write(root.path().join("app.ts"), source).unwrap();
    let worktree = root.path().join(".claude/worktrees/agent-copy");
    let checkout = root.path().join("other-checkout");
    for dir in [&worktree, &checkout] {
        fs::create_dir_all(dir.join(".wtflow/index")).unwrap();
        fs::write(dir.join(".wtflow.yaml"), "collapse: false\n").unwrap();
        fs::write(dir.join("app.ts"), source).unwrap();
        // Accidentally loading a different project's index must fail this test.
        fs::write(dir.join(".wtflow/index/typescript.scip"), b"invalid index").unwrap();
    }
    fs::write(
        worktree.join(".git"),
        "gitdir: /elsewhere/worktrees/agent-copy",
    )
    .unwrap();
    fs::create_dir(checkout.join(".git")).unwrap();
    let (discovery, warnings) = Cx::load_entrypoints(root.path()).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(discovery.entrypoints().len(), 1);
    let mut cx = discovery.into_context().unwrap();
    assert_eq!(cx.files.len(), 1);
    cx.enable_scip(false).unwrap();
    assert!(cx.chain.scip.is_none());
    assert_eq!(Cx::load(root.path()).unwrap().files.len(), 1);
    assert_eq!(wtflow_extract::source::paths(root.path()).unwrap().len(), 1);
    let (nested, _) = Cx::load_entrypoints(&worktree).unwrap();
    assert_eq!(nested.entrypoints().len(), 1);
    assert_eq!(nested.entrypoints()[0].file, "app.ts");
    assert_eq!(nested.config.root, worktree.canonicalize().unwrap());
}
