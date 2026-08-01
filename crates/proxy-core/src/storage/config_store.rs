use crate::domain::{Provider, UpstreamModel, VirtualModel};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

pub const DEFAULT_PROXY_PORT: u16 = 51234;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,
    pub providers: Vec<Provider>,
    pub upstream_models: Vec<UpstreamModel>,
    pub virtual_models: Vec<VirtualModel>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            proxy_port: DEFAULT_PROXY_PORT,
            providers: Vec::new(),
            upstream_models: Vec::new(),
            virtual_models: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.proxy_port == 0 {
            return Err("Proxy port must be between 1 and 65535".to_string());
        }
        let mut provider_ids = HashSet::new();
        for provider in &self.providers {
            validate_id("Provider", &provider.id)?;
            if !provider_ids.insert(provider.id.as_str()) {
                return Err(format!("Duplicate Provider ID: {}", provider.id));
            }
            validate_endpoint(
                "Provider generate endpoint",
                &provider.generate_endpoint,
                true,
            )?;
            if !provider.models_endpoint.trim().is_empty() {
                validate_endpoint("Provider models endpoint", &provider.models_endpoint, false)?;
            }
        }

        let mut upstream_ids = HashSet::new();
        for model in &self.upstream_models {
            validate_id("UpstreamModel", &model.id)?;
            if !upstream_ids.insert(model.id.as_str()) {
                return Err(format!("Duplicate UpstreamModel ID: {}", model.id));
            }
            if !provider_ids.contains(model.provider_id.as_str()) {
                return Err(format!(
                    "UpstreamModel {} references missing Provider {}",
                    model.id, model.provider_id
                ));
            }
            if model.upstream_model_id.trim().is_empty() {
                return Err(format!(
                    "UpstreamModel {} has an empty upstream model ID",
                    model.id
                ));
            }
        }

        let mut virtual_ids = HashSet::new();
        let mut accepted_virtual_ids = HashMap::new();
        for model in &self.virtual_models {
            validate_id("VirtualModel", &model.id)?;
            if !virtual_ids.insert(model.id.as_str()) {
                return Err(format!("Duplicate VirtualModel ID: {}", model.id));
            }
            let host_model_id = model.effective_host_model_id().into_owned();
            validate_id("VirtualModel host model", &host_model_id)?;
            if !model.has_valid_host_model_id() {
                return Err(format!(
                    "VirtualModel {} host model ID must match MODEL_PLACEHOLDER_M400..M599",
                    model.id
                ));
            }
            for accepted_id in model.accepted_ids() {
                if let Some(existing_model_id) = accepted_virtual_ids.get(accepted_id.as_ref()) {
                    if *existing_model_id != model.id.as_str() {
                        return Err(format!(
                            "VirtualModel {} identifier conflicts with VirtualModel {}: {}",
                            model.id, existing_model_id, accepted_id
                        ));
                    }
                } else {
                    accepted_virtual_ids.insert(accepted_id.into_owned(), model.id.as_str());
                }
            }
            let upstream = self
                .upstream_models
                .iter()
                .find(|upstream| upstream.id == model.upstream_model_id)
                .ok_or_else(|| {
                    format!(
                        "VirtualModel {} references missing UpstreamModel {}",
                        model.id, model.upstream_model_id
                    )
                })?;
            if let Some(level) = model.default_reasoning_level {
                if upstream.capabilities.reasoning.mapping_for(level).is_none() {
                    return Err(format!(
                        "VirtualModel {} uses unsupported reasoning level {:?}",
                        model.id, level
                    ));
                }
            }
        }

        for model in &self.virtual_models {
            if let Some(fallback_id) = &model.fallback_virtual_model_id {
                if fallback_id == &model.id {
                    return Err(format!(
                        "VirtualModel {} cannot fallback to itself",
                        model.id
                    ));
                }
                if !virtual_ids.contains(fallback_id.as_str()) {
                    return Err(format!(
                        "VirtualModel {} references missing fallback {}",
                        model.id, fallback_id
                    ));
                }
            }
        }

        Ok(())
    }
}

const fn default_proxy_port() -> u16 {
    DEFAULT_PROXY_PORT
}

#[derive(Clone)]
pub struct ConfigStore {
    config: Arc<RwLock<AppConfig>>,
    file_path: Option<PathBuf>,
}

