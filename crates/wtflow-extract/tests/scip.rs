use std::path::PathBuf;
use wtflow_core::{
    lint::{self, Context},
    yaml, Kind, ResolutionMode,
};
use wtflow_extract::Cx;
fn root(lang: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata")
        .join(lang)
}
#[test]
fn real_ts_barrel_alias_dispatch_and_documentation() {
    let mut cx = Cx::load(&root("ts")).unwrap();
    let heuristic = cx
        .extract("src/dispatch.ts", "Dispatch.run", None, 2)
        .unwrap();
    assert_eq!(heuristic.steps[0].kind, Kind::Do);
    cx.enable_scip(true).unwrap();
    let flow = cx
        .extract("src/dispatch.ts", "Dispatch.run", None, 2)
        .unwrap();
    assert_eq!(flow.resolution, ResolutionMode::Scip);
    assert_eq!(flow.steps[0].target.as_deref(), Some("normalize"));
    let dispatch = &flow.steps[1];
    assert_eq!(dispatch.kind, Kind::Switch);
    assert_eq!(dispatch.cases.len(), 2);
    assert!(dispatch.cases.iter().all(|c| c.steps[0].symbol.is_some()));
    assert!(lint::check(&flow, &Context::default())
        .iter()
        .any(|d| d.rule == "W113"));
    let symbol = dispatch.cases[0].steps[0].symbol.as_ref().unwrap();
    assert!(cx
        .chain
        .scip
        .as_ref()
        .unwrap()
        .documentation(symbol)
        .unwrap()
        .documentation
        .iter()
        .any(|s| s.contains("exact match score")));
}
#[test]
fn real_python_and_java_resolve_calls() {
    for (lang, file, symbol, golden) in [
        ("py", "control.py", "control", "python-control-scip"),
        (
            "java",
            "src/main/java/demo/Control.java",
            "Control.control",
            "java-control-scip",
        ),
        ("ts", "src/control.ts", "control", "typescript-control-scip"),
    ] {
        let mut cx = Cx::load(&root(lang)).unwrap();
        cx.enable_scip(true).unwrap();
        let flow = cx.extract(file, symbol, Some("control"), 0).unwrap();
        assert_eq!(flow.resolution, ResolutionMode::Scip);
        let call = &flow.steps[0].body[1];
        assert!(call.symbol.is_some(), "{lang}: {call:?}");
        assert_eq!(call.kind, Kind::Call);
        let expected =
            std::fs::read_to_string(root("golden").join(format!("{golden}.flow.yaml"))).unwrap();
        assert_eq!(yaml::emit(&flow).unwrap(), expected);
    }
}
#[test]
fn stale_inlined_target_uses_heuristic_without_scip_symbol() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = root("ts");
    for file in [
        ".wtflow/config.yaml",
        ".wtflow/index/meta.yaml",
        ".wtflow/index/typescript.scip",
        "src/reconciliation/service.ts",
        "src/matching/service.ts",
    ] {
        let path = dir.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(fixture.join(file), path).unwrap();
    }
    let source = dir.path().join("src/matching/service.ts");
    std::fs::write(
        &source,
        std::fs::read_to_string(&source)
            .unwrap()
            .replace("attempts < 3", "attempts < 5"),
    )
    .unwrap();
    let mut cx = Cx::load(dir.path()).unwrap();
    cx.enable_scip(true).unwrap();
    let flow = cx
        .extract(
            "src/reconciliation/service.ts",
            "ReconciliationService.reconcile",
            None,
            2,
        )
        .unwrap();
    assert_eq!(flow.resolution, ResolutionMode::Mixed);
    let mut nodes = vec![];
    wtflow_core::visit(&flow.steps, &mut nodes);
    let group = nodes
        .iter()
        .find(|n| n.target.as_deref() == Some("MatchingService.match"))
        .unwrap();
    assert!(group.symbol.is_none());
    assert_eq!(cx.stale_for(&flow), vec!["src/matching/service.ts"]);
}
#[test]
fn package_indexes_merge_by_repository_relative_path() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".wtflow.yaml"), "collapse: true\n").unwrap();
    for (lang, source, index) in [
        ("py", "control.py", "python.scip"),
        ("java", "src/main/java/demo/Control.java", "java.scip"),
    ] {
        for relative in [
            source,
            &format!(".wtflow/index/{index}"),
            ".wtflow/index/meta.yaml",
        ] {
            let target = dir.path().join(lang).join(relative);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::copy(root(lang).join(relative), target).unwrap();
        }
    }
    let mut cx = Cx::load(dir.path()).unwrap();
    cx.enable_scip(true).unwrap();
    for (file, symbol) in [
        ("py/control.py", "control"),
        ("java/src/main/java/demo/Control.java", "Control.control"),
    ] {
        let flow = cx.extract(file, symbol, None, 0).unwrap();
        assert_eq!(flow.resolution, ResolutionMode::Scip);
        assert!(flow.steps[0].body[1].symbol.is_some());
        assert!(cx.stale_for(&flow).is_empty());
    }
}
#[test]
fn context_packets_have_real_documentation_paths_neighbors_and_glossary() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = root("ts");
    for file in [
        ".wtflow/config.yaml",
        "src/dispatch.ts",
        "src/matching/matchers.ts",
        "src/matching/index.ts",
        ".wtflow/index/meta.yaml",
        ".wtflow/index/typescript.scip",
    ] {
        let path = dir.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(fixture.join(file), path).unwrap();
    }
    std::fs::write(
        dir.path().join("glossary.yaml"),
        "version: '1'\nterms:\n  matcher: matching strategy\n",
    )
    .unwrap();
    let mut cx = Cx::load(dir.path()).unwrap();
    cx.enable_scip(true).unwrap();
    let mut flow = cx
        .extract("src/dispatch.ts", "Dispatch.run", None, 2)
        .unwrap();
    let fingerprint = flow.fingerprint.clone();
    let packets = wtflow_extract::context::packets(&cx, &flow, false).unwrap();
    let packet = packets
        .iter()
        .find(|p| p.node.target.as_deref() == Some("ExactMatcher.match"))
        .unwrap();
    assert!(packet.callee.as_ref().unwrap().signature.contains("match"));
    assert!(packet
        .callee
        .as_ref()
        .unwrap()
        .documentation
        .iter()
        .any(|s| s.contains("exact match score")));
    assert_eq!(packet.ancestor_path, vec![flow.steps[1].id.clone()]);
    assert_eq!(
        packet.glossary.as_ref().unwrap()["terms"]["matcher"],
        "matching strategy"
    );
    assert_eq!(
        packets[0].neighbors.next.as_deref(),
        Some(flow.steps[1].id.as_str())
    );
    assert!(packets[0].neighbors.previous.is_none());
    assert_eq!(packets[0].path, "steps[0]");
    let before = serde_json::to_string(&packets).unwrap();
    assert_eq!(
        serde_json::to_string(&wtflow_extract::context::packets(&cx, &flow, false).unwrap())
            .unwrap(),
        before
    );
    flow.steps[0].label = Some("Normalize".into());
    assert!(wtflow_extract::context::packets(&cx, &flow, false)
        .unwrap()
        .iter()
        .all(|p| p.node.id != flow.steps[0].id));
    assert!(wtflow_extract::context::packets(&cx, &flow, true)
        .unwrap()
        .iter()
        .any(|p| p.node.id == flow.steps[0].id));
    assert_eq!(flow.fingerprint, fingerprint);
    flow.verify_fingerprint().unwrap();
}
