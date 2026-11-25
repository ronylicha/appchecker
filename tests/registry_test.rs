use appchecker::core::{DepInfo, Ecosystem, RegistryUrls, UpdateSeverity};
use appchecker::deps::RegistryClient;
use chrono::Utc;
use std::io::{Read, Write};
use std::net::TcpListener;

fn dependency(name: &str) -> DepInfo {
    DepInfo {
        name: name.into(),
        ecosystem: Ecosystem::Npm,
        current_version: "1.0.0".into(),
        latest_version: None,
        update_available: false,
        severity: UpdateSeverity::Info,
        dev_dependency: false,
        manifest_name: None,
        checked_at: Utc::now(),
    }
}

#[test]
fn compares_partial_versions_and_ignores_unresolved_wildcards() {
    assert!(RegistryClient::is_newer("1", "1.1.0"));
    assert!(!RegistryClient::is_newer("1.0", "1.0.0"));
    assert!(RegistryClient::is_newer("^1.0.0", "1.1.0"));
    assert!(RegistryClient::is_newer("1.0.0", "1.0.1"));
    assert!(!RegistryClient::is_newer("*", "2.0.0"));
    assert!(!RegistryClient::is_newer("<2.0.0", "2.2.3"));
    assert!(!RegistryClient::is_comparable("<=1.10.15"));
}

#[tokio::test]
async fn detailed_check_separates_updates_from_registry_failures() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0_u8; 2048];
            let read = stream.read(&mut buffer).unwrap();
            let request = String::from_utf8_lossy(&buffer[..read]);
            let (status, body) = if request.contains("/express") {
                ("200 OK", r#"{"dist-tags":{"latest":"2.0.0"}}"#)
            } else {
                ("500 Internal Server Error", "registry unavailable")
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });

    let urls = RegistryUrls {
        npm: format!("http://{address}"),
        ..RegistryUrls::default()
    };
    let client = RegistryClient::new(urls, false);
    let mut broken = dependency("broken");
    broken.dev_dependency = true;
    broken.manifest_name = Some("broken_alias".into());
    let result = client
        .check_batch_detailed(&[dependency("express"), broken], 2)
        .await;
    server.join().unwrap();

    assert_eq!(result.dependencies.len(), 2);
    let express = result
        .dependencies
        .iter()
        .find(|dependency| dependency.name == "express")
        .unwrap();
    assert_eq!(express.latest_version.as_deref(), Some("2.0.0"));
    assert!(express.update_available);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].package, "broken");
    assert!(result.errors[0].dev_dependency);
    assert_eq!(
        result.errors[0].manifest_name.as_deref(),
        Some("broken_alias")
    );
    assert!(result.errors[0].message.contains("500"));
}

#[tokio::test]
async fn go_proxy_escapes_uppercase_module_letters() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0_u8; 2048];
        let read = stream.read(&mut buffer).unwrap();
        let request = String::from_utf8_lossy(&buffer[..read]);
        assert!(
            request.contains("github.com/!burnt!sushi/toml"),
            "{request}"
        );
        let body = r#"{"Version":"v1.0.0"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    let urls = RegistryUrls {
        go_proxy: format!("http://{address}"),
        ..RegistryUrls::default()
    };
    let client = RegistryClient::new(urls, false);
    let mut dep = dependency("github.com/BurntSushi/toml");
    dep.ecosystem = Ecosystem::Go;

    let (checked, error) = client.check_update_detailed(&dep).await;

    server.join().unwrap();
    assert!(error.is_none(), "{error:?}");
    assert_eq!(checked.latest_version.as_deref(), Some("1.0.0"));
}

#[tokio::test]
async fn excessive_concurrency_is_bounded_instead_of_panicking() {
    let client = RegistryClient::new(RegistryUrls::default(), true);
    let result = client.check_batch_detailed(&[], usize::MAX).await;
    assert!(result.dependencies.is_empty());
    assert!(result.errors.is_empty());
}

#[tokio::test]
async fn batch_checks_log_registry_failures_for_daemon_users() {
    #[derive(Clone)]
    struct LogBuffer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for LogBuffer {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0_u8; 2048];
        let _ = stream.read(&mut buffer).unwrap();
        stream.write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    });
    let logs = LogBuffer(std::sync::Arc::new(std::sync::Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer({
            let logs = logs.clone();
            move || logs.clone()
        })
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let client = RegistryClient::new(
        RegistryUrls {
            npm: format!("http://{address}"),
            ..RegistryUrls::default()
        },
        false,
    );
    let checked = client.check_batch(&[dependency("broken")], 1).await;
    server.join().unwrap();

    assert_eq!(checked.len(), 1);
    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(
        output.contains("broken"),
        "missing package in daemon logs: {output}"
    );
    assert!(
        output.contains("500"),
        "missing registry error in daemon logs: {output}"
    );
}

#[tokio::test]
async fn osv_returns_cve_aliases_and_highest_severity() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).unwrap();
        let request = String::from_utf8_lossy(&buffer[..read]);
        assert!(request.contains("POST /v1/query"));
        assert!(request.contains("\"ecosystem\":\"npm\""));
        assert!(request.contains("\"name\":\"express\""));
        assert!(request.contains("\"version\":\"1.0.0\""));
        let body = r#"{"vulns":[{"id":"GHSA-one","aliases":["CVE-2024-0001"],"database_specific":{"severity":"HIGH"},"summary":"Example high"},{"id":"GHSA-two","aliases":["CVE-2024-0002"],"database_specific":{"severity":"CRITICAL"},"summary":"Example critical"}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    let client = RegistryClient::new(
        RegistryUrls {
            osv: format!("http://{address}"),
            ..RegistryUrls::default()
        },
        false,
    );

    let vulnerabilities = client
        .query_vulnerabilities(&dependency("express"))
        .await
        .unwrap();
    server.join().unwrap();

    assert_eq!(vulnerabilities.len(), 2);
    assert_eq!(vulnerabilities[0].aliases[0], "CVE-2024-0001");
    assert_eq!(
        appchecker::deps::highest_vulnerability_severity(&vulnerabilities),
        appchecker::deps::VulnerabilitySeverity::Critical
    );
}

#[tokio::test]
async fn osv_failure_is_not_reported_as_no_known_vulnerabilities() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0_u8; 2048];
        let _ = stream.read(&mut buffer).unwrap();
        let body = "upstream unavailable";
        let response = format!(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    let client = RegistryClient::new(
        RegistryUrls {
            osv: format!("http://{address}"),
            ..RegistryUrls::default()
        },
        false,
    );

    let error = client
        .query_vulnerabilities(&dependency("express"))
        .await
        .unwrap_err();
    server.join().unwrap();
    assert!(error.to_string().contains("503"));
}

#[tokio::test]
async fn air_gap_skips_osv_without_contacting_a_server() {
    let client = RegistryClient::new(RegistryUrls::default(), true);
    let error = client
        .query_vulnerabilities(&dependency("express"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("air-gap"));
}

#[tokio::test]
#[ignore = "requires network access to the public OSV service"]
async fn live_osv_queries_the_real_service() {
    let mut dependency = dependency("requests");
    dependency.ecosystem = Ecosystem::Pip;
    dependency.current_version = "2.31.0".into();
    let client = RegistryClient::new(RegistryUrls::default(), false);
    let result = client.query_vulnerabilities(&dependency).await;
    assert!(result.is_ok(), "live OSV query failed: {result:?}");
}
