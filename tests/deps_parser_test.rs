use std::path::PathBuf;

/// Helper to get fixture path for a given ecosystem
fn fixture_path(ecosystem: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(ecosystem)
}

#[test]
fn test_npm_parser_detects_dependencies_and_dev_dependencies() {
    let path = fixture_path("npm");
    let deps = appchecker::deps::DepsParser::detect_and_parse(&path);

    // Should find 3 dependencies + 2 devDependencies = 5 total
    assert_eq!(deps.len(), 5, "Expected 5 npm deps, got {}", deps.len());

    // Check specific packages
    let express = deps
        .iter()
        .find(|d| d.name == "express")
        .expect("express not found");
    assert_eq!(express.current_version, "4.18.2");
    assert_eq!(express.ecosystem.to_string(), "npm");

    let lodash = deps
        .iter()
        .find(|d| d.name == "lodash")
        .expect("lodash not found");
    assert_eq!(lodash.current_version, "4.17.21");

    let typescript = deps
        .iter()
        .find(|d| d.name == "typescript")
        .expect("typescript not found");
    assert_eq!(typescript.current_version, "5.3.0");
    assert!(typescript.dev_dependency);
    assert!(!express.dev_dependency);
}

#[test]
fn test_cargo_parser_detects_dependencies_and_dev_dependencies() {
    let path = fixture_path("cargo");
    let deps = appchecker::deps::DepsParser::detect_and_parse(&path);

    // Should find 3 dependencies + 1 dev-dependency = 4 total
    assert_eq!(deps.len(), 4, "Expected 4 cargo deps, got {}", deps.len());

    let serde = deps
        .iter()
        .find(|d| d.name == "serde")
        .expect("serde not found");
    assert_eq!(serde.current_version, "1.0");
    assert_eq!(serde.ecosystem.to_string(), "cargo");

    let tokio = deps
        .iter()
        .find(|d| d.name == "tokio")
        .expect("tokio not found");
    assert_eq!(tokio.current_version, "1");

    let criterion = deps
        .iter()
        .find(|d| d.name == "criterion")
        .expect("criterion not found");
    assert_eq!(criterion.current_version, "0.5");
    assert!(criterion.dev_dependency);
    assert!(!serde.dev_dependency);
}

#[test]
fn test_go_parser_detects_require_block_and_single_require() {
    let path = fixture_path("go");
    let deps = appchecker::deps::DepsParser::detect_and_parse(&path);

    // Should find 3 in require block + 1 single require = 4 total
    assert_eq!(deps.len(), 4, "Expected 4 go deps, got {}", deps.len());

    let gin = deps
        .iter()
        .find(|d| d.name == "github.com/gin-gonic/gin")
        .expect("gin not found");
    assert_eq!(gin.current_version, "1.9.1");
    assert_eq!(gin.ecosystem.to_string(), "go");

    let zap = deps
        .iter()
        .find(|d| d.name == "go.uber.org/zap")
        .expect("zap not found");
    assert_eq!(zap.current_version, "1.26.0");

    // Single-line require (indirect)
    let errors = deps
        .iter()
        .find(|d| d.name == "github.com/pkg/errors")
        .expect("pkg/errors not found");
    assert_eq!(errors.current_version, "0.9.1");
}

#[test]
fn test_pip_parser_detects_pyproject_toml_dependencies() {
    let path = fixture_path("pip");
    let deps = appchecker::deps::DepsParser::detect_and_parse(&path);

    // Should find 3 dependencies from pyproject.toml
    assert_eq!(deps.len(), 3, "Expected 3 pip deps, got {}", deps.len());

    let requests = deps
        .iter()
        .find(|d| d.name == "requests")
        .expect("requests not found");
    assert_eq!(requests.current_version, "2.31.0");
    assert_eq!(requests.ecosystem.to_string(), "pip");

    let flask = deps
        .iter()
        .find(|d| d.name == "flask")
        .expect("flask not found");
    assert_eq!(flask.current_version, "3.0.0");

    let pydantic = deps
        .iter()
        .find(|d| d.name == "pydantic")
        .expect("pydantic not found");
    assert_eq!(pydantic.current_version, ">=2.5,<3.0");
}

