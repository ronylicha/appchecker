use appchecker::core::{DepInfo, Ecosystem, UpdateSeverity};
use appchecker::deps::{DepsParser, RegistryClient};
use chrono::Utc;

fn fixture_root() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("appchecker-privacy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn skips_local_url_and_workspace_dependencies() {
    let root = fixture_root();
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"normal":"1.0.0","local":"file:../local","workspace":"workspace:*","git":"git+https://example/repo.git","shorthand":"owner/repo","alias":"npm:foo","absolute":"/home/dev/pkg"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[dependencies]\nnormal = \"1.0.0\"\nlocal = { path = \"../local\" }\ngit = { git = \"https://example/repo.git\" }\nworkspace = { workspace = true }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\ndependencies = [\n  \"requests[security]>=2.31.0; python_version >= '3.11'\",\n  \"private @ git+https://user:token@example/repo.git\",\n  \"./local.whl\",\n  \"/home/dev/pkg-1.0.whl\",\n  \"wheels/internal.whl\",\n]\n",
    )
    .unwrap();
    std::fs::write(
        root.join("Gemfile"),
        "gem \"normal\", \"~> 1.0\"\ngem \"private\", path: \"../private\"\ngem \"git_source\", git: \"https://example/repo.git\"\npath \"../local\" do\n  gem \"blocked\"\nend\n",
    )
    .unwrap();
    std::fs::write(
        root.join("go.mod"),
        "require internal/foo v0.0.0\nreplace internal/foo => ../foo\nreplace (\n  other.com/mod v0.0.0 => git.corp.internal/a v1.0.0\n)\n",
    )
    .unwrap();

    let dependencies = DepsParser::detect_and_parse(&root);

    assert_eq!(dependencies.len(), 4, "{dependencies:?}");
    assert!(
        dependencies
            .iter()
            .any(|dependency| dependency.name == "normal")
    );
    assert!(dependencies.iter().any(|dependency| {
        dependency.name == "requests" && dependency.ecosystem == Ecosystem::Pip
    }));
    assert!(
        dependencies
            .iter()
            .all(|dependency| dependency.name != "private")
    );
    assert!(
        dependencies
            .iter()
            .all(|dependency| dependency.name != "local")
    );
    assert!(
        dependencies
            .iter()
            .all(|dependency| dependency.name != "workspace")
    );
    assert!(
        dependencies
            .iter()
            .all(|dependency| dependency.name != "git")
    );
    assert!(
        dependencies
            .iter()
            .all(|dependency| dependency.name != "shorthand")
    );
    assert!(
        dependencies
            .iter()
            .all(|dependency| dependency.name != "alias")
    );
    assert!(
        dependencies
            .iter()
            .all(|dependency| dependency.name != "absolute")
    );
    assert!(
        dependencies
            .iter()
            .all(|dependency| dependency.name != "git_source")
    );
    assert!(dependencies.iter().any(|dependency| {
        dependency.name == "normal" && dependency.ecosystem == Ecosystem::Gem
    }));
    assert!(
        dependencies
            .iter()
            .all(|dependency| dependency.name != "internal/foo")
    );
    assert!(
        dependencies
            .iter()
            .all(|dependency| dependency.name != "other.com/mod")
    );
    assert!(
        dependencies
            .iter()
            .all(|dependency| dependency.name != "blocked")
    );
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn registry_rejects_non_public_dependency_references_before_http() {
    let dependency = DepInfo {
        name: "pkg @ file:///home/private/package.whl".into(),
        ecosystem: Ecosystem::Pip,
        current_version: "1.0.0".into(),
        latest_version: None,
        update_available: false,
        severity: UpdateSeverity::Info,
        dev_dependency: false,
        manifest_name: None,
        checked_at: Utc::now(),
    };
    let client = RegistryClient::new(Default::default(), false);

    let (_, error) = client.check_update_detailed(&dependency).await;

    let error = error.expect("direct URL must be rejected");
    assert!(error.message.contains("not a public registry dependency"));
}

#[test]
fn scoped_gem_sources_do_not_change_the_top_level_source() {
    let private_root = fixture_root();
    std::fs::write(
        private_root.join("Gemfile"),
        "source \"https://gems.corp.internal\"\nsource \"https://rubygems.org\" do\n  gem \"public-scoped\"\nend\ngem \"corp-secret\", \"~> 1.0\"\n",
    )
    .unwrap();
    let private = DepsParser::detect_and_parse(&private_root);
    assert!(
        private
            .iter()
            .any(|dependency| dependency.name == "public-scoped")
    );
    assert!(
        private
            .iter()
            .all(|dependency| dependency.name != "corp-secret")
    );

    let public_root = fixture_root();
    std::fs::write(
        public_root.join("Gemfile"),
        "source \"https://rubygems.org\"\nsource \"https://gems.corp.internal\" do\n  gem \"corp-secret\"\nend\ngem \"rails\", \"~> 7.1\"\n",
    )
    .unwrap();
    let public = DepsParser::detect_and_parse(&public_root);
    assert!(
        public
            .iter()
            .all(|dependency| dependency.name != "corp-secret")
    );
    assert!(public.iter().any(|dependency| dependency.name == "rails"));
    std::fs::remove_dir_all(private_root).ok();
    std::fs::remove_dir_all(public_root).ok();
}