impl ConfigStore {
    pub fn in_memory(initial_config: AppConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(initial_config)),
            file_path: None,
        }
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, String> {
        let path_buf = path.as_ref().to_path_buf();
        let config = if path_buf.exists() {
            let content = fs::read_to_string(&path_buf)
                .map_err(|error| format!("Failed to read config: {error}"))?;
            serde_json::from_str::<AppConfig>(&content)
                .map_err(|error| format!("Failed to parse config: {error}"))?
        } else {
            AppConfig::default()
        };
        config.validate()?;

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            file_path: Some(path_buf),
        })
    }

    pub fn get_config(&self) -> AppConfig {
        self.config.read().unwrap().clone()
    }

    pub fn update_config(&self, new_config: AppConfig) -> Result<(), String> {
        new_config.validate()?;
        let mut guard = self.config.write().unwrap();
        self.persist_config(&new_config)?;
        *guard = new_config;
        Ok(())
    }

    pub fn update_config_with<F>(&self, update: F) -> Result<AppConfig, String>
    where
        F: FnOnce(&mut AppConfig),
    {
        let mut guard = self.config.write().unwrap();
        let mut new_config = guard.clone();
        update(&mut new_config);
        new_config.validate()?;
        self.persist_config(&new_config)?;
        *guard = new_config.clone();
        Ok(new_config)
    }

    fn persist_config(&self, config: &AppConfig) -> Result<(), String> {
        let Some(path) = &self.file_path else {
            return Ok(());
        };
        let json_content = serde_json::to_string_pretty(config)
            .map_err(|error| format!("Failed to serialize config: {error}"))?;
        let temporary_path = path.with_extension("tmp");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create config directory: {error}"))?;
        }

        fs::write(&temporary_path, json_content)
            .map_err(|error| format!("Failed to write temporary config: {error}"))?;
        if let Err(error) = fs::rename(&temporary_path, path) {
            let _ = fs::remove_file(&temporary_path);
            return Err(format!("Failed to replace config file: {error}"));
        }
        Ok(())
    }
}

fn validate_id(kind: &str, id: &str) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err(format!("{kind} ID cannot be empty"));
    }
    Ok(())
}

fn validate_endpoint(label: &str, endpoint: &str, required: bool) -> Result<(), String> {
    if endpoint.trim().is_empty() {
        return if required {
            Err(format!("{label} cannot be empty"))
        } else {
            Ok(())
        };
    }

    let url =
        reqwest::Url::parse(endpoint).map_err(|error| format!("{label} is invalid: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("{label} must use http or https"));
    }
    if url.scheme() == "http" {
        let host = url
            .host_str()
            .ok_or_else(|| format!("{label} is missing a host"))?;
        let is_loopback = host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .map(|address| address.is_loopback())
                .unwrap_or(false);
        if !is_loopback {
            return Err(format!("{label} must use https unless it targets loopback"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ModelCapabilities, ParameterOverrides, ProviderProtocol};
    use std::collections::HashMap;

    fn sample_config() -> AppConfig {
        AppConfig {
            proxy_port: DEFAULT_PROXY_PORT,
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
        }
    }

    #[test]
    fn config_store_persists_and_reloads_valid_config() {
        let directory =
            std::env::temp_dir().join(format!("agy-byok-config-test-{}", uuid::Uuid::new_v4()));
        let path = directory.join("config.v1.json");
        let store = ConfigStore::load_from_file(&path).unwrap();
        assert!(store.get_config().providers.is_empty());

        store.update_config(sample_config()).unwrap();
        let reloaded = ConfigStore::load_from_file(&path).unwrap();

        assert_eq!(reloaded.get_config().providers[0].id, "provider-1");
        assert_eq!(reloaded.get_config().providers[0].api_key, "sk-test");
        assert!(!path.with_extension("tmp").exists());
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
    fn provider_api_key_defaults_to_empty() {
        let mut value = serde_json::to_value(sample_config()).unwrap();
        value["providers"][0]
            .as_object_mut()
            .unwrap()
            .remove("api_key");

        let config: AppConfig = serde_json::from_value(value).unwrap();

        assert!(config.providers[0].api_key.is_empty());
    }

    #[test]
    fn proxy_port_defaults_for_legacy_config_and_rejects_zero() {
        let mut value = serde_json::to_value(sample_config()).unwrap();
        value.as_object_mut().unwrap().remove("proxy_port");

        let mut config: AppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.proxy_port, DEFAULT_PROXY_PORT);
        assert!(config.validate().is_ok());

        config.proxy_port = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn config_validation_rejects_missing_references() {
        let mut config = sample_config();
        config.upstream_models[0].provider_id = "missing".to_string();

        let error = config.validate().unwrap_err();

        assert!(error.contains("missing Provider"));
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

        assert!(error.contains("custom-foo"));
    }
}
