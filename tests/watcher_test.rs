use appchecker::watcher::FsWatcher;
use std::path::Path;

#[test]
fn only_root_manifests_trigger_dependency_scans() {
    let root = Path::new("/tmp/project");
    assert!(FsWatcher::is_manifest_path(
        &root.join("package.json"),
        root
    ));
    assert!(!FsWatcher::is_manifest_path(
        &root.join("node_modules/dep/package.json"),
        root
    ));
    assert!(!FsWatcher::is_manifest_path(
        &root.join("nested/package.json"),
        root
    ));
    assert!(!FsWatcher::is_manifest_path(
        &root.join("vendor/composer.json"),
        root
    ));
}
