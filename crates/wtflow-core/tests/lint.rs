use wtflow_core::{
    ids,
    lint::{self, Context, Severity},
    yaml, Flow, Kind, Node,
};
fn flow(mut nodes: Vec<Node>) -> Flow {
    let mut f = yaml::load(
        include_str!("../../../testdata/golden/core.flow.yaml"),
        "core",
    )
    .unwrap();
    ids::assign(&mut nodes);
    f.steps = nodes;
    f.refresh_fingerprint().unwrap();
    f
}
fn rules(nodes: Vec<Node>) -> Vec<String> {
    lint::check(&flow(nodes), &Context::default())
        .into_iter()
        .map(|d| d.rule)
        .collect()
}
#[test]
fn every_warning_rule() {
    let mut switch = Node::new(Kind::Switch, "x");
    switch.cases.push(wtflow_core::Case {
        when: "a".into(),
        fallthrough: Some(true),
        steps: vec![],
    });
    let mut swallowed = Node::new(Kind::Try, "try");
    swallowed.catch.push(Node::new(Kind::Do, "(error ignored)"));
    let mut write = Node::new(Kind::Call, "save()");
    write.writes.push("item".into());
    let mut tx = Node::new(Kind::Group, "transaction");
    tx.tx = Some("db".into());
    tx.body.push(Node::new(Kind::Emit, "publish()"));
    let mut external = Node::new(Kind::Call, "ocr()");
    external.boundary = Some("ocr".into());
    tx.body.push(external);
    let mut dispatch = Node::new(Kind::Switch, "dispatch matcher");
    dispatch.cases = vec![
        wtflow_core::Case {
            when: "a".into(),
            fallthrough: None,
            steps: vec![],
        },
        wtflow_core::Case {
            when: "b".into(),
            fallthrough: None,
            steps: vec![],
        },
    ];
    for (rule, nodes) in [
        (
            "W101",
            vec![Node::new(Kind::Return, "return"), write.clone()],
        ),
        ("W102", vec![switch.clone()]),
        ("W103", vec![switch]),
        ("W104", vec![swallowed]),
        ("W105", vec![write.clone(), write.clone()]),
        ("W106", vec![Node::new(Kind::While, "true")]),
        ("W107", vec![Node::new(Kind::If, "x")]),
        ("W110", vec![tx.clone()]),
        ("W112", vec![tx]),
        ("W113", vec![dispatch.clone()]),
        ("I001", vec![write]),
        ("I002", vec![Node::new(Kind::Do, "unknown()")]),
    ] {
        assert!(rules(nodes).contains(&rule.into()), "{rule}");
    }
    assert!(!rules(vec![dispatch]).contains(&"W102".into()));
}
#[test]
fn freshness_strictness_and_severity_order() {
    let f = flow(vec![Node::new(Kind::Do, "unknown()")]);
    let mut ctx = Context {
        stale_files: vec!["src/b.ts".into(), "src/a.ts".into()],
        ..Context::default()
    };
    let ds = lint::check(&f, &ctx);
    assert_eq!(ds[0].rule, "W120");
    assert!(ds[0].message.ends_with("a.ts"));
    assert!(!lint::fails(&ds, false));
    assert!(lint::fails(&ds, true));
    ctx.source = true;
    assert_eq!(lint::check(&f, &ctx)[0].severity, Severity::Error);
    ctx.source_changed = true;
    assert!(lint::check(&f, &ctx).iter().any(|d| d.rule == "E005"));
}
#[test]
fn structural_errors_and_parse_errors() {
    let f = flow(vec![Node::new(Kind::Do, "x()")]);
    let text = yaml::emit(&f).unwrap();
    for (rule, bad) in [
        ("E000", "[".into()),
        ("E000", text.replace("version: 1", "version: 2")),
        ("E001", text.replace("kind: do", "kind: unknown")),
        ("E002", text.replace("id: x", "id: ''")),
        ("E003", text.replace("code: x()", "code: y()")),
        ("E004", text.replace("kind: do", "kind: do\n    body: []")),
        (
            "E006",
            text.replace("kind: do", "kind: do\n    extra: true"),
        ),
    ] {
        assert!(
            lint::document(&bad, "test", &Context::default())
                .iter()
                .any(|d| d.rule == rule),
            "{rule}"
        );
    }
    let mut duplicate = f.clone();
    duplicate.steps.push(duplicate.steps[0].clone());
    duplicate.refresh_fingerprint().unwrap();
    assert!(lint::check(&duplicate, &Context::default())
        .iter()
        .any(|d| d.rule == "E002"));
}
#[test]
fn nested_transactions_and_loop_exits_avoid_false_positives() {
    let mut loop_node = Node::new(Kind::While, "true");
    loop_node.body.push(Node::new(Kind::Break, "break"));
    assert!(!rules(vec![loop_node]).contains(&"W106".into()));
    let mut write = Node::new(Kind::Call, "save()");
    write.writes.push("item".into());
    let mut group = Node::new(Kind::Group, "tx");
    group.tx = Some("db".into());
    group.body = vec![write.clone(), write.clone()];
    assert!(!rules(vec![group]).contains(&"W105".into()));
    let mut group = Node::new(Kind::Group, "inline");
    group.body.push(Node::new(Kind::Return, "return"));
    assert!(!rules(vec![group, write]).contains(&"W101".into()));
}