#[test]
fn skips_composer_object_repositories_that_disable_or_replace_packagist() {
    for repositories in [
        r#"{"packagist.org":false}"#,
        r#"{"corp":{"type":"vcs","url":"https://git.corp.internal/packages"}}"#,
    ] {
        let root = fixture_root();
        std::fs::write(
            root.join("composer.json"),
            format!(r#"{{"repositories":{repositories},"require":{{"corp/secret":"1.0.0"}}}}"#),
        )
        .unwrap();
        assert!(DepsParser::detect_and_parse(&root).is_empty());
        std::fs::remove_dir_all(root).ok();
    }
}

#[test]
fn skips_requirements_with_non_public_index_options() {
    for option in [
        "--extra-index-url https://pypi.corp.internal/simple",
        "--extra-index-url=https://pypi.org.corp.internal/simple",
        "--find-links ./wheels",
        "-f ./wheels",
        "-ihttps://pypi.corp.internal/simple",
        "-i https://pypi.corp.internal/simple",
    ] {
        let root = fixture_root();
        std::fs::write(
            root.join("requirements.txt"),
            format!("{option}\ninternal-package==1.0.0\n"),
        )
        .unwrap();
        assert!(
            DepsParser::detect_and_parse(&root).is_empty(),
            "private index was ignored: {option}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    let public_root = fixture_root();
    std::fs::write(
        public_root.join("requirements.txt"),
        "--index-url https://pypi.org/simple\nrequests==2.31.0\n",
    )
    .unwrap();
    let public = DepsParser::detect_and_parse(&public_root);
    assert_eq!(public.len(), 1);
    assert_eq!(public[0].name, "requests");
    std::fs::remove_dir_all(public_root).ok();
}

#[test]
fn skips_combined_and_commented_private_pip_options() {
    for option in [
        "--no-index --find-links ./wheels",
        "--no-index # offline",
        "--trusted-host pypi.corp.internal --index-url https://pypi.corp.internal/simple",
        "--index-url https://pypi.org/simple --extra-index-url https://pypi.corp.internal/simple",
        "-f./wheels",
        "--find-links=./wheels",
    ] {
        let root = fixture_root();
        std::fs::write(
            root.join("requirements.txt"),
            format!("{option}\ncorp-internal-sdk==1.0.0\n"),
        )
        .unwrap();
        assert!(
            DepsParser::detect_and_parse(&root).is_empty(),
            "private options bypassed: {option}"
        );
        std::fs::remove_dir_all(root).ok();
    }
}

#[test]
fn skips_non_public_pyproject_package_indexes() {
    for index in [
        "[tool.uv]\nindex-url = \"https://pypi.corp.internal/simple\"",
        "[tool.uv]\nextra-index-url = [\"https://pypi.corp.internal/simple\"]",
        "[[tool.uv.index]]\nurl = \"https://pypi.corp.internal/simple\"",
        "[[tool.poetry.source]]\nname = \"corp\"\nurl = \"https://pypi.corp.internal/simple\"",
        "[[tool.pdm.source]]\nname = \"corp\"\nurl = \"https://pypi.corp.internal/simple\"",
    ] {
        let root = fixture_root();
        std::fs::write(
            root.join("pyproject.toml"),
            format!("[project]\ndependencies = [\"corp-internal-sdk>=1.0\"]\n{index}\n"),
        )
        .unwrap();
        assert!(
            DepsParser::detect_and_parse(&root).is_empty(),
            "private pyproject index bypassed: {index}"
        );
        std::fs::remove_dir_all(root).ok();
    }
}

#[test]
fn validates_ruby_source_hosts_and_nested_blocks() {
    for (gemfile, forbidden) in [
        (
            "source \"https://rubygems.org.corp.internal\"\ngem \"corp-secret\"\n",
            "corp-secret",
        ),
        (
            "source \"https://artifactory.corp.internal/api/gems/rubygems.org\"\ngem \"corp-secret\"\n",
            "corp-secret",
        ),
        (
            "source \"https://gems.corp.internal\" do\n  if ENV[\"CI\"]\n    gem \"conditional\"\n  end\n  gem \"corp-secret\"\nend\n",
            "corp-secret",
        ),
        (
            "source \"https://gems.corp.internal\" do\n  unless ENV[\"PUBLIC\"]\n    gem \"conditional\"\n  end\n  gem \"corp-secret\"\nend\n",
            "corp-secret",
        ),
    ] {
        let root = fixture_root();
        std::fs::write(root.join("Gemfile"), gemfile).unwrap();
        let dependencies = DepsParser::detect_and_parse(&root);
        assert!(
            dependencies
                .iter()
                .all(|dependency| dependency.name != forbidden),
            "private gem leaked: {gemfile}"
        );
        std::fs::remove_dir_all(root).ok();
    }
}

#[test]
fn skips_replaced_and_patched_cargo_dependencies_including_aliases() {
    for manifest in [
        "[dependencies]\ncorp-internal = \"0.1.0\"\n[replace]\n\"corp-internal:0.1.0\" = { path = \"../corp-internal\" }\n",
        "[dependencies]\ncorp-internal = \"0.1.0\"\n[replace]\n\"https://github.com/rust-lang/crates.io-index#corp-internal:0.1.0\" = { path = \"../corp-internal\" }\n",
        "[dependencies]\ncorp = { package = \"corp-internal\", version = \"0.1.0\" }\n[patch.crates-io]\ncorp-internal = { path = \"../corp-internal\" }\n",
        "[dependencies]\ncorp = { package = \"corp-internal\", version = \"0.1.0\" }\n[patch.crates-io]\nrenamed = { package = \"corp-internal\", path = \"../corp-internal\" }\n",
    ] {
        let root = fixture_root();
        std::fs::write(root.join("Cargo.toml"), manifest).unwrap();
        assert!(
            DepsParser::detect_and_parse(&root).is_empty(),
            "private Cargo replacement bypassed: {manifest}"
        );
        std::fs::remove_dir_all(root).ok();
    }
}

#[test]
fn respects_private_npmrc_registries() {
    let root = fixture_root();
    std::fs::write(root.join(".npmrc"), "registry=https://npm.corp.internal/\n").unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"public-package":"1.0.0","@corp/internal-auth":"1.0.0"}}"#,
    )
    .unwrap();
    assert!(DepsParser::detect_and_parse(&root).is_empty());

    std::fs::write(
        root.join(".npmrc"),
        "@corp:registry=https://npm.corp.internal/\n",
    )
    .unwrap();
    let dependencies = DepsParser::detect_and_parse(&root);
    assert_eq!(dependencies.len(), 1);
    assert_eq!(dependencies[0].name, "public-package");
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"local-catalog":"catalog:internal","@corp/internal-auth":"1.0.0"}}"#,
    )
    .unwrap();
    assert!(DepsParser::detect_and_parse(&root).is_empty());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn skips_dependencies_overridden_by_non_public_uv_sources() {
    for source in [
        "corp-internal = { path = \"../corp-internal\" }",
        "corp-internal = { workspace = true }",
        "corp-internal = { git = \"https://git.corp.internal/corp-internal\" }",
        "corp-internal = { url = \"https://files.corp.internal/corp-internal.whl\" }",
        "corp-internal = { index = \"corp\" }",
    ] {
        let root = fixture_root();
        std::fs::write(
            root.join("pyproject.toml"),
            format!("[project]\ndependencies = [\"corp-internal>=1.0.0\", \"requests>=2.31.0\"]\n[tool.uv.sources]\n{source}\n"),
        ).unwrap();
        let dependencies = DepsParser::detect_and_parse(&root);
        assert_eq!(dependencies.len(), 1, "uv source was ignored: {source}");
        assert_eq!(dependencies[0].name, "requests");
        std::fs::remove_dir_all(root).ok();
    }
}

