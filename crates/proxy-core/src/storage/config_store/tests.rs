use super::*;
use crate::domain::{
    AppConfig, ModelCapabilities, ModelCompressionPolicy, ModelTokenLimits, ParameterOverrides,
    Provider, ProviderProtocol, TiktokenEncoding, TokenLimitSource, TokenizerConfig, UpstreamModel,
    VirtualModel, DEFAULT_PROXY_PORT, MIN_PROXY_PORT,
};
use std::collections::HashMap;

fn compression_policy() -> ModelCompressionPolicy {
    ModelCompressionPolicy {
        token_threshold: 80_000,
        max_token_limit: 100_000,
        max_output_tokens: 20_000,
        ..ModelCompressionPolicy::default()
    }
}

fn sample_config() -> AppConfig {
    AppConfig {
        proxy_port: DEFAULT_PROXY_PORT,
        disabled_official_models: std::collections::HashSet::new(),
        providers: vec![Provider {
            id: "provider-1".to_string(),
            name: "Provider".to_string(),
            protocol: ProviderProtocol::OpenaiChatCompletions,
            models_endpoint: "https://api.example.com/v1/models".to_string(),
            generate_endpoint: "https://api.example.com/v1/chat/completions".to_string(),
            api_key: "sk-test".to_string(),
            headers: HashMap::new(),
            default_parameters: ParameterOverrides::default(),
            connect_timeout_ms: 3000,
            request_timeout_ms: 60000,
            stream_idle_timeout_ms: 30000,
            enabled: true,
        }],
        upstream_models: vec![UpstreamModel {
            id: "upstream-1".to_string(),
            provider_id: "provider-1".to_string(),
            upstream_model_id: "gpt-test".to_string(),
            display_name: "GPT Test".to_string(),
            capabilities: ModelCapabilities::default(),
            token_limits: ModelTokenLimits::default(),
            compression_policy: None,
            tokenizer: None,
            parameter_overrides: ParameterOverrides::default(),
            enabled: true,
        }],
        virtual_models: vec![VirtualModel {
            id: "virtual-1".to_string(),
            host_model_id: None,
            upstream_model_id: "upstream-1".to_string(),
            display_name: "Virtual Test".to_string(),
            default_reasoning_level: None,
            parameter_overrides: ParameterOverrides::default(),
            fallback_virtual_model_id: None,
            enabled: true,
        }],
        model_compression_policies: Default::default(),
        custom_host_paths: Default::default(),
    }
}

#[test]
fn config_store_persists_and_reloads_valid_config() {
    let directory =
        std::env::temp_dir().join(format!("agy-byok-config-test-{}", uuid::Uuid::new_v4()));
    let path = directory.join("config.v1.json");
    let store = ConfigStore::load_from_file(&path).unwrap();
    assert!(store.get_config().providers.is_empty());

    let mut config = sample_config();
    config.upstream_models[0].compression_policy = Some(compression_policy());
    store.update_config(config).unwrap();
    store
        .update_config_with(|config| config.providers[0].name = "Updated Provider".to_string())
        .unwrap();
    let reloaded = ConfigStore::load_from_file(&path).unwrap();

    assert_eq!(reloaded.get_config().providers[0].id, "provider-1");
    assert_eq!(reloaded.get_config().providers[0].name, "Updated Provider");
    assert_eq!(reloaded.get_config().providers[0].api_key, "sk-test");
    assert_eq!(
        reloaded.get_config().upstream_models[0].compression_policy,
        Some(compression_policy())
    );
    assert!(!path.with_extension("tmp").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let _ = fs::remove_dir_all(directory);
}

#[cfg(unix)]
#[test]
fn private_temporary_write_never_follows_an_existing_symlink() {
    use std::os::unix::fs::symlink;

    let directory =
        std::env::temp_dir().join(format!("agy-byok-config-test-{}", uuid::Uuid::new_v4()));
    let victim = directory.join("victim.txt");
    let temporary = directory.join("config.next");
    fs::create_dir_all(&directory).unwrap();
    fs::write(&victim, b"unchanged").unwrap();
    symlink(&victim, &temporary).unwrap();

    let error = write_private_file(&temporary, b"replacement").unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&victim).unwrap(), b"unchanged");
    let _ = fs::remove_dir_all(directory);
}

