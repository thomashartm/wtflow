use wtflow_extract::{
    functions,
    source::{descendants, SourceFile},
};

#[test]
fn grammar_compatibility_preserves_original_text_calls_and_positions() {
    let text = "// Unicode before offsets: café 🐈\n\
async function graphImports(): Promise<\n\
  NonNullable<import('@nestjs/common').ModuleMetadata['imports']>\n\
> {\n\
  const { using } = await readPolicy();\n\
  expect(using).not.toBeNull();\n\
  const module = await import('@nestjs/config');\n\
  return `${module.key}\0${module.language}`;\n\
}\n\
function httpImports(): NonNullable<import('@nestjs/common').ModuleMetadata['imports']> { return load(); }\n";
    let raw = SourceFile::parse_unchecked("case.ts".into(), text.into()).unwrap();
    assert!(raw.tree.root_node().has_error());
    let file = SourceFile::parse("case.ts".into(), text.into()).unwrap();
    assert_eq!(file.text, text);
    assert!(!file.tree.root_node().has_error());
    let funcs = functions::collect(&file);
    assert_eq!(funcs.len(), 2);
    assert_eq!(
        funcs[0].output,
        "Promise<\nNonNullable<import('@nestjs/common').ModuleMetadata['imports']>\n>"
    );
    assert_eq!(
        funcs[1].output,
        "NonNullable<import('@nestjs/common').ModuleMetadata['imports']>"
    );
    let calls = functions::all_calls(&file);
    assert!(calls.iter().any(|n| file.text(*n) == "expect(using)"));
    assert!(calls
        .iter()
        .any(|n| file.text(*n) == "import('@nestjs/config')"));
    assert!(!calls
        .iter()
        .any(|n| file.text(*n) == "import('@nestjs/common')"));
    let load = calls.iter().find(|n| file.text(**n) == "load()").unwrap();
    assert_eq!(load.start_byte(), text.find("load()").unwrap());
    assert_eq!(load.start_position().row, 9);
    let mut nodes = Vec::new();
    descendants(file.tree.root_node(), &mut nodes);
    assert!(nodes
        .iter()
        .any(|n| n.kind() == "template_string"
            && file.text(*n) == "`${module.key}\0${module.language}`"));
    assert!(nodes
        .iter()
        .any(|n| n.kind() == "identifier" && file.text(*n) == "using"));
}

#[test]
fn compatibility_does_not_accept_invalid_syntax_or_rewrite_runtime_imports() {
    for text in [
        "function f() { expect(using); broken( ; }",
        "function f(): NonNullable<import('pkg').Type> { broken( ; }",
        "function f() { return `ok\0${broken(}`; }",
        "function f() { \0 call(); }",
        "function f(): NonNullable<import('pkg', broken( ).Type> {}",
    ] {
        assert!(
            SourceFile::parse("invalid.ts".into(), text.into()).is_err(),
            "{text:?}"
        );
    }
    let text = "async function f() { using resource = acquire(); const s = 'using'; return import('pkg'); }";
    let file = SourceFile::parse("runtime.ts".into(), text.into()).unwrap();
    assert_eq!(file.text, text);
    assert!(functions::all_calls(&file)
        .iter()
        .any(|n| file.text(*n) == "import('pkg')"));
}

#[test]
fn method_decorators_survive_comments_and_parameter_decorators_are_not_triggers() {
    let text = "@Controller('orders')\nclass Orders {\n\
@Post(':id/approve')\n// A comment between decorators and the method\n\
approve(@Query('include') include: string) { return save(include); }\n\
helper(@Query('filter') filter: string) { return find(filter); }\n\
@Query('order')\nquery() { return find(); }\n}\n";
    let file = SourceFile::parse("orders.ts".into(), text.into()).unwrap();
    let funcs = functions::collect(&file);
    let triggers: Vec<_> = funcs
        .iter()
        .map(|f| wtflow_extract::entrypoints::trigger(&file, f))
        .collect();
    assert_eq!(
        triggers,
        ["http POST /orders/:id/approve", "", "task order"]
    );
}