#[test]
fn fails_closed_for_multiline_gems_and_multiple_top_level_sources() {
    let root = fixture_root();
    std::fs::write(
        root.join("Gemfile"),
        "source \"https://rubygems.org\"\ngem \"corp_private_engine\",\n  path: \"engines/corp_private_engine\"\n",
    ).unwrap();
    assert!(DepsParser::detect_and_parse(&root).is_empty());

    std::fs::write(
        root.join("Gemfile"),
        "source \"https://gems.corp.internal\"\nsource \"https://rubygems.org\"\ngem \"corp-secret\"\n",
    ).unwrap();
    assert!(DepsParser::detect_and_parse(&root).is_empty());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn skips_uv_array_sources_and_private_uv_settings() {
    let root = fixture_root();
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\ndependencies = [\"corp-internal>=1.0\"]\n[tool.uv.sources]\ncorp-internal = [{ path = \"../corp\", marker = \"python_version >= '3.11'\" }]\n",
    ).unwrap();
    assert!(DepsParser::detect_and_parse(&root).is_empty());
    for settings in [
        "[tool.uv]\nno-index = true",
        "[tool.uv]\nfind-links = [\"./wheels\"]",
        "[tool.uv]\ndefault-index = \"https://pypi.corp.internal/simple\"",
        "[tool.uv.pip]\nindex-url = \"https://pypi.corp.internal/simple\"",
        "[tool.uv.pip]\nextra-index-url = [\"https://pypi.corp.internal/simple\"]",
        "[tool.uv.pip]\nno-index = true",
    ] {
        std::fs::write(
            root.join("pyproject.toml"),
            format!("[project]\ndependencies = [\"corp-internal>=1.0\"]\n{settings}\n"),
        )
        .unwrap();
        assert!(
            DepsParser::detect_and_parse(&root).is_empty(),
            "uv settings were ignored: {settings}"
        );
    }
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn skips_cargo_source_replacement_configuration() {
    for config in [
        "[source.crates-io]\nreplace-with = \"corp-registry\"",
        "[source.crates-io]\nreplace-with = \"vendored-sources\"",
        "not valid toml = [",
    ] {
        let root = fixture_root();
        std::fs::create_dir_all(root.join(".cargo")).unwrap();
        std::fs::write(root.join(".cargo/config.toml"), config).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[dependencies]\nserde = \"1.0.0\"\n",
        )
        .unwrap();
        assert!(DepsParser::detect_and_parse(&root).is_empty());
        std::fs::remove_dir_all(root).ok();
    }
}

#[test]
fn skips_ruby_custom_git_sources_and_unknown_options() {
    for declaration in [
        "gem \"corp-secret\", corp: \"org/repo\"",
        "gem \"corp-secret\", gitlab: \"org/repo\"",
        "gem \"corp-secret\", gist: \"abc123\"",
        "gem \"corp-secret\", branch: \"main\"",
        "gem \"corp-secret\", ref: \"abc123\"",
        "gem \"corp-secret\", tag: \"v1.0.0\"",
    ] {
        let root = fixture_root();
        std::fs::write(
            root.join("Gemfile"),
            format!("git_source(:corp)\n{declaration}\n"),
        )
        .unwrap();
        assert!(
            DepsParser::detect_and_parse(&root).is_empty(),
            "private Ruby option leaked: {declaration}"
        );
        std::fs::remove_dir_all(root).ok();
    }
}

#[test]
fn reports_when_non_public_dependencies_are_skipped() {
    let root = fixture_root();
    std::fs::write(
        root.join(".npmrc"),
        "@corp:registry=https://npm.corp.internal/\n",
    )
    .unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"public-package":"1.0.0","@corp/internal-auth":"1.0.0"}}"#,
    )
    .unwrap();
    let report = DepsParser::detect_and_report(&root);
    assert_eq!(report.dependencies.len(), 1);
    assert!(report.non_public_skipped);

    std::fs::remove_file(root.join(".npmrc")).unwrap();
    let public_report = DepsParser::detect_and_report(&root);
    assert!(!public_report.non_public_skipped);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn requirements_local_urls_editables_and_includes_are_marked_skipped() {
    for requirements in [
        "corp-sdk @ git+https://git.corp.internal/corp-sdk.git\n",
        "./wheels/corp-1.0.whl\n",
        "-e ./local-pkg\n",
        "--editable ./local-pkg\n",
        "-r requirements/base.txt\ncorp-internal-sdk==1.0.0\n",
        "--constraint constraints.txt\ncorp-internal-sdk==1.0.0\n",
    ] {
        let root = fixture_root();
        std::fs::write(root.join("requirements.txt"), requirements).unwrap();
        let report = DepsParser::detect_and_report(&root);
        assert!(
            report.dependencies.is_empty(),
            "private requirement parsed: {requirements}"
        );
        assert!(
            report.non_public_skipped,
            "private requirement looked healthy: {requirements}"
        );
        std::fs::remove_dir_all(root).ok();
    }
}

