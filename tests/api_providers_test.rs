use appchecker::desktop::ai::AiProvider;
use appchecker::desktop::api::{ApiClient, ModelPricing};

#[test]
fn non_cn_endpoints_are_explicit() {
    assert_eq!(
        ApiClient::base_url(AiProvider::Kimi),
        "https://api.kimi.ai/coding/v1"
    );
    assert_eq!(
        ApiClient::base_url(AiProvider::Zai),
        "https://api.z.ai/api/coding/paas/v4"
    );
    assert_eq!(
        ApiClient::base_url(AiProvider::Alibaba),
        "https://coding-intl.dashscope.aliyuncs.com/v1"
    );
    assert_eq!(
        ApiClient::base_url(AiProvider::OpenRouter),
        "https://openrouter.ai/api/v1"
    );
}

#[test]
fn openrouter_prices_are_converted_per_million_tokens() {
    let payload = r#"{"data":[{"id":"vendor/model","name":"Vendor Model","pricing":{"prompt":"0.000001","completion":"0.0000025"}}]}"#;
    let models = ApiClient::parse_models(AiProvider::OpenRouter, payload).unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(
        models[0].pricing,
        Some(ModelPricing {
            input_per_million: "$1.00".into(),
            output_per_million: "$2.50".into(),
        })
    );
}

#[test]
fn generic_openai_and_anthropic_catalogs_are_supported() {
    let openai = ApiClient::parse_models(
        AiProvider::OpenAi,
        r#"{"data":[{"id":"gpt-example","owned_by":"openai"}]}"#,
    )
    .unwrap();
    assert_eq!(openai[0].id, "gpt-example");

    let anthropic = ApiClient::parse_models(
        AiProvider::Anthropic,
        r#"{"data":[{"id":"claude-example","display_name":"Claude Example","lifecycle":"active"}]}"#,
    )
    .unwrap();
    assert_eq!(anthropic[0].label, "Claude Example");
}

#[test]
fn openai_structured_content_is_parsed() {
    let payload = r#"{"choices":[{"message":{"content":"{\"findings\":[{\"package\":\"serde\",\"summary\":\"Major\",\"breaking_changes\":[\"removed API\"]}],\"patch\":\"\"}"}}]}"#;
    let analysis = ApiClient::parse_analysis(AiProvider::OpenAi, payload).unwrap();
    assert_eq!(analysis.findings[0].package, "serde");
}

#[test]
fn anthropic_tool_input_is_parsed() {
    let payload = r#"{"content":[{"type":"tool_use","name":"remediation","input":{"findings":[],"patch":"diff --git a/a b/a"}}]}"#;
    let analysis = ApiClient::parse_analysis(AiProvider::Anthropic, payload).unwrap();
    assert!(analysis.patch.contains("diff --git"));
}

#[test]
#[ignore = "uses the public OpenRouter model catalog"]
fn live_openrouter_models_include_prices() {
    let models = ApiClient::new(AiProvider::OpenRouter).models().unwrap();
    assert!(!models.is_empty());
    assert!(models.iter().any(|model| model.pricing.is_some()));
}

#[test]
#[ignore = "uses the desktop secret service with a disposable fake key"]
fn keyring_roundtrip_uses_a_disposable_fake_key() {
    appchecker::desktop::credentials::set(AiProvider::OpenRouter, "fake-not-a-real-key").unwrap();
    let stored = appchecker::desktop::credentials::get(AiProvider::OpenRouter).unwrap();
    assert_eq!(stored.as_deref(), Some("fake-not-a-real-key"));
    appchecker::desktop::credentials::delete(AiProvider::OpenRouter).unwrap();
}

#[test]
fn openai_request_uses_selected_model_and_approved_sources() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0_u8; 32768];
        let read = std::io::Read::read(&mut stream, &mut buffer).unwrap();
        let request = String::from_utf8_lossy(&buffer[..read]);
        assert!(request.contains("POST /chat/completions"));
        assert!(request.contains("Bearer test-key"));
        assert!(request.contains("gpt-test"));
        assert!(request.contains("requirements.txt"));
        assert!(request.contains("json_schema"));
        let body = r#"{"choices":[{"message":{"content":"{\"findings\":[],\"patch\":\"\"}"}}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        std::io::Write::write_all(&mut stream, response.as_bytes()).unwrap();
    });
    let root = std::env::temp_dir().join(format!("appchecker-api-{}", uuid::Uuid::new_v4()));
    let repository = root.join("repo");
    std::fs::create_dir_all(&repository).unwrap();
    std::fs::write(repository.join("requirements.txt"), "requests==1.0.0\n").unwrap();
    let prepared = appchecker::desktop::ai::prepare_analysis_files(
        &repository,
        &[std::path::PathBuf::from("requirements.txt")],
        &root.join("analysis"),
    )
    .unwrap();
    let response = ApiClient::from_provider(AiProvider::OpenAi, Some("test-key".into()))
        .with_base_url(format!("http://{address}"))
        .analyze(&prepared, Some("gpt-test".into()))
        .unwrap();
    server.join().unwrap();
    assert!(response.findings.is_empty());
    std::fs::remove_dir_all(root).ok();
}
