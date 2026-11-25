use appchecker::desktop::ai::{AiProvider, parse_models};

#[test]
fn codex_catalog_returns_visible_api_models_in_priority_order() {
    let input = r#"{"models":[
        {"slug":"gpt-second","display_name":"Second","priority":2,"supported_in_api":true,"visibility":"list"},
        {"slug":"gpt-first","display_name":"First","priority":1,"supported_in_api":true,"visibility":"list"},
        {"slug":"gpt-hidden","display_name":"Hidden","priority":0,"supported_in_api":false,"visibility":"list"}
    ]}"#;
    let models = parse_models(AiProvider::Codex, input).unwrap();
    let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
    assert_eq!(ids, ["gpt-first", "gpt-second"]);
    assert_eq!(models[0].label, "First");
}

#[test]
fn antigravity_models_keep_the_cli_display_names() {
    let input = "Fetching available models...\ngemini-4-neon-high\tGemini 4 Neon (High)\nclaude-sonnet-5-5-medium\tClaude Sonnet 5.5 (Medium)\n";
    let models = parse_models(AiProvider::Antigravity, input).unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "gemini-4-neon-high");
    assert_eq!(models[1].label, "Claude Sonnet 5.5 (Medium)");
}

#[test]
fn claude_help_exposes_supported_latest_aliases() {
    let input = "--model <model> Use an alias for the latest version of a model (e.g. 'fable', 'opus' or 'sonnet') or a model's full name.";
    let models = parse_models(AiProvider::Claude, input).unwrap();
    let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
    assert_eq!(ids, ["fable", "opus", "sonnet"]);
}

#[test]
fn an_empty_model_catalog_is_an_explicit_error() {
    let error =
        parse_models(AiProvider::Antigravity, "Fetching available models...\n").unwrap_err();
    assert!(error.to_string().contains("No models"));
}

#[test]
#[ignore = "requires the installed provider CLIs"]
fn installed_provider_catalogs_parse_successfully() {
    for provider in [
        AiProvider::Claude,
        AiProvider::Codex,
        AiProvider::Antigravity,
    ] {
        let models = appchecker::desktop::ai::ProviderClient::new(provider)
            .models()
            .unwrap();
        assert!(!models.is_empty(), "no models returned for {provider:?}");
    }
}
