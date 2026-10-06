use std::fs;
use wtflow_extract::{context::flow_context, Cx};

#[test]
fn call_purpose_and_return_types_come_from_definitions_in_each_language() {
    for (file, entry, source, output) in [
        ("app.ts", "App.run", "class App { run() { this.score(); }\n/** Calculate the matching score. */\nscore(): Promise<number> { return compute(); } }", "Promise<number>"),
        ("app.py", "App.run", "class App:\n    def run(self):\n        self.score()\n    def score(self) -> float:\n        \"\"\"Calculate the matching score.\"\"\"\n        return compute()\n", "float"),
        ("App.java", "App.run", "class App { void run() { this.score(); }\n/** Calculate the matching score. */\nfloat score() { return compute(); } }", "float"),
    ] {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(".wtflow.yaml"), "collapse: false\n").unwrap();
        fs::write(root.path().join(file), source).unwrap();
        let cx = Cx::load(root.path()).unwrap();
        let flow = cx.extract(file, entry, None, 0).unwrap();
        let context = flow_context(&cx, &flow);
        let info = context.calls.values().find(|c| c.return_type == output)
            .unwrap_or_else(|| panic!("{file}: {:#?}", context.calls));
        assert_eq!(info.documentation, ["Calculate the matching score."]);
        assert!(info.definition.starts_with(file));
        fs::write(root.path().join(file), source.replace("Calculate the matching score.", "New explanation.")).unwrap();
        let updated = Cx::load(root.path()).unwrap().extract(file, entry, None, 0).unwrap();
        assert_eq!(flow.fingerprint, updated.fingerprint);
    }
}
