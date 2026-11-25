use appchecker::persist::Store;

#[test]
fn store_creates_missing_parent_directories() {
    let root = std::env::temp_dir().join(format!(
        "appchecker-store-test-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let db_path = root.join("nested/data/appchecker.db");

    let store = Store::open(&db_path).expect("store should create its parent directories");
    assert!(db_path.exists());

    store.shutdown();
    std::fs::remove_dir_all(root).ok();
}