#[test]
fn inline_comments_do_not_leak_quotes_or_newlines_into_flow_code() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".wtflow.yaml"), "collapse: false\n").unwrap();
    let source = r#"class Jobs {
      create() {
        this.createDraft({ ...body,
          // Written after the body, from the request's authentication.
          ...this.caller(request), /* don't treat " as a quote */
          url: 'https://example.com/a//b', text: `first
second`,
        });
      }
      /** Create an authorized draft. */
      createDraft(input: Input): Job { return save(input); }
    }"#;
    std::fs::write(root.path().join("jobs.ts"), source).unwrap();
    let cx = wtflow_extract::Cx::load(root.path()).unwrap();
    let flow = cx.extract("jobs.ts", "Jobs.create", None, 0).unwrap();
    let call = &flow.steps[0];
    assert!(!call.code.contains(['\n', '\r']));
    assert!(!call.code.contains("authentication"));
    assert!(call.code.contains("...this.caller(request)"));
    assert!(call.code.contains("'https://example.com/a//b'"));
    assert!(call.code.contains("`first\\nsecond`"));
    let yaml = wtflow_core::yaml::emit(&flow).unwrap();
    let loaded = wtflow_core::yaml::load(&yaml, "regression.flow.yaml").unwrap();
    assert_eq!(loaded.fingerprint, flow.fingerprint);
    let context = wtflow_extract::context::flow_context(&cx, &flow);
    assert_eq!(
        context.calls[&call.id].documentation,
        ["Create an authorized draft."]
    );
    assert_eq!(context.calls[&call.id].return_type, "Job");
    std::fs::write(
        root.path().join("jobs.ts"),
        source.replace("from the request's authentication.", "another explanation."),
    )
    .unwrap();
    let updated = wtflow_extract::Cx::load(root.path())
        .unwrap()
        .extract("jobs.ts", "Jobs.create", None, 0)
        .unwrap();
    assert_eq!(flow.fingerprint, updated.fingerprint);
}

#[test]
fn multiline_foreach_receivers_produce_valid_flow_code() {
    let root = tempfile::tempdir().unwrap();
    let source = r#"function process(requestedRelations: string[]) {
  [...requestedRelations]
    .filter((r) => r !== 'accountTransaction' && !r.startsWith('accountTransaction.'))
    .sort((a, b) => a.split('.').length - b.split('.').length)
    .forEach((relation) => { send(relation); });
}"#;
    let expected = "[...requestedRelations] .filter((r) => r !== 'accountTransaction' && !r.startsWith('accountTransaction.')) .sort((a, b) => a.split('.').length - b.split('.').length)";
    let mut fingerprint = None;
    for source in [
        source.to_owned(),
        source.replace('\n', "\r\n"),
        source.replace(
            "    .sort",
            "    // Don't let this comment's quote hide the next call.\n    .sort",
        ),
    ] {
        std::fs::write(root.path().join("relations.ts"), source).unwrap();
        let flow = wtflow_extract::Cx::load(root.path())
            .unwrap()
            .extract("relations.ts", "process", None, 0)
            .unwrap();
        let node = &flow.steps[0];
        assert_eq!(node.kind, wtflow_core::Kind::ForEach);
        assert_eq!(node.code, expected);
        assert_eq!(node.body[0].code, "send(relation)");
        let yaml = wtflow_core::yaml::emit(&flow).unwrap();
        let loaded = wtflow_core::yaml::load(&yaml, "relations.flow.yaml").unwrap();
        assert_eq!(loaded.steps, flow.steps);
        if let Some(previous) = &fingerprint {
            assert_eq!(&flow.fingerprint, previous);
        }
        fingerprint = Some(flow.fingerprint);
    }
}