#[test]
fn test_composer_parser_skips_php_and_ext_requirements() {
    let path = fixture_path("composer");
    let deps = appchecker::deps::DepsParser::detect_and_parse(&path);

    // Should find 3 real deps: laravel/framework, guzzlehttp/guzzle (require) + phpunit/phpunit (require-dev)
    // Should skip "php" and "ext-json"
    assert_eq!(
        deps.len(),
        3,
        "Expected 3 composer deps, got {}",
        deps.len()
    );

    let laravel = deps
        .iter()
        .find(|d| d.name == "laravel/framework")
        .expect("laravel not found");
    assert_eq!(laravel.current_version, "11.0");
    assert_eq!(laravel.ecosystem.to_string(), "composer");

    let guzzle = deps
        .iter()
        .find(|d| d.name == "guzzlehttp/guzzle")
        .expect("guzzle not found");
    assert_eq!(guzzle.current_version, "7.8");

    let phpunit = deps
        .iter()
        .find(|d| d.name == "phpunit/phpunit")
        .expect("phpunit not found");
    assert!(phpunit.dev_dependency);
    assert!(!laravel.dev_dependency);

    // Verify php and ext-json are NOT included
    assert!(
        deps.iter().all(|d| d.name != "php"),
        "php should be skipped"
    );
    assert!(
        deps.iter().all(|d| !d.name.starts_with("ext-")),
        "ext-* should be skipped"
    );
}

#[test]
fn test_gem_parser_detects_gems_and_skips_source_group_lines() {
    let path = fixture_path("gem");
    let deps = appchecker::deps::DepsParser::detect_and_parse(&path);

    // Should find 4 gems: rails, puma, pg, rubocop
    assert_eq!(deps.len(), 4, "Expected 4 gem deps, got {}", deps.len());

    let rails = deps
        .iter()
        .find(|d| d.name == "rails")
        .expect("rails not found");
    assert_eq!(rails.current_version, "7.1");
    assert_eq!(rails.ecosystem.to_string(), "gem");

    let puma = deps
        .iter()
        .find(|d| d.name == "puma")
        .expect("puma not found");
    assert_eq!(puma.current_version, "6.0");

    let rubocop = deps
        .iter()
        .find(|d| d.name == "rubocop")
        .expect("rubocop not found");
    assert_eq!(rubocop.current_version, "1.60");
    assert!(rubocop.dev_dependency);
    assert!(!rails.dev_dependency);
}

#[test]
fn test_empty_directory_returns_no_deps() {
    let temp_dir = std::env::temp_dir().join("appchecker_test_empty");
    std::fs::create_dir_all(&temp_dir).unwrap();

    let deps = appchecker::deps::DepsParser::detect_and_parse(&temp_dir);
    assert_eq!(deps.len(), 0, "Empty dir should return 0 deps");

    std::fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn malformed_manifests_return_errors_instead_of_silent_empty_dependencies() {
    let root = std::env::temp_dir().join(format!("appchecker-malformed-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("package.json"), "{not json").unwrap();
    std::fs::write(root.join("Cargo.toml"), "[dependencies\nserde = \"1.0\"").unwrap();

    let report = appchecker::deps::DepsParser::detect_and_report(&root);

    assert!(report.dependencies.is_empty());
    assert_eq!(report.errors.len(), 2);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.manifest.ends_with("package.json"))
    );
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.manifest.ends_with("Cargo.toml"))
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn compound_python_and_gem_constraints_remain_uncomparable_ranges() {
    let root = std::env::temp_dir().join(format!("appchecker-ranges-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("requirements.txt"),
        "requests>=2.20.0,<2.30.0\nrich[color]>=13.0.0; python_version >= '3.11'\n",
    )
    .unwrap();
    std::fs::write(
        root.join("Gemfile"),
        "gem \"rails\", \">= 7.0\", \"< 8.0\"\n",
    )
    .unwrap();
    let dependencies = appchecker::deps::DepsParser::detect_and_parse(&root);
    assert_eq!(dependencies.len(), 3);
    assert!(
        dependencies.iter().all(|dependency| {
            !appchecker::deps::RegistryClient::is_comparable(&dependency.current_version)
        }),
        "compound constraints were truncated: {dependencies:?}"
    );
    std::fs::remove_dir_all(root).ok();
}
