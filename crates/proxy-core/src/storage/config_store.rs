use crate::domain::{Provider, UpstreamModel, VirtualModel};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    pub providers: Vec<Provider>,
    pub upstream_models: Vec<UpstreamModel>,
    pub virtual_models: Vec<VirtualModel>,
}

impl AppConfig {
    pub fn validate(&self) -> Result<(), String> {
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
        for model in &self.virtual_models {
            validate_id("VirtualModel", &model.id)?;
            if !virtual_ids.insert(model.id.as_str()) {
                return Err(format!("Duplicate VirtualModel ID: {}", model.id));
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

        if let Some(ref path) = self.file_path {
            let json_content = serde_json::to_string_pretty(&new_config)
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
        }

        *guard = new_config;
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
            providers: vec![Provider {
                id: "provider-1".to_string(),
                name: "Provider".to_string(),
                protocol: ProviderProtocol::Openai,
                models_endpoint: "https://api.example.com/v1/models".to_string(),
                generate_endpoint: "https://api.example.com/v1/chat/completions".to_string(),
                api_key_ref: "key-1".to_string(),
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
        assert!(!path.with_extension("tmp").exists());
        let _ = fs::remove_dir_all(directory);
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
}
