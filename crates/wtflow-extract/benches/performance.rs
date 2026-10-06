use criterion::{black_box, criterion_group, criterion_main, Criterion};
use protobuf::Message;
use scip::types::{Document, Index, Metadata, Occurrence, ToolInfo};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use wtflow_extract::Cx;
use wtflow_resolve::{metadata, scip::ScipResolver};
fn benches(c: &mut Criterion) {
    let repo = tempfile::tempdir().expect("benchmark temporary repository");
    std::fs::write(repo.path().join(".wtflow.yaml"), "collapse: false\n").expect("config");
    for i in 0..1500 {
        std::fs::write(
            repo.path().join(format!("entry{i:04}.ts")),
            format!("class C{i} {{ @Post('/items/{i}') run(value: string) {{ send(value); }} }}\n"),
        )
        .expect("source fixture");
    }
    let mut group = c.benchmark_group("acceptance");
    group
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    group.bench_function("entrypoints_1500_ts", |b| {
        b.iter(|| {
            let cx = Cx::load(repo.path()).expect("parse fixtures");
            black_box(cx.entrypoints());
        })
    });
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ts");
    let large = tempfile::tempdir().expect("large index repository");
    let index_dir = large.path().join(".wtflow/index");
    std::fs::create_dir_all(&index_dir).expect("index directory");
    std::fs::write(large.path().join(".wtflow.yaml"), "collapse: false\n").expect("config");
    let mut index = Index::parse_from_bytes(
        &std::fs::read(fixture.join(".wtflow/index/typescript.scip")).expect("real index"),
    )
    .expect("decode fixture index");
    index.metadata = protobuf::MessageField::some(Metadata {
        tool_info: protobuf::MessageField::some(ToolInfo {
            name: "scip-typescript".into(),
            version: "benchmark".into(),
            ..ToolInfo::default()
        }),
        ..Metadata::default()
    });
    let mut sources: BTreeMap<String, Arc<str>> = BTreeMap::new();
    let mut hashes = BTreeMap::new();
    for doc in &index.documents {
        let source =
            std::fs::read_to_string(fixture.join(&doc.relative_path)).expect("fixture source");
        let target = large.path().join(&doc.relative_path);
        std::fs::create_dir_all(target.parent().expect("source parent")).expect("parent");
        std::fs::write(target, &source).expect("copy source");
        hashes.insert(doc.relative_path.clone(), metadata::hash(source.as_bytes()));
        sources.insert(doc.relative_path.clone(), Arc::from(source));
    }
    // A real occurrence-heavy 50 MB corpus: 350 references per document, not padding.
    let external = format!(
        "scip-typescript npm benchmark-package 1.0.0 `{}`/external().",
        "module".repeat(12)
    );
    let text = "external();\n".repeat(350);
    let mut size = index.compute_size();
    let mut i = 0;
    while size < 50_000_000 {
        let path = format!("bulk/file{i:04}.ts");
        let doc = Document {
            relative_path: path.clone(),
            position_encoding: protobuf::EnumOrUnknown::from_i32(1),
            occurrences: (0..350)
                .map(|line| Occurrence {
                    range: vec![line, 0, 8],
                    symbol: external.clone(),
                    ..Occurrence::default()
                })
                .collect(),
            ..Document::default()
        };
        size += doc.compute_size() + 5;
        index.documents.push(doc);
        sources.insert(path.clone(), Arc::from(text.clone()));
        hashes.insert(path.clone(), metadata::hash(text.as_bytes()));
        let target = large.path().join(path);
        std::fs::create_dir_all(target.parent().expect("parent")).expect("bulk directory");
        std::fs::write(target, &text).expect("bulk source");
        i += 1;
    }
    let bytes = index.write_to_bytes().expect("serialize benchmark index");
    let index_path = index_dir.join("typescript.scip");
    std::fs::write(&index_path, &bytes).expect("index file");
    let meta = metadata::Metadata {
        commit: "benchmark".into(),
        dirty: false,
        files: hashes,
        indexers: BTreeMap::from([("typescript".into(), "benchmark".into())]),
    };
    std::fs::write(index_dir.join("meta.yaml"), meta.emit().expect("metadata"))
        .expect("metadata file");
    eprintln!(
        "benchmark SCIP: {} bytes, {} documents, {} source bytes",
        bytes.len(),
        index.documents.len(),
        sources.values().map(|s| s.len()).sum::<usize>()
    );
    group.bench_function("load_scip_50mb", |b| {
        b.iter(|| {
            black_box(
                ScipResolver::load(large.path(), std::slice::from_ref(&index_path), &sources)
                    .expect("load index"),
            )
        })
    });
    let mut cx = Cx::load(large.path()).expect("parse large repository");
    cx.enable_scip(true).expect("enable SCIP");
    group.bench_function("extract_depth3_after_50mb_load", |b| {
        b.iter(|| {
            black_box(
                cx.extract("src/dispatch.ts", "Dispatch.run", None, 3)
                    .expect("extract"),
            )
        })
    });
    group.finish();
}
criterion_group!(performance, benches);
criterion_main!(performance);