#[cfg(unix)]
#[test]
fn config_store_rejects_symbolic_link_inputs() {
    use std::os::unix::fs::symlink;

    let directory =
        std::env::temp_dir().join(format!("agy-byok-config-test-{}", uuid::Uuid::new_v4()));
    let target = directory.join("target.json");
    let link = directory.join("config.v1.json");
    fs::create_dir_all(&directory).unwrap();
    fs::write(&target, serde_json::to_vec(&AppConfig::default()).unwrap()).unwrap();
    symlink(&target, &link).unwrap();

    let error = ConfigStore::load_from_file(&link).err().unwrap();

    assert!(matches!(error, ConfigStoreError::InvalidFileType { path } if path == link));
    let _ = fs::remove_dir_all(directory);
}

#[cfg(unix)]
#[test]
fn config_store_secures_existing_config_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let directory =
        std::env::temp_dir().join(format!("agy-byok-config-test-{}", uuid::Uuid::new_v4()));
    let path = directory.join("config.v1.json");
    fs::create_dir_all(&directory).unwrap();
    fs::write(&path, serde_json::to_vec(&AppConfig::default()).unwrap()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

    ConfigStore::load_from_file(&path).unwrap();

    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn parse_error_identifies_the_config_file() {
    let directory =
        std::env::temp_dir().join(format!("agy-byok-config-test-{}", uuid::Uuid::new_v4()));
    let path = directory.join("config.v1.json");
    fs::create_dir_all(&directory).unwrap();
    fs::write(&path, "{ invalid json").unwrap();

    let error = ConfigStore::load_from_file(&path).err().unwrap();

    assert!(matches!(
        &error,
        ConfigStoreError::Parse {
            path: error_path,
            ..
        } if error_path == &path
    ));
    assert!(error.to_string().contains(path.to_str().unwrap()));
    assert!(path.exists());
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn incompatible_schema_is_deleted_and_replaced_with_defaults_in_memory() {
    let directory =
        std::env::temp_dir().join(format!("agy-byok-config-test-{}", uuid::Uuid::new_v4()));
    let path = directory.join("config.v1.json");
    fs::create_dir_all(&directory).unwrap();
    let mut value = serde_json::to_value(sample_config()).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .remove("disabled_official_models");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

    let store = ConfigStore::load_from_file(&path).unwrap();

    assert!(!path.exists());
    assert!(store.get_config().providers.is_empty());
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn invalid_config_is_deleted_and_replaced_with_defaults_in_memory() {
    let directory =
        std::env::temp_dir().join(format!("agy-byok-config-test-{}", uuid::Uuid::new_v4()));
    let path = directory.join("config.v1.json");
    fs::create_dir_all(&directory).unwrap();
    let mut config = sample_config();
    config.proxy_port = MIN_PROXY_PORT - 1;
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();

    let store = ConfigStore::load_from_file(&path).unwrap();

    assert!(!path.exists());
    assert_eq!(store.get_config().proxy_port, DEFAULT_PROXY_PORT);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn atomic_updates_preserve_independent_changes() {
    let store = ConfigStore::in_memory(sample_config());
    let port_store = store.clone();
    let provider_store = store.clone();

    let port_update = std::thread::spawn(move || {
        port_store
            .update_config_with(|config| config.proxy_port = 52345)
            .unwrap();
    });
    let provider_update = std::thread::spawn(move || {
        provider_store
            .update_config_with(|config| config.providers[0].name = "Updated".to_string())
            .unwrap();
    });
    port_update.join().unwrap();
    provider_update.join().unwrap();

    let config = store.get_config();
    assert_eq!(config.proxy_port, 52345);
    assert_eq!(config.providers[0].name, "Updated");
}

#[test]
fn config_rejects_missing_provider_api_key() {
    let mut value = serde_json::to_value(sample_config()).unwrap();
    value["providers"][0]
        .as_object_mut()
        .unwrap()
        .remove("api_key");

    assert!(serde_json::from_value::<AppConfig>(value).is_err());
}

#[test]
fn model_compression_policy_round_trips_in_upstream_and_official_map() {
    let policy = compression_policy();
    let mut config = sample_config();
    config.upstream_models[0].compression_policy = Some(policy.clone());
    config
        .model_compression_policies
        .insert("official-model".to_string(), policy.clone());

    let value = serde_json::to_value(&config).unwrap();
    assert_eq!(
        value["upstream_models"][0]["compression_policy"],
        serde_json::to_value(&policy).unwrap()
    );
    assert_eq!(
        value["model_compression_policies"]["official-model"],
        serde_json::to_value(&policy).unwrap()
    );

    let round_trip: AppConfig = serde_json::from_value(value).unwrap();
    assert_eq!(
        round_trip.upstream_models[0].compression_policy,
        Some(policy.clone())
    );
    assert_eq!(
        round_trip.model_compression_policies.get("official-model"),
        Some(&policy)
    );
}

#[test]
fn config_validation_rejects_invalid_model_compression_policies() {
    for (threshold, limit, output) in [(0, 100, 20), (100, 100, 1), (1, 100, 100), (80, 100, 30)] {
        let mut policy = compression_policy();
        policy.token_threshold = threshold;
        policy.max_token_limit = limit;
        policy.max_output_tokens = output;
        let mut config = sample_config();
        config.upstream_models[0].compression_policy = Some(policy);

        let error = config.validate().unwrap_err();

        assert!(
            error.to_string().contains("UpstreamModel upstream-1"),
            "{error}"
        );
    }
}

#[test]
fn config_rejects_missing_model_token_limits() {
    let mut value = serde_json::to_value(sample_config()).unwrap();
    value["upstream_models"][0]
        .as_object_mut()
        .unwrap()
        .remove("token_limits");

    assert!(serde_json::from_value::<AppConfig>(value).is_err());
}

#[test]
fn config_rejects_missing_nullable_fields() {
    for (parent_pointer, field) in [
        ("", "disabled_official_models"),
        ("", "custom_host_paths"),
        ("/custom_host_paths", "app"),
        ("/custom_host_paths", "ide"),
        ("/providers/0/default_parameters", "temperature"),
        ("/providers/0/default_parameters", "max_tokens"),
        ("/providers/0/default_parameters", "top_p"),
        ("/providers/0/default_parameters", "top_k"),
        ("/providers/0/default_parameters", "extra_body"),
        ("/upstream_models/0/token_limits", "context_window"),
        ("/upstream_models/0/token_limits", "input_token_limit"),
        ("/upstream_models/0/token_limits", "output_token_limit"),
        ("/upstream_models/0", "compression_policy"),
        ("/upstream_models/0", "tokenizer"),
        ("/virtual_models/0", "host_model_id"),
        ("/virtual_models/0", "default_reasoning_level"),
        ("/virtual_models/0", "fallback_virtual_model_id"),
    ] {
        let mut value = serde_json::to_value(sample_config()).unwrap();
        value
            .pointer_mut(parent_pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);

        assert!(
            serde_json::from_value::<AppConfig>(value).is_err(),
            "missing field was accepted: {parent_pointer}/{field}"
        );
    }
}

#[test]
fn config_rejects_unknown_fields_inside_nested_values() {
    let mut value = serde_json::to_value(sample_config()).unwrap();
    let mut policy = serde_json::to_value(compression_policy()).unwrap();
    policy
        .as_object_mut()
        .unwrap()
        .insert("unexpected".to_string(), serde_json::json!(true));
    value["upstream_models"][0]["compression_policy"] = policy;
    assert!(serde_json::from_value::<AppConfig>(value).is_err());

    let mut value = serde_json::to_value(sample_config()).unwrap();
    value["upstream_models"][0]["tokenizer"] = serde_json::json!({
        "kind": "tiktoken",
        "encoding": "o200k_base",
        "unexpected": true
    });
    assert!(serde_json::from_value::<AppConfig>(value).is_err());

    let mut value = serde_json::to_value(sample_config()).unwrap();
    value["upstream_models"][0]["capabilities"]["reasoning"]["levels"] = serde_json::json!({
        "low": {
            "kind": "effort",
            "value": "low",
            "unexpected": true
        }
    });
    assert!(serde_json::from_value::<AppConfig>(value).is_err());
}

#[test]
fn config_rejects_missing_token_limit_sources() {
    let mut value = serde_json::to_value(sample_config()).unwrap();
    value["upstream_models"][0]["token_limits"] = serde_json::json!({
        "context_window": 200_000,
        "input_token_limit": 180_000,
        "output_token_limit": 20_000
    });

    assert!(serde_json::from_value::<AppConfig>(value).is_err());
}

#[test]
fn token_limit_sources_serialize_as_snake_case() {
    let mut config = sample_config();
    let limits = &mut config.upstream_models[0].token_limits;
    limits.context_window_source = TokenLimitSource::Catalog;
    limits.input_token_limit_source = TokenLimitSource::Configured;
    limits.output_token_limit_source = TokenLimitSource::Estimated;

    let value = serde_json::to_value(config).unwrap();
    let limits = &value["upstream_models"][0]["token_limits"];

    assert_eq!(limits["context_window_source"], "catalog");
    assert_eq!(limits["input_token_limit_source"], "configured");
    assert_eq!(limits["output_token_limit_source"], "estimated");
}

#[test]
fn tiktoken_encodings_round_trip() {
    for (encoding, serialized_encoding) in [
        (TiktokenEncoding::Cl100kBase, "cl100k_base"),
        (TiktokenEncoding::O200kBase, "o200k_base"),
    ] {
        let mut config = sample_config();
        let tokenizer = TokenizerConfig::Tiktoken { encoding };
        config.upstream_models[0].tokenizer = Some(tokenizer);

        let value = serde_json::to_value(&config).unwrap();
        assert_eq!(
            value["upstream_models"][0]["tokenizer"],
            serde_json::json!({
                "kind": "tiktoken",
                "encoding": serialized_encoding
            })
        );

        let round_trip: AppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(round_trip.upstream_models[0].tokenizer, Some(tokenizer));
    }
}

#[test]
fn tokenizer_rejects_unsupported_tiktoken_encoding() {
    let mut value = serde_json::to_value(sample_config()).unwrap();
    value["upstream_models"][0]["tokenizer"] = serde_json::json!({
        "kind": "tiktoken",
        "encoding": "p50k_base"
    });

    assert!(serde_json::from_value::<AppConfig>(value).is_err());
}

#[test]
fn config_validation_rejects_zero_model_token_limits() {
    let mut config = sample_config();
    config.upstream_models[0].token_limits.input_token_limit = Some(0);

    let error = config.validate().unwrap_err();

    assert!(error
        .to_string()
        .contains("input_token_limit must be greater than 0"));
}

#[test]
fn config_validation_accepts_official_catalog_ids_and_rejects_empty_policy_keys() {
    let mut config = sample_config();
    config
        .model_compression_policies
        .insert("official-model".to_string(), compression_policy());
    assert!(config.validate().is_ok());

    config
        .model_compression_policies
        .insert(String::new(), compression_policy());
    assert!(config
        .validate()
        .unwrap_err()
        .to_string()
        .contains("empty model ID"));
}

#[test]
fn config_validation_rejects_invalid_provider_runtime_settings() {
    let mut zero_timeout = sample_config();
    zero_timeout.providers[0].connect_timeout_ms = 0;
    assert!(zero_timeout.validate().is_err());

    let mut inverted_timeouts = sample_config();
    inverted_timeouts.providers[0].connect_timeout_ms = 60_001;
    assert!(inverted_timeouts.validate().is_err());

    let mut missing_catalog = sample_config();
    missing_catalog.providers[0].models_endpoint.clear();
    assert!(missing_catalog.validate().is_err());

    let mut invalid_parameter = sample_config();
    invalid_parameter.providers[0].default_parameters.top_p = Some(1.1);
    assert!(invalid_parameter.validate().is_err());

    let mut invalid_header = sample_config();
    invalid_header.providers[0]
        .headers
        .insert("invalid header".to_string(), "value".to_string());
    assert!(invalid_header.validate().is_err());
}

#[test]
fn config_rejects_missing_or_reserved_proxy_port() {
    let mut value = serde_json::to_value(sample_config()).unwrap();
    value.as_object_mut().unwrap().remove("proxy_port");
    assert!(serde_json::from_value::<AppConfig>(value).is_err());

    let mut config = sample_config();
    config.proxy_port = MIN_PROXY_PORT - 1;
    assert!(config.validate().is_err());
}

#[test]
fn config_validation_rejects_missing_references() {
    let mut config = sample_config();
    config.upstream_models[0].provider_id = "missing".to_string();

    let error = config.validate().unwrap_err();

    assert!(error.to_string().contains("missing Provider"));
}

#[test]
fn config_validation_allows_http_only_for_loopback() {
    let mut config = sample_config();
    config.providers[0].generate_endpoint =
        "http://127.0.0.1:11434/v1/chat/completions".to_string();
    assert!(config.validate().is_ok());

    config.providers[0].generate_endpoint =
        "http://api.example.com/v1/chat/completions".to_string();
    assert!(config.validate().is_err());
}

#[test]
fn virtual_model_derives_a_stable_ide_placeholder() {
    let config = sample_config();
    let model = &config.virtual_models[0];
    let before = model.effective_host_model_id().into_owned();
    let mut renamed = model.clone();
    renamed.display_name = "Renamed".to_string();

    assert_eq!(renamed.effective_host_model_id(), before);
    assert!(before.starts_with("MODEL_PLACEHOLDER_M"));
    assert!(model.has_valid_host_model_id());
}

#[test]
fn config_validation_rejects_invalid_or_duplicate_host_model_ids() {
    let mut config = sample_config();
    config.virtual_models[0].host_model_id = Some("not-an-ide-placeholder".to_string());
    assert!(config.validate().is_err());

    let mut config = sample_config();
    let mut duplicate = config.virtual_models[0].clone();
    duplicate.id = "virtual-2".to_string();
    duplicate.host_model_id = Some("MODEL_PLACEHOLDER_M400".to_string());
    config.virtual_models[0].host_model_id = Some("MODEL_PLACEHOLDER_M400".to_string());
    config.virtual_models.push(duplicate);
    assert!(config.validate().is_err());
}

#[test]
fn config_validation_rejects_catalog_key_collision_with_disabled_model() {
    let mut config = sample_config();
    config.virtual_models[0].id = "foo".to_string();
    config.virtual_models[0].host_model_id = Some("MODEL_PLACEHOLDER_M400".to_string());

    let mut conflicting = config.virtual_models[0].clone();
    conflicting.id = "custom-foo".to_string();
    conflicting.host_model_id = Some("MODEL_PLACEHOLDER_M401".to_string());
    conflicting.enabled = false;
    config.virtual_models.push(conflicting);

    let error = config.validate().unwrap_err();

    assert!(error.to_string().contains("custom-foo"));
}
