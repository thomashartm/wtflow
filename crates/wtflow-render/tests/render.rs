use wtflow_core::{yaml, Case, Kind, Node};
use wtflow_render::{render, Language, Options};
fn graph(nodes: Vec<Node>, de: bool) -> String {
    let mut f = yaml::load(
        include_str!("../../../testdata/golden/core.flow.yaml"),
        "core",
    )
    .unwrap();
    f.steps = nodes;
    render(
        &f,
        &Options {
            lang: if de { Language::De } else { Language::En },
            detail: true,
        },
    )
    .unwrap()
}
#[test]
fn branches_loops_groups_and_terminals() {
    let mut conditional = Node::new(Kind::If, "a < b");
    conditional.then.push(Node::new(Kind::Continue, "continue"));
    conditional.otherwise.push(Node::new(Kind::Break, "break"));
    let mut loop_node = Node::new(Kind::ForEach, "items");
    loop_node.body.push(conditional);
    let mut group = Node::new(Kind::Group, "helper");
    group.body.push(Node::new(Kind::Return, "return result"));
    let mut call = Node::new(Kind::Call, "save(\"x\")");
    call.label = Some("Save".into());
    call.writes = vec!["item".into()];
    call.boundary = Some("storage".into());
    let nodes = vec![
        loop_node,
        group,
        call,
        Node::new(Kind::Return, "return done"),
        Node::new(Kind::Do, "unreachable()"),
    ];
    let text = graph(nodes.clone(), false);
    assert!(text.contains("n3 --> n1"));
    assert!(text.contains("n4 --> n5"));
    assert!(text.contains("n6 --> n7"));
    assert!(!text.contains("n8 --> n9"));
    insta::assert_snapshot!("branches_en", text);
    insta::assert_snapshot!("branches_de", graph(nodes, true));
}
#[test]
fn switch_try_parallel_and_shapes() {
    let mut switch = Node::new(Kind::Switch, "status");
    switch.cases = vec![
        Case {
            when: "a".into(),
            fallthrough: Some(true),
            steps: vec![Node::new(Kind::Call, "a()")],
        },
        Case {
            when: "b".into(),
            fallthrough: None,
            steps: vec![Node::new(Kind::Break, "break")],
        },
    ];
    switch.default.push(Node::new(Kind::Wait, "wait"));
    let mut tx = Node::new(Kind::Try, "transaction");
    tx.tx = Some("db".into());
    tx.body.push(switch);
    tx.catch.push(Node::new(Kind::Fail, "throw error"));
    tx.finally.push(Node::new(Kind::Call, "close()"));
    let mut par = Node::new(Kind::Parallel, "all");
    par.body = vec![
        Node::new(Kind::Call, "one()"),
        Node::new(Kind::Call, "two()"),
    ];
    let mut emit = Node::new(Kind::Emit, "publish()");
    emit.topic = Some("done".into());
    insta::assert_snapshot!("switch_try_parallel", graph(vec![tx, par, emit], false));
}
#[test]
fn text_escape_unicode_and_limit() {
    let n = Node::new(Kind::Do, format!("{}<\">", "日".repeat(61)));
    let text = graph(vec![n], false);
    assert!(text.contains(&format!("{}…", "日".repeat(59))));
    let text = graph(vec![Node::new(Kind::Do, "x < \"y\" > z")], false);
    assert!(text.contains("#lt; #quot;y#quot; #gt;"));
    assert!(!text.contains(":::call"));
}
