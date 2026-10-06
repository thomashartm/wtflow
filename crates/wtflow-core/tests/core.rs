use wtflow_core::{fingerprint, ids, schema, yaml, Entry, Flow, Kind, Node, ResolutionMode, State};
fn sample() -> Flow {
    let mut n = Node::new(Kind::If, "openItems.length === 0");
    n.src = "src/service.ts:17-20".into();
    n.then.push(Node::new(Kind::Return, "return report"));
    let mut f = Flow {
        flow: "reconciliation.reconcile".into(),
        version: 1,
        owner: "accounting-core".into(),
        trigger: "event bank.statement.imported".into(),
        entry: Entry {
            lang: "typescript".into(),
            file: "src/service.ts".into(),
            symbol: "Service.reconcile".into(),
            depth: 2,
        },
        resolution: ResolutionMode::Heuristic,
        fingerprint: String::new(),
        inputs: vec![],
        output: String::new(),
        state: State::default(),
        boundaries: vec![],
        steps: vec![n],
    };
    ids::assign(&mut f.steps);
    f.refresh_fingerprint().unwrap();
    f
}
#[test]
fn canonical_round_trip_and_schema() {
    let f = sample();
    let text = yaml::emit(&f).unwrap();
    assert_eq!(yaml::load(&text, "sample.flow.yaml").unwrap(), f);
    assert_eq!(
        yaml::emit(&yaml::load(&text, "sample").unwrap()).unwrap(),
        text
    );
    assert!(text.contains("  - id: if_open_items\n    kind: if\n"));
    schema::validate(&serde_json::to_value(&f).unwrap(), false).unwrap();
}
#[test]
fn schema_rejects_unknown_kind_children_properties_and_absolute_paths() {
    let value = serde_json::to_value(sample()).unwrap();
    for (key, val) in [
        ("kind", serde_json::json!("unknown")),
        ("body", serde_json::json!([])),
        ("typo", serde_json::json!(true)),
    ] {
        let mut v = value.clone();
        v["steps"][0][key] = val;
        assert!(schema::validate(&v, false).is_err(), "{key}");
    }
    for path in [
        "/tmp/source.ts",
        "../outside.ts",
        "C:/source.ts",
        "src/../other.ts",
    ] {
        let mut v = value.clone();
        v["entry"]["file"] = path.into();
        assert!(schema::validate(&v, false).is_err(), "{path}");
    }
}
#[test]
fn config_schema_is_strict_at_every_object() {
    for bad in [
        serde_json::json!({"unknown":1}),
        serde_json::json!({"index":{"typescript":{"unknown":true}}}),
        serde_json::json!({"rules":[{"kind":"emit"}]}),
        serde_json::json!({"modules":[{"path":"src","owner":"a","typo":1}]}),
    ] {
        assert!(schema::validate(&bad, true).is_err());
    }
    schema::validate(
        &serde_json::json!({"rules":[{"match":"foo", "symbol":"bar", "kind":"emit"}]}),
        true,
    )
    .unwrap();
}
#[test]
fn fingerprint_contract_excludes_only_label_and_src() {
    let mut n = Node::new(Kind::Call, "run()");
    n.id = "call_run".into();
    n.target = Some("run".into());
    n.symbol = Some("scip symbol".into());
    assert_eq!(
        String::from_utf8(fingerprint::canonical_json(&[n.clone()]).unwrap()).unwrap(),
        r#"[{"id":"call_run","kind":"call","code":"run()","target":"run","symbol":"scip symbol"}]"#
    );
    let before = fingerprint::compute(&[n.clone()]).unwrap();
    n.label = Some("Run".into());
    n.src = "x.ts:99".into();
    assert_eq!(before, fingerprint::compute(&[n.clone()]).unwrap());
    n.code = "run(2)".into();
    assert_ne!(before, fingerprint::compute(&[n]).unwrap());
    assert_eq!(fingerprint::compute(&[]).unwrap(), "4f53cda18c2baa0c");
}
#[test]
fn ids_prefer_topics_callees_and_are_unique_in_document_order() {
    let mut nodes = vec![
        Node::new(
            Kind::Emit,
            "this.pubsub.publish('bank.statement.imported', value)",
        ),
        Node::new(Kind::Call, "this.matchingService.findMatches(x)"),
        Node::new(Kind::Do, "const x = await save()"),
        Node::new(Kind::Do, "const x = await save()"),
        Node::new(Kind::Continue, "continue"),
    ];
    ids::assign(&mut nodes);
    assert_eq!(
        nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        vec![
            "emit_bank_statement_imported",
            "call_matching_service_find",
            "x_save",
            "x_save_2",
            "continue"
        ]
    );
    let mut long = vec![Node::new(Kind::Call, "someVeryLongIdentifierWithManyWords()"); 12];
    ids::assign(&mut long);
    assert!(long.iter().all(|n| n.id.len() <= 36));
}
#[test]
fn scalars_roundtrip_as_strings() {
    for s in [
        "",
        "true",
        "Null",
        "yes",
        "1.2",
        "0x10",
        "2024-01-02",
        "-abc",
        "a: b",
        "a #comment",
        " a",
        "a ",
        "[x]",
        "a\nb",
        "a\t\"b",
        "日本語",
        "💡abc",
        "plain",
        "https://example.com",
    ] {
        let q = yaml::quote(s).unwrap();
        assert_eq!(serde_yaml_ng::from_str::<String>(&q).unwrap(), s, "{q}");
    }
}
#[test]
fn collapsed_code_and_empty_lists_roundtrip() {
    let mut f = sample();
    f.steps = vec![Node::new(Kind::Do, "first()\nsecond()")];
    ids::assign(&mut f.steps);
    f.refresh_fingerprint().unwrap();
    let text = yaml::emit(&f).unwrap();
    assert!(text.contains("code: |-\n      first()\n      second()\n"));
    assert_eq!(yaml::load(&text, "test").unwrap(), f);
    f.steps.clear();
    f.refresh_fingerprint().unwrap();
    assert!(yaml::emit(&f).unwrap().ends_with("steps: []\n"));
}
#[test]
fn golden_roundtrips() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/golden");
    for file in std::fs::read_dir(dir).unwrap() {
        let path = file.unwrap().path();
        if path.to_string_lossy().ends_with(".flow.yaml") {
            let text = std::fs::read_to_string(&path).unwrap();
            let f = yaml::load(&text, &path.display().to_string()).unwrap();
            assert_eq!(yaml::emit(&f).unwrap(), text, "{}", path.display());
            f.verify_fingerprint().unwrap();
        }
    }
}
