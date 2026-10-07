use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};
fn clear(root: &Path, args: &[&str], answer: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wtflow"))
        .current_dir(root)
        .arg("clear")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(answer.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}
fn project(root: &Path) {
    fs::create_dir_all(root.join(".wtflow/flows")).unwrap();
    fs::create_dir_all(root.join(".wtflow/index")).unwrap();
    fs::create_dir_all(root.join(".wtflow/logs")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/app.ts"), "source").unwrap();
    fs::write(
        root.join(".wtflow/config.yaml"),
        "# keep settings\ncollapse: false\n",
    )
    .unwrap();
    for path in [
        "flows/example.flow.yaml",
        "index/typescript.scip",
        "logs/run.log",
        "entrypoints.json",
    ] {
        fs::write(root.join(".wtflow").join(path), "generated").unwrap();
    }
}
#[test]
fn clear_defaults_to_keep_config_and_removes_generated_content_only() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let output = clear(&root.path().join("src"), &[], "\n");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Keep config.yaml? [Y/n]"));
    assert_eq!(
        fs::read_to_string(root.path().join(".wtflow/config.yaml")).unwrap(),
        "# keep settings\ncollapse: false\n"
    );
    assert_eq!(
        fs::read_dir(root.path().join(".wtflow")).unwrap().count(),
        1
    );
    assert_eq!(
        fs::read_to_string(root.path().join("src/app.ts")).unwrap(),
        "source"
    );
}
#[test]
fn clear_can_remove_config_explicitly_in_another_directory_even_if_invalid() {
    let root = tempfile::tempdir().unwrap();
    let project_dir = root.path().join("project");
    project(&project_dir);
    fs::write(project_dir.join(".wtflow/config.yaml"), "[invalid").unwrap();
    let output = clear(root.path(), &["--dir", "project"], "maybe\nno\n");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_dir(project_dir.join(".wtflow")).unwrap().count(),
        0
    );
    assert!(project_dir.join("src/app.ts").exists());
}
#[test]
fn cancelled_or_missing_store_is_not_modified() {
    let root = tempfile::tempdir().unwrap();
    assert!(clear(root.path(), &[], "").status.success());
    assert!(!root.path().join(".wtflow").exists());
    project(root.path());
    for answer in ["", "q\n", "invalid\n"] {
        let output = clear(root.path(), &[], answer);
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Cancelled"));
        assert!(root.path().join(".wtflow/index/typescript.scip").exists());
    }
}
#[cfg(unix)]
#[test]
fn clear_never_follows_symlinks_outside_the_store() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("keep.txt"), "keep").unwrap();
    symlink(outside.path(), root.path().join(".wtflow")).unwrap();
    assert_eq!(clear(root.path(), &[], "").status.code(), Some(2));
    fs::remove_file(root.path().join(".wtflow")).unwrap();
    project(root.path());
    symlink(outside.path(), root.path().join(".wtflow/external")).unwrap();
    assert!(clear(root.path(), &[], "\n").status.success());
    assert!(outside.path().join("keep.txt").exists());
    assert!(root
        .path()
        .join(".wtflow/external")
        .symlink_metadata()
        .is_err());
}
