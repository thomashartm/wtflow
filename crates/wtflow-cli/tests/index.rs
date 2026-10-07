#![cfg(unix)]
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};

fn project() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("bin")).unwrap();
    fs::write(
        root.path().join(".wtflow.yaml"),
        "index:\n  typescript:\n    enabled: true\n",
    )
    .unwrap();
    fs::write(
        root.path().join("entry.ts"),
        "type Imports = NonNullable<import('@nestjs/common').ModuleMetadata['imports']>;\n",
    )
    .unwrap();
    root
}
fn index(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wtflow"))
        .current_dir(root)
        .env("PATH", root.join("bin"))
        .arg("index")
        .args(args)
        .output()
        .unwrap()
}
fn indexer(root: &Path, body: &str) {
    let path = root.join("bin/npx");
    fs::write(&path, format!("#!/bin/sh\nif [ \"$3\" = --version ]; then printf 'test-indexer 1.0\\n'; exit 0; fi\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn indexing_does_not_parse_source_and_logs_both_streams_with_unique_references() {
    let root = project();
    indexer(root.path(), "printf 'indexer stdout\\n'\nprintf 'indexer stderr\\n' >&2\nwhile [ \"$#\" -gt 0 ]; do if [ \"$1\" = --output ]; then shift; destination=$1; fi; shift; done\nprintf 'fixture index' > \"$destination\"");
    let output = index(root.path(), &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(".wtflow/logs/index-000001.log"));
    assert!(!stderr.contains("indexer stdout"));
    assert!(!stderr.contains("indexer stderr"));
    assert!(!stderr.contains('\x1b'));
    let log = fs::read_to_string(root.path().join(".wtflow/logs/index-000001.log")).unwrap();
    for expected in [
        "test-indexer 1.0",
        "indexer stdout",
        "indexer stderr",
        "Result: success",
    ] {
        assert!(log.contains(expected), "{log}");
    }
    let meta = wtflow_resolve::metadata::Metadata::load(root.path())
        .unwrap()
        .unwrap();
    assert!(meta.fresh("entry.ts", &fs::read(root.path().join("entry.ts")).unwrap()));
    let again = index(root.path(), &[]);
    assert!(again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("index-000002.log"));
    let cached = fs::read_to_string(root.path().join(".wtflow/logs/index-000002.log")).unwrap();
    assert!(cached.contains("no indexers were run"));
    assert_eq!(
        log,
        fs::read_to_string(root.path().join(".wtflow/logs/index-000001.log")).unwrap()
    );
}

#[test]
fn indexer_failure_and_missing_tool_keep_logs_and_fail_the_command() {
    let root = project();
    let missing = index(root.path(), &[]);
    assert_eq!(missing.status.code(), Some(2));
    let stderr = String::from_utf8(missing.stderr).unwrap();
    assert!(stderr.contains("missing tool npx"));
    assert!(stderr.contains("index-000001.log"));
    assert!(
        fs::read_to_string(root.path().join(".wtflow/logs/index-000001.log"))
            .unwrap()
            .contains("Result: failed")
    );
    indexer(
        root.path(),
        "printf 'build failed in indexer\\n' >&2\nexit 7",
    );
    let failed = index(root.path(), &["--no-progress"]);
    assert_eq!(failed.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("index-000002.log"));
    let log = fs::read_to_string(root.path().join(".wtflow/logs/index-000002.log")).unwrap();
    assert!(log.contains("build failed in indexer"));
    assert!(log.contains("Result: failed"));
    assert!(!root.path().join(".wtflow/index/meta.yaml").exists());
}

#[test]
fn failed_rebuild_does_not_replace_the_previous_index_or_metadata() {
    let root = project();
    fs::create_dir_all(root.path().join(".wtflow/index")).unwrap();
    fs::write(
        root.path().join(".wtflow/index/typescript.scip"),
        "previous index",
    )
    .unwrap();
    let meta = wtflow_resolve::metadata::Metadata::default()
        .emit()
        .unwrap();
    fs::write(root.path().join(".wtflow/index/meta.yaml"), &meta).unwrap();
    indexer(root.path(), "while [ \"$#\" -gt 0 ]; do if [ \"$1\" = --output ]; then shift; destination=$1; fi; shift; done\nprintf 'partial index' > \"$destination\"\nexit 7");
    assert!(!index(root.path(), &["--force"]).status.success());
    assert_eq!(
        fs::read_to_string(root.path().join(".wtflow/index/typescript.scip")).unwrap(),
        "previous index"
    );
    assert_eq!(
        fs::read_to_string(root.path().join(".wtflow/index/meta.yaml")).unwrap(),
        meta
    );
    assert_eq!(
        fs::read_dir(root.path().join(".wtflow/index"))
            .unwrap()
            .count(),
        2
    );
}