#[test]
fn respects_goprivate_environment_in_a_child_process() {
    if std::env::var_os("APPCHECKER_GO_PRIVACY_CHILD").is_some() {
        let root = fixture_root();
        std::fs::write(
            root.join("go.mod"),
            "require git.corp.internal/team/service v1.0.0\n",
        )
        .unwrap();
        let report = DepsParser::detect_and_report(&root);
        assert!(report.dependencies.is_empty());
        assert!(report.non_public_skipped);
        std::fs::remove_dir_all(root).ok();
        return;
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "respects_goprivate_environment_in_a_child_process",
            "--exact",
        ])
        .env("APPCHECKER_GO_PRIVACY_CHILD", "1")
        .env("GOPRIVATE", "git.corp.internal,github.com/private-org/*")
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn skips_go_workspaces_and_composer_packagist_overrides() {
    let root = fixture_root();
    std::fs::write(root.join("go.work"), "go 1.24\nuse ./local-module\n").unwrap();
    std::fs::write(
        root.join("go.mod"),
        "require example.com/public-module v1.0.0\n",
    )
    .unwrap();
    let go_report = DepsParser::detect_and_report(&root);
    assert!(go_report.dependencies.is_empty());
    assert!(go_report.non_public_skipped);

    std::fs::remove_file(root.join("go.work")).unwrap();
    std::fs::remove_file(root.join("go.mod")).unwrap();
    std::fs::write(
        root.join("composer.json"),
        r#"{"repositories":{"packagist.org":{"type":"composer","url":"https://packagist.corp.internal"}},"require":{"corp/secret":"1.0.0"}}"#,
    ).unwrap();
    let composer_report = DepsParser::detect_and_report(&root);
    assert!(composer_report.dependencies.is_empty());
    assert!(composer_report.non_public_skipped);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn uv_private_overrides_also_apply_to_requirements_fallback_and_updates() {
    let root = fixture_root();
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\ndependencies = [\"corp-internal>=1.0\"]\n[tool.uv.sources]\ncorp-internal = { path = \"../corp-internal\" }\n",
    ).unwrap();
    std::fs::write(root.join("requirements.txt"), "corp-internal==1.0.0\n").unwrap();
    let report = DepsParser::detect_and_report(&root);
    assert!(report.dependencies.is_empty());
    assert!(report.non_public_skipped);

    let dependency = DepInfo {
        name: "corp-internal".into(),
        ecosystem: Ecosystem::Pip,
        current_version: "1.0.0".into(),
        latest_version: Some("2.0.0".into()),
        update_available: true,
        severity: UpdateSeverity::Major,
        dev_dependency: false,
        manifest_name: None,
        checked_at: Utc::now(),
    };
    let plan = appchecker::deps::UpdateManager::plan(&root, &dependency).unwrap();
    let appchecker::deps::UpdateAction::Manifest { path, .. } = plan.action else {
        panic!("expected manifest update");
    };
    assert!(
        path.ends_with("requirements.txt"),
        "update was misrouted to pyproject.toml"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn normalizes_cargo_replacement_names_and_accepts_ruby_array_options() {
    let root = fixture_root();
    std::fs::write(
        root.join("Cargo.toml"),
        "[dependencies]\ncorp_internal = \"0.1.0\"\n[patch.crates-io]\ncorp-internal = { path = \"../corp-internal\" }\n",
    ).unwrap();
    let cargo_report = DepsParser::detect_and_report(&root);
    assert!(cargo_report.dependencies.is_empty());
    assert!(cargo_report.non_public_skipped);

    std::fs::remove_file(root.join("Cargo.toml")).unwrap();
    std::fs::write(root.join("Gemfile"), "gem \"rspec-rails\", \"~> 6.0\", group: [:development, :test]\ngem \"tzinfo-data\", platforms: [:mingw, :mswin]\n").unwrap();
    let gem_report = DepsParser::detect_and_report(&root);
    assert_eq!(gem_report.dependencies.len(), 2);
    assert!(
        gem_report
            .dependencies
            .iter()
            .any(|dependency| dependency.name == "rspec-rails" && dependency.dev_dependency)
    );
    assert!(!gem_report.non_public_skipped);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn reads_go_env_files_and_unspaced_blocks() {
    if std::env::var_os("APPCHECKER_GO_ENV_CHILD").is_some() {
        let root = fixture_root();
        std::fs::write(root.join("go.mod"), "require (\n  git.corp.internal/team/service v1.0.0\n) // comment\nreplace(\n  git.corp.internal/team/service => ../service\n)\n").unwrap();
        let report = DepsParser::detect_and_report(&root);
        assert!(report.dependencies.is_empty());
        assert!(report.non_public_skipped);
        std::fs::remove_dir_all(root).ok();
        return;
    }
    let root = fixture_root();
    std::fs::create_dir_all(root.join("go")).unwrap();
    std::fs::write(
        root.join("go/env"),
        "GOPRIVATE=git.corp.internal\nGOPROXY=https://proxy.golang.org,direct\n",
    )
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["reads_go_env_files_and_unspaced_blocks", "--exact"])
        .env("APPCHECKER_GO_ENV_CHILD", "1")
        .env("XDG_CONFIG_HOME", &root)
        .env_remove("GOPRIVATE")
        .env_remove("GONOPROXY")
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn respects_yarn_and_global_npm_registry_configuration() {
    if std::env::var_os("APPCHECKER_NPM_ENV_CHILD").is_some() {
        let root = fixture_root();
        std::fs::write(
            root.join(".yarnrc.yml"),
            "npmScopes:\n  corp:\n    npmRegistryServer: \"https://npm.corp.internal\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@corp/internal-auth":"1.0.0"}}"#,
        )
        .unwrap();
        let report = DepsParser::detect_and_report(&root);
        assert!(report.dependencies.is_empty());
        assert!(report.non_public_skipped);

        std::fs::remove_file(root.join(".yarnrc.yml")).unwrap();
        std::fs::write(root.join(".npmrc"), [0xff, 0xfe]).unwrap();
        let unreadable = DepsParser::detect_and_report(&root);
        assert!(unreadable.dependencies.is_empty());
        assert!(unreadable.non_public_skipped);
        std::fs::remove_dir_all(root).ok();
        return;
    }
    let root = fixture_root();
    std::fs::write(
        root.join(".npmrc"),
        "@corp:registry=https://npm.corp.internal/\n",
    )
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "respects_yarn_and_global_npm_registry_configuration",
            "--exact",
        ])
        .env("APPCHECKER_NPM_ENV_CHILD", "1")
        .env("HOME", &root)
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn checks_uv_toml_and_fails_closed_for_invalid_pyproject() {
    let root = fixture_root();
    std::fs::write(
        root.join("uv.toml"),
        "index-url = \"https://pypi.corp.internal/simple\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\ndependencies = [\"corp-internal>=1.0\"]\n",
    )
    .unwrap();
    let report = DepsParser::detect_and_report(&root);
    assert!(report.dependencies.is_empty());
    assert!(report.non_public_skipped);

    std::fs::remove_file(root.join("uv.toml")).unwrap();
    std::fs::write(root.join("pyproject.toml"), "not valid toml = [").unwrap();
    std::fs::write(root.join("requirements.txt"), "corp-internal==1.0.0\n").unwrap();
    let invalid = DepsParser::detect_and_report(&root);
    assert!(invalid.dependencies.is_empty());
    assert!(invalid.non_public_skipped);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn standard_go_proxy_fallback_does_not_disable_public_checks() {
    if std::env::var_os("APPCHECKER_GO_PROXY_CHILD").is_some() {
        let root = fixture_root();
        std::fs::write(
            root.join("go.mod"),
            "require github.com/gin-gonic/gin v1.10.0\n",
        )
        .unwrap();
        let report = DepsParser::detect_and_report(&root);
        assert_eq!(report.dependencies.len(), 1);
        assert!(!report.non_public_skipped);
        std::fs::remove_dir_all(root).ok();
        return;
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "standard_go_proxy_fallback_does_not_disable_public_checks",
            "--exact",
        ])
        .env("APPCHECKER_GO_PROXY_CHILD", "1")
        .env("GOPROXY", "https://proxy.golang.org|direct")
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn skips_custom_ruby_source_blocks() {
    let root = fixture_root();
    std::fs::write(
        root.join("Gemfile"),
        "git_source(:corp) { |repo| \"https://git.corp.internal/#{repo}.git\" }\ncorp \"team/repo\" do\n  gem \"corp-secret\", \"~> 1.0\"\nend\n",
    ).unwrap();
    let report = DepsParser::detect_and_report(&root);
    assert!(report.dependencies.is_empty());
    assert!(report.non_public_skipped);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn merges_global_and_project_npmrc_and_environment() {
    if std::env::var_os("APPCHECKER_NPM_MERGE_CHILD").is_some() {
        let root = fixture_root();
        std::fs::write(root.join(".npmrc"), "legacy-peer-deps=true\n").unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@corp/internal-auth":"1.0.0"}}"#,
        )
        .unwrap();
        let report = DepsParser::detect_and_report(&root);
        assert!(report.dependencies.is_empty());
        assert!(report.non_public_skipped);
        std::fs::remove_dir_all(root).ok();
        return;
    }
    let root = fixture_root();
    std::fs::write(
        root.join(".npmrc"),
        "@corp:registry=https://npm.corp.internal/\n",
    )
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["merges_global_and_project_npmrc_and_environment", "--exact"])
        .env("APPCHECKER_NPM_MERGE_CHILD", "1")
        .env("HOME", &root)
        .env("NPM_CONFIG_REGISTRY", "https://registry.npmjs.org/")
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn respects_cargo_home_and_ancestor_source_configuration() {
    if std::env::var_os("APPCHECKER_CARGO_HOME_CHILD").is_some() {
        let root = fixture_root();
        let nested = root.join("parent/project");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("Cargo.toml"),
            "[dependencies]\ncorp-internal = \"0.1.0\"\n",
        )
        .unwrap();
        let report = DepsParser::detect_and_report(&nested);
        assert!(report.dependencies.is_empty());
        assert!(report.non_public_skipped);
        std::fs::remove_dir_all(root).ok();
        return;
    }
    let root = fixture_root();
    std::fs::write(
        root.join("config.toml"),
        "[source.crates-io]\nreplace-with = \"corp-mirror\"\n",
    )
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "respects_cargo_home_and_ancestor_source_configuration",
            "--exact",
        ])
        .env("APPCHECKER_CARGO_HOME_CHILD", "1")
        .env("CARGO_HOME", &root)
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn respects_pip_uv_environment_and_user_configuration() {
    if std::env::var_os("APPCHECKER_PIP_ENV_CHILD").is_some() {
        let root = fixture_root();
        std::fs::write(root.join("requirements.txt"), "corp-internal-sdk==1.0.0\n").unwrap();
        let report = DepsParser::detect_and_report(&root);
        assert!(report.dependencies.is_empty());
        assert!(report.non_public_skipped);
        std::fs::remove_dir_all(root).ok();
        return;
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "respects_pip_uv_environment_and_user_configuration",
            "--exact",
        ])
        .env("APPCHECKER_PIP_ENV_CHILD", "1")
        .env("PIP_INDEX_URL", "https://pypi.corp.internal/simple")
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn respects_user_pip_and_uv_config_files() {
    if std::env::var_os("APPCHECKER_PIP_CONFIG_CHILD").is_some() {
        let root = fixture_root();
        std::fs::write(root.join("requirements.txt"), "corp-internal-sdk==1.0.0\n").unwrap();
        let report = DepsParser::detect_and_report(&root);
        assert!(report.dependencies.is_empty());
        assert!(report.non_public_skipped);
        std::fs::remove_dir_all(root).ok();
        return;
    }
    let root = fixture_root();
    std::fs::write(
        root.join("pip.conf"),
        "[global]\nindex-url = https://pypi.corp.internal/simple\n",
    )
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["respects_user_pip_and_uv_config_files", "--exact"])
        .env("APPCHECKER_PIP_CONFIG_CHILD", "1")
        .env("PIP_CONFIG_FILE", root.join("pip.conf"))
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn detects_parent_go_workspace_configuration() {
    let root = fixture_root();
    let project = root.join("workspace/service-a");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(root.join("workspace/go.work"), "use (\n  ./service-a\n)\n").unwrap();
    std::fs::write(
        project.join("go.mod"),
        "require corp.internal/internal-lib v0.0.0\n",
    )
    .unwrap();
    let report = DepsParser::detect_and_report(&project);
    assert!(report.dependencies.is_empty());
    assert!(report.non_public_skipped);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn respects_global_composer_repositories() {
    if std::env::var_os("APPCHECKER_COMPOSER_CHILD").is_some() {
        let root = fixture_root();
        std::fs::write(
            root.join("composer.json"),
            r#"{"require":{"acme/internal-sdk":"1.0.0"}}"#,
        )
        .unwrap();
        let report = DepsParser::detect_and_report(&root);
        assert!(report.dependencies.is_empty());
        assert!(report.non_public_skipped);
        std::fs::remove_dir_all(root).ok();
        return;
    }
    let root = fixture_root();
    std::fs::write(
        root.join("config.json"),
        r#"{"repositories":{"corp":{"type":"composer","url":"https://packagist.corp.internal"}}}"#,
    )
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["respects_global_composer_repositories", "--exact"])
        .env("APPCHECKER_COMPOSER_CHILD", "1")
        .env("COMPOSER_HOME", &root)
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn handles_pip_ini_variants_and_bundler_mirrors() {
    if std::env::var_os("APPCHECKER_PIP_INI_CHILD").is_some() {
        let root = fixture_root();
        std::fs::write(root.join("requirements.txt"), "corp-internal-sdk==1.0.0\n").unwrap();
        let report = DepsParser::detect_and_report(&root);
        assert!(report.dependencies.is_empty());
        assert!(report.non_public_skipped);
        std::fs::remove_dir_all(root).ok();
        return;
    }
    let root = fixture_root();
    std::fs::write(root.join("pip.conf"), "[global]\nno-index = True\n").unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["handles_pip_ini_variants_and_bundler_mirrors", "--exact"])
        .env("APPCHECKER_PIP_INI_CHILD", "1")
        .env("PIP_CONFIG_FILE", root.join("pip.conf"))
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn skips_private_bundler_mirror_configuration() {
    let root = fixture_root();
    std::fs::create_dir_all(root.join(".bundle")).unwrap();
    std::fs::write(
        root.join(".bundle/config"),
        "BUNDLE_MIRROR__HTTPS://RUBYGEMS.ORG: \"https://gems.corp.internal\"\n",
    )
    .unwrap();
    std::fs::write(root.join("Gemfile"), "gem \"corp-secret\", \"~> 1.0\"\n").unwrap();
    let report = DepsParser::detect_and_report(&root);
    assert!(report.dependencies.is_empty());
    assert!(report.non_public_skipped);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn skips_cargo_default_registry_overrides() {
    let root = fixture_root();
    std::fs::create_dir_all(root.join(".cargo")).unwrap();
    std::fs::write(
        root.join(".cargo/config.toml"),
        "[registry]\ndefault = \"corp\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[dependencies]\ninternal-auth = \"0.1.0\"\n",
    )
    .unwrap();
    let report = DepsParser::detect_and_report(&root);
    assert!(report.dependencies.is_empty());
    assert!(report.non_public_skipped);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn gowork_auto_searches_for_workspaces_without_disabling_public_checks() {
    if std::env::var_os("APPCHECKER_GOWORK_AUTO_CHILD").is_some() {
        let root = fixture_root();
        std::fs::write(
            root.join("go.mod"),
            "require github.com/gin-gonic/gin v1.10.0\n",
        )
        .unwrap();
        let report = DepsParser::detect_and_report(&root);
        assert_eq!(report.dependencies.len(), 1);
        assert!(!report.non_public_skipped);
        std::fs::remove_dir_all(root).ok();
        return;
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "gowork_auto_searches_for_workspaces_without_disabling_public_checks",
            "--exact",
        ])
        .env("APPCHECKER_GOWORK_AUTO_CHILD", "1")
        .env("GOWORK", "auto")
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn respects_npm_userconfig_yarn_env_and_ancestors() {
    if std::env::var_os("APPCHECKER_NPM_CONFIG_CHILD").is_some() {
        let root = fixture_root();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@corp/internal-auth":"1.0.0"}}"#,
        )
        .unwrap();
        let report = DepsParser::detect_and_report(&root);
        assert!(report.dependencies.is_empty());
        assert!(report.non_public_skipped);
        std::fs::remove_dir_all(root).ok();
        return;
    }
    let root = fixture_root();
    std::fs::write(
        root.join("corp.npmrc"),
        "@corp:registry=https://npm.corp.internal/\n",
    )
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["respects_npm_userconfig_yarn_env_and_ancestors", "--exact"])
        .env("APPCHECKER_NPM_CONFIG_CHILD", "1")
        .env("NPM_CONFIG_USERCONFIG", root.join("corp.npmrc"))
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn fails_closed_for_invalid_go_env_and_tab_replacements() {
    let root = fixture_root();
    std::fs::write(root.join("go.env"), [0xff, 0xfe]).unwrap();
    std::fs::write(
        root.join("go.mod"),
        "require corp.internal/mod v1.0.0\nreplace\tcorp.internal/mod => ../mod\n",
    )
    .unwrap();
    std::fs::write(root.join("go.work"), "go 1.24\n").unwrap();
    let report = DepsParser::detect_and_report(&root);
    assert!(report.dependencies.is_empty());
    assert!(report.non_public_skipped);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn handles_case_insensitive_pip_keys_and_ancestor_uv_config() {
    if std::env::var_os("APPCHECKER_PIP_CASE_CHILD").is_some() {
        let root = fixture_root();
        std::fs::write(root.join("requirements.txt"), "corp-internal-sdk==1.0.0\n").unwrap();
        let report = DepsParser::detect_and_report(&root);
        assert!(report.dependencies.is_empty());
        assert!(report.non_public_skipped);
        std::fs::remove_dir_all(root).ok();
        return;
    }
    let root = fixture_root();
    std::fs::write(
        root.join("pip.conf"),
        "[global]\nIndex-Url = https://pypi.corp.internal/simple\n",
    )
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "handles_case_insensitive_pip_keys_and_ancestor_uv_config",
            "--exact",
        ])
        .env("APPCHECKER_PIP_CASE_CHILD", "1")
        .env("PIP_CONFIG_FILE", root.join("pip.conf"))
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn benign_cargo_config_does_not_disable_public_dependencies() {
    let root = fixture_root();
    std::fs::create_dir_all(root.join(".cargo")).unwrap();
    std::fs::write(root.join(".cargo/config.toml"), "[build]\njobs = 4\n").unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[dependencies]\nserde = \"1.0.0\"\n",
    )
    .unwrap();
    let report = DepsParser::detect_and_report(&root);
    assert_eq!(report.dependencies.len(), 1);
    assert!(!report.non_public_skipped);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn private_project_npmrc_has_precedence_over_public_user_config() {
    if std::env::var_os("APPCHECKER_NPM_PRECEDENCE_CHILD").is_some() {
        let root = fixture_root();
        std::fs::write(root.join(".npmrc"), "registry=https://npm.corp.internal/\n").unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@corp/internal-auth":"1.0.0"}}"#,
        )
        .unwrap();
        let report = DepsParser::detect_and_report(&root);
        assert!(report.dependencies.is_empty());
        assert!(report.non_public_skipped);
        std::fs::remove_dir_all(root).ok();
        return;
    }
    let root = fixture_root();
    std::fs::write(
        root.join(".npmrc"),
        "registry=https://registry.npmjs.org/\n",
    )
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "private_project_npmrc_has_precedence_over_public_user_config",
            "--exact",
        ])
        .env("APPCHECKER_NPM_PRECEDENCE_CHILD", "1")
        .env("HOME", &root)
        .env("NPM_CONFIG_USERCONFIG", root.join(".npmrc"))
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::remove_dir_all(root).ok();
}
