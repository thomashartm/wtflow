use std::fs;
use wtflow_core::{visit, yaml, Kind};
use wtflow_extract::{Cx, DEFAULT_DEPTH};

fn project(source: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".wtflow.yaml"), "collapse: false\n").unwrap();
    fs::write(root.path().join("app.ts"), source).unwrap();
    root
}

#[test]
fn follows_to_the_leaf_and_retains_recursion_as_a_visible_call() {
    let root = project("class App { run() { return this.a(); } a() { return this.b(); } b() { return this.c(); } c() { if (ready) { publish('finished'); } else { this.run(); } return result; } }");
    let cx = Cx::load(root.path()).unwrap();
    let result = cx
        .extract_report("app.ts", "App.run", None, DEFAULT_DEPTH)
        .unwrap();
    let mut nodes = vec![];
    visit(&result.flow.steps, &mut nodes);
    assert!(nodes.iter().any(|n| n.code == "publish('finished')"));
    assert!(nodes
        .iter()
        .any(|n| n.kind == Kind::Call && n.target.as_deref() == Some("App.run")));
    assert!(result.notes.iter().any(|n| n.contains("recursive call")));
    assert_eq!(nodes.iter().filter(|n| n.kind == Kind::Group).count(), 3);
    let output = yaml::emit(&result.flow).unwrap();
    assert_eq!(
        output,
        yaml::emit(&yaml::load(&output, "flow.yaml").unwrap()).unwrap()
    );
}

#[test]
fn depth_and_expansion_guards_are_deterministic_and_keep_calls() {
    let mut source = String::from("class App { run() { this.f0(); } ");
    for n in 0..35 {
        source.push_str(&format!("f{n}() {{ this.f{}(); }} ", n + 1));
    }
    source.push_str("f35() { finish(); } }");
    let root = project(&source);
    let cx = Cx::load(root.path()).unwrap();
    let result = cx
        .extract_report("app.ts", "App.run", None, DEFAULT_DEPTH)
        .unwrap();
    assert!(result.notes.iter().any(|n| n.contains("depth limit")));
    let text = yaml::emit(&result.flow).unwrap();
    yaml::load(&text, "deep.flow.yaml").unwrap();
    let mut source = String::from("class App { run() { this.f0(); } ");
    for n in 0..12 {
        source.push_str(&format!(
            "f{n}() {{ this.f{}(); this.f{}(); }} ",
            n + 1,
            n + 1
        ));
    }
    source.push_str("f12() { finish(); } }");
    fs::write(root.path().join("app.ts"), source).unwrap();
    let cx = Cx::load(root.path()).unwrap();
    let a = cx
        .extract_report("app.ts", "App.run", None, DEFAULT_DEPTH)
        .unwrap();
    let b = cx
        .extract_report("app.ts", "App.run", None, DEFAULT_DEPTH)
        .unwrap();
    assert!(a.notes.iter().any(|n| n.contains("flow size limit")));
    assert_eq!(a.notes, b.notes);
    assert_eq!(yaml::emit(&a.flow).unwrap(), yaml::emit(&b.flow).unwrap());
    let mut nodes = vec![];
    visit(&a.flow.steps, &mut nodes);
    assert!(nodes.len() < 2000);
    assert!(nodes.iter().any(|n| n.kind == Kind::Call));
}
