use std::{
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};
use wtflow_extract::config::RepositoryConfig;

fn init(root: &Path, args: &[&str], answers: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wtflow"))
        .current_dir(root)
        .env("PATH", "") // Setup must not depend on language tools or run indexers.
        .arg("init")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    command
        .stdin
        .take()
        .unwrap()
        .write_all(answers.as_bytes())
        .unwrap();
    command.wait_with_output().unwrap()
}

fn success(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn current_directory_uses_detected_language_and_defaults() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("tsconfig.json"), "{}").unwrap();
    let output = init(root.path(), &[], "\n\n\n");
    let transcript = success(&output);
    assert!(transcript.contains("[ts]"));
    assert!(!transcript.contains("Python project name"));
    let config = RepositoryConfig::load(root.path()).unwrap();
    assert_eq!(
        config.owner("src/service.ts"),
        root.path().file_name().unwrap().to_str().unwrap()
    );
    assert!(config.config.collapse);
    let ts = config.config.index.typescript.unwrap();
    assert!(ts.enabled);
    assert_eq!(ts.args, ["--infer-tsconfig"]);
    assert!(config.config.index.java.is_none());
    assert!(config.config.index.python.is_none());
    assert!(!root.path().join(".wtflow/index").exists());
}

#[test]
fn explicit_directories_get_deterministic_multilanguage_config_and_folder_owners() {
    let root = tempfile::tempdir().unwrap();
    let answers =
        "python,java,typescript,ts\nSales: # team\ndocument-ai\nyes\n./src//payments/\npayments\nno\n";
    for dir in ["first project", "second project"] {
        success(&init(root.path(), &["--dir", dir], answers));
        let config = RepositoryConfig::load(&root.path().join(dir)).unwrap();
        assert_eq!(config.owner("src/order.ts"), "Sales: # team");
        assert_eq!(config.owner("src/payments/pay.ts"), "payments");
        assert_eq!(
            config.config.index.python.unwrap().project_name.as_deref(),
            Some("document-ai")
        );
        assert!(config.config.index.java.unwrap().enabled);
        assert!(config.config.index.typescript.unwrap().enabled);
    }
    assert_eq!(
        std::fs::read(root.path().join("first project/.wtflow/config.yaml")).unwrap(),
        std::fs::read(root.path().join("second project/.wtflow/config.yaml")).unwrap()
    );
    assert!(!root.path().join(".wtflow.yaml").exists());
}

#[test]
fn invalid_answers_are_reprompted_before_any_file_is_written() {
    let root = tempfile::tempdir().unwrap();
    let output = init(
        root.path(),
        &[],
        "\nrust\njava\nteam\nmaybe\ny\n../outside\n/absolute\n.\nsrc\ncomponent\nn\n",
    );
    let transcript = success(&output);
    assert!(transcript.contains("Please enter a value"));
    assert!(transcript.contains("Choose ts, java, or py"));
    assert!(transcript.contains("Please answer yes or no"));
    assert!(transcript.contains("Use a relative path"));
    assert!(transcript.contains("already has an owner"));
    let config = RepositoryConfig::load(root.path()).unwrap();
    assert_eq!(config.owner("src/Service.java"), "component");
    assert_eq!(config.config.modules.len(), 2);
}

#[test]
fn interrupted_input_and_existing_configs_never_write() {
    let root = tempfile::tempdir().unwrap();
    let output = init(root.path(), &["--dir", "unfinished"], "ts\nteam\n");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("input ended"));
    assert!(!root.path().join("unfinished").exists());

    std::fs::create_dir(root.path().join(".wtflow")).unwrap();
    let path = root.path().join(".wtflow/config.yaml");
    std::fs::write(&path, "# keep this file\ncollapse: false\n").unwrap();
    let output = init(root.path(), &[], "");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("already exists"));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# keep this file\ncollapse: false\n"
    );
    assert!(output.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn dangling_config_symlink_is_preserved() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(".wtflow.yaml");
    std::os::unix::fs::symlink("missing.yaml", &path).unwrap();
    let output = init(root.path(), &[], "");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(std::fs::read_link(path).unwrap(), Path::new("missing.yaml"));
    assert!(!root.path().join("missing.yaml").exists());
}

#[test]
fn init_moves_legacy_config_without_changing_content_or_running_questions() {
    let root = tempfile::tempdir().unwrap();
    let legacy = root.path().join(".wtflow.yaml");
    let text = "# keep my comments\nmodules:\n  - path: src\n    owner: billing\ncollapse: false\n";
    std::fs::write(&legacy, text).unwrap();
    std::fs::create_dir_all(root.path().join(".wtflow/flows")).unwrap();
    std::fs::write(root.path().join(".wtflow/flows/keep.txt"), "keep").unwrap();
    let output = init(root.path(), &[], "");
    assert!(success(&output).contains("Moved"));
    assert_eq!(
        std::fs::read_to_string(root.path().join(".wtflow/config.yaml")).unwrap(),
        text
    );
    assert!(!legacy.exists());
    assert!(root.path().join(".wtflow/flows/keep.txt").exists());
    assert_eq!(
        RepositoryConfig::discover(&root.path().join(".wtflow/flows"))
            .unwrap()
            .root,
        root.path().canonicalize().unwrap()
    );
}

#[test]
fn migration_preserves_invalid_legacy_and_refuses_conflicting_config() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join(".wtflow.yaml");
    std::fs::write(&old, "[invalid").unwrap();
    assert_eq!(init(root.path(), &[], "").status.code(), Some(2));
    assert_eq!(std::fs::read_to_string(&old).unwrap(), "[invalid");
    assert!(!root.path().join(".wtflow").exists());
    std::fs::create_dir(root.path().join(".wtflow")).unwrap();
    let new = root.path().join(".wtflow/config.yaml");
    std::fs::write(&new, "collapse: true\n").unwrap();
    assert_eq!(init(root.path(), &[], "").status.code(), Some(2));
    assert_eq!(std::fs::read_to_string(&new).unwrap(), "collapse: true\n");
    assert_eq!(std::fs::read_to_string(&old).unwrap(), "[invalid");
}
