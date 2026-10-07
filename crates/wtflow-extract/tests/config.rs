use std::fs;
use wtflow_extract::config::RepositoryConfig;

#[test]
fn config_under_store_keeps_project_relative_paths_and_nearest_root() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wtflow/flows")).unwrap();
    fs::create_dir_all(root.path().join("src/nested")).unwrap();
    fs::write(root.path().join("src/nested/app.ts"), "function f() {}").unwrap();
    fs::write(
        root.path().join(".wtflow/config.yaml"),
        "modules: [{path: src, owner: new}]\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".wtflow.yaml"),
        "modules: [{path: src, owner: legacy}]\n",
    )
    .unwrap();
    for path in [
        "src/nested/app.ts",
        "src/nested",
        ".wtflow/flows",
        ".wtflow/config.yaml",
    ] {
        let config = RepositoryConfig::discover(&root.path().join(path)).unwrap();
        assert_eq!(config.root, root.path().canonicalize().unwrap());
        assert_eq!(config.owner("src/nested/app.ts"), "new");
        assert!(config.path.ends_with(".wtflow/config.yaml"));
    }
    fs::write(root.path().join("src/.wtflow.yaml"), "collapse: false\n").unwrap();
    let config = RepositoryConfig::discover(&root.path().join("src/nested")).unwrap();
    assert_eq!(config.root, root.path().join("src").canonicalize().unwrap());
    assert!(!config.config.collapse);
}

#[test]
fn invalid_current_config_does_not_fall_back_to_legacy_or_defaults() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".wtflow")).unwrap();
    fs::write(root.path().join(".wtflow/config.yaml"), "unknown: key\n").unwrap();
    fs::write(root.path().join(".wtflow.yaml"), "collapse: true\n").unwrap();
    let err = RepositoryConfig::discover(root.path()).err().unwrap();
    assert!(format!("{err:#}").contains(".wtflow/config.yaml"));
    fs::remove_file(root.path().join(".wtflow/config.yaml")).unwrap();
    assert!(
        RepositoryConfig::discover(root.path())
            .unwrap()
            .config
            .collapse
    );
}
