use std::path::PathBuf;
use wtflow_core::{
    lint::{self, Context},
    yaml, Kind,
};
use wtflow_extract::Cx;
fn root(lang: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata")
        .join(lang)
}
fn mask(text: &str) -> String {
    text.lines()
        .filter(|l| !l.starts_with("fingerprint:") && !l.starts_with("resolution:"))
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn authored_goldens_precede_adapters() {
    for (lang, file, symbol, name, golden) in [
        (
            "ts",
            "src/control.ts",
            "control",
            "control",
            "typescript-control",
        ),
        ("py", "control.py", "control", "control", "python-control"),
        (
            "java",
            "src/main/java/demo/Control.java",
            "Control.control",
            "control",
            "java-control",
        ),
        (
            "java",
            "src/main/java/demo/Routes.java",
            "Routes@events",
            "events",
            "java-camel",
        ),
    ] {
        let cx = Cx::load(&root(lang)).unwrap();
        let flow = cx.extract(file, symbol, Some(name), 0).unwrap();
        let text = yaml::emit(&flow).unwrap();
        let expected =
            std::fs::read_to_string(root("golden").join(format!("{golden}.flow.yaml"))).unwrap();
        assert_eq!(mask(&text), mask(&expected), "{lang}: {symbol}");
        assert_eq!(
            text,
            yaml::emit(&cx.extract(file, symbol, Some(name), 0).unwrap()).unwrap()
        );
    }
}
#[test]
fn required_language_features() {
    let cx = Cx::load(&root("ts")).unwrap();
    let flow = cx
        .extract(
            "src/reconciliation/service.ts",
            "ReconciliationService.reconcile",
            None,
            2,
        )
        .unwrap();
    let mut nodes = vec![];
    wtflow_core::visit(&flow.steps, &mut nodes);
    assert!(nodes
        .iter()
        .any(|n| n.kind == Kind::Group && n.target.as_deref() == Some("MatchingService.match")));
    assert!(nodes.iter().any(|n| n.kind == Kind::While));
    assert!(nodes
        .iter()
        .any(|n| n.tx.as_deref() == Some("db") && n.body.len() == 2));
    assert_eq!(flow.state.writes, vec!["match", "openItem"]);
    assert!(flow.boundaries.contains(&"matching".into()));
    let eps = cx.entrypoints();
    assert!(
        eps.iter()
            .any(|e| e.trigger == "event bank.statement.imported"),
        "{eps:?}"
    );
    assert!(
        eps.iter()
            .any(|e| e.trigger == "http POST /statements/reconcile"),
        "{eps:?}"
    );
    let cx = Cx::load(&root("py")).unwrap();
    let flow = cx.extract("app.py", "Documents.process", None, 2).unwrap();
    assert!(lint::check(&flow, &Context::default())
        .iter()
        .any(|d| d.rule == "W112"));
    assert_eq!(flow.steps[0].otherwise[0].kind, Kind::If);
    let cx = Cx::load(&root("java")).unwrap();
    let flow = cx
        .extract(
            "src/main/java/demo/Resource.java",
            "Resource.create",
            None,
            2,
        )
        .unwrap();
    assert!(lint::check(&flow, &Context::default())
        .iter()
        .any(|d| d.rule == "W110"));
    let flow = cx
        .extract("src/main/java/demo/Routes.java", "Routes@input", None, 3)
        .unwrap();
    let mut nodes = vec![];
    wtflow_core::visit(&flow.steps, &mut nodes);
    assert!(nodes
        .iter()
        .any(|n| n.kind == Kind::Emit && n.topic.as_deref() == Some("results")));
    assert!(nodes.iter().any(|n| n.kind == Kind::Try));
    assert!(nodes
        .iter()
        .any(|n| n.boundary.as_deref() == Some("example.com")));
}
#[test]
fn recursion_callbacks_parallel_and_multiple_handlers() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(".wtflow.yaml"), "collapse: false\n").unwrap();
    std::fs::write(
        tmp.path().join("sample.ts"),
        r#"
function rec() { rec(); }
function all(items: string[]) {
  items.forEach(item => { send(item); });
  Promise.all([one(), two()]);
  for (;;) { break; }
  do { work(); } while (retry());
  try { work(); } catch (error) { recover(); } finally { close(); }
}
"#,
    )
    .unwrap();
    let cx = Cx::load(tmp.path()).unwrap();
    let flow = cx.extract("sample.ts", "rec", None, 9).unwrap();
    assert_eq!(flow.steps[0].kind, Kind::Call);
    let flow = cx.extract("sample.ts", "all", None, 2).unwrap();
    assert_eq!(
        flow.steps.iter().map(|n| n.kind).collect::<Vec<_>>(),
        vec![
            Kind::ForEach,
            Kind::Parallel,
            Kind::While,
            Kind::While,
            Kind::Try
        ]
    );
    assert_eq!(flow.steps[1].body.len(), 2);
    assert_eq!(flow.steps[4].finally.len(), 1);
    std::fs::write(tmp.path().join("handlers.py"),"def work():\n    try:\n        run()\n    except ValueError:\n        recover()\n    except TypeError:\n        retry()\n    finally:\n        close()\n").unwrap();
    let cx = Cx::load(tmp.path()).unwrap();
    let flow = cx.extract("handlers.py", "work", None, 2).unwrap();
    assert_eq!(flow.steps[0].catch[0].kind, Kind::Switch);
    assert_eq!(flow.steps[0].catch[0].cases.len(), 2);
    assert_eq!(flow.steps[0].finally.len(), 1);
}
#[test]
fn bugs_fixture_triggers_w101_through_w106() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("bugs.ts"),
        std::fs::read(root("ts").join("src/bugs.ts")).unwrap(),
    )
    .unwrap();
    std::fs::write(
        tmp.path().join(".wtflow.yaml"),
        "rules:\n  - {match: '^repo\\.save', writes: item}\n",
    )
    .unwrap();
    let cx = Cx::load(tmp.path()).unwrap();
    let flow = cx.extract("bugs.ts", "bugs", None, 2).unwrap();
    let ds = lint::check(&flow, &Context::default());
    for code in ["W101", "W102", "W103", "W104", "W105", "W106"] {
        assert!(ds.iter().any(|d| d.rule == code), "{code}: {ds:?}");
    }
}
#[test]
fn controller_return_call_is_inlined_before_return() {
    let cx = Cx::load(&root("ts")).unwrap();
    let flow = cx
        .extract(
            "src/reconciliation/controller.ts",
            "ReconciliationController.imported",
            None,
            2,
        )
        .unwrap();
    assert_eq!(flow.steps[0].kind, Kind::Group);
    assert_eq!(
        flow.steps[0].target.as_deref(),
        Some("ReconciliationService.reconcile")
    );
    assert_eq!(flow.steps[1].kind, Kind::Return);
}
