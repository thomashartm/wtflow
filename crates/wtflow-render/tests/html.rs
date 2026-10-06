use wtflow_core::{Kind, Node};
#[test]
fn offline_view_preserves_branches_and_escapes_source_labels_and_notes() {
    let mut flow = wtflow_core::yaml::load(
        include_str!("../../../testdata/golden/core.flow.yaml"),
        "test",
    )
    .unwrap();
    flow.flow = "</title><script>bad()</script>".into();
    let mut branch = Node::new(Kind::If, "a < b");
    branch.label = Some("<img src=x onerror=bad()>".into());
    branch.then.push(Node::new(Kind::Return, "return a"));
    branch.otherwise.push(Node::new(Kind::Fail, "throw b"));
    flow.steps = vec![branch];
    let html = wtflow_render::html::render(&flow, "<script>bad()</script>");
    assert!(html.contains("&lt;script&gt;bad()&lt;/script&gt;"));
    assert!(!html.contains("<script>bad()"));
    assert!(!html.contains("<img"));
    assert!(html.contains("a &lt; b"));
    assert!(html.contains("<h3>Yes</h3>"));
    assert!(html.contains("<h3>No</h3>"));
    assert!(html.contains("return a"));
    assert!(html.contains("throw b"));
    assert_eq!(
        html,
        wtflow_render::html::render(&flow, "<script>bad()</script>")
    );
}

#[test]
fn loops_are_highlighted_and_call_context_is_safe_and_bound_to_structure() {
    use wtflow_core::context::{CallDetails, FlowContext};
    let mut flow = wtflow_core::yaml::load(
        include_str!("../../../testdata/golden/core.flow.yaml"),
        "test",
    )
    .unwrap();
    let mut call = Node::new(Kind::Call, "this.score()");
    call.id = "call_score".into();
    call.label = Some("My purpose".into());
    let mut each = Node::new(Kind::ForEach, "item of items");
    each.body.push(call);
    let mut repeat = Node::new(Kind::While, "retry < 3");
    repeat.body.push(Node::new(Kind::Continue, "continue"));
    flow.steps = vec![each, repeat];
    let mut context = FlowContext {
        fingerprint: flow.fingerprint.clone(),
        ..Default::default()
    };
    context.calls.insert(
        "call_score".into(),
        CallDetails {
            return_type: "Promise<number>".into(),
            documentation: vec!["Score <script>unsafe</script>".into()],
            ..Default::default()
        },
    );
    let html = wtflow_render::html::render_with_context(&flow, "", &context);
    assert_eq!(
        html.matches("<span class=\"loop-label\">↻ LOOP</span>")
            .count(),
        2
    );
    assert_eq!(html.matches("<h3>Repeat body</h3>").count(), 2);
    assert!(html.contains("class=\"step k_for_each\""));
    assert!(html.contains("class=\"step k_while\""));
    assert!(html.contains("Returns: Promise&lt;number&gt;"));
    assert!(html.contains("Score &lt;script&gt;unsafe&lt;/script&gt;"));
    assert!(html.contains("data-id=\"call_score\" data-code=\"this.score()\""));
    assert!(html.contains(">My purpose</textarea>"));
    context.fingerprint = "stale".into();
    let html = wtflow_render::html::render_with_context(&flow, "", &context);
    assert!(html.contains("Returns: not available"));
    assert!(!html.contains("Score &lt;script&gt;"));
}
