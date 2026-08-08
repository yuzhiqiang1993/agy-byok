use super::{OfficialModelSettings, ParameterOverrides, Provider, UpstreamModel, VirtualModel};
use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::net::IpAddr;

pub const DEFAULT_PROXY_PORT: u16 = 12345;
pub const MIN_PROXY_PORT: u16 = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    InvalidValue(String),
    DuplicateId {
        kind: &'static str,
        id: String,
    },
    MissingReference {
        owner_kind: &'static str,
        owner_id: String,
        target_kind: &'static str,
        target_id: String,
    },
    IdentifierConflict {
        model_id: String,
        existing_model_id: String,
        identifier: String,
    },
    UnsupportedReasoning {
        model_id: String,
        level: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidValue(message) => formatter.write_str(message),
            Self::DuplicateId { kind, id } => write!(formatter, "Duplicate {kind} ID: {id}"),
            Self::MissingReference {
                owner_kind,
                owner_id,
                target_kind,
                target_id,
            } => write!(
                formatter,
                "{owner_kind} {owner_id} references missing {target_kind} {target_id}"
            ),
            Self::IdentifierConflict {
                model_id,
                existing_model_id,
                identifier,
            } => write!(
                formatter,
                "VirtualModel {model_id} identifier conflicts with VirtualModel {existing_model_id}: {identifier}"
            ),
            Self::UnsupportedReasoning { model_id, level } => write!(
                formatter,
                "VirtualModel {model_id} uses unsupported reasoning level {level}"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub proxy_port: u16,
    pub providers: Vec<Provider>,
    pub upstream_models: Vec<UpstreamModel>,
    pub virtual_models: Vec<VirtualModel>,
    pub official_model_settings: OfficialModelSettings,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            proxy_port: DEFAULT_PROXY_PORT,
            providers: Vec::new(),
            upstream_models: Vec::new(),
            virtual_models: Vec::new(),
            official_model_settings: OfficialModelSettings::default(),
        }
    }
}

impl AppConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.proxy_port < MIN_PROXY_PORT {
            return Err(ConfigError::InvalidValue(format!(
                "Proxy port must be between {MIN_PROXY_PORT} and 65535"
            )));
        }
        self.official_model_settings
            .validate()
            .map_err(ConfigError::InvalidValue)?;

        let mut provider_ids = HashSet::new();
        for provider in &self.providers {
            validate_id("Provider", &provider.id)?;
            if !provider_ids.insert(provider.id.as_str()) {
                return Err(ConfigError::DuplicateId {
                    kind: "Provider",
                    id: provider.id.clone(),
                });
            }
            if provider.name.trim().is_empty() {
                return Err(ConfigError::InvalidValue(format!(
                    "Provider {} has an empty name",
                    provider.id
                )));
            }
            validate_endpoint("Provider generate endpoint", &provider.generate_endpoint)?;
            validate_endpoint("Provider models endpoint", &provider.models_endpoint)?;
            if provider.connect_timeout_ms == 0
                || provider.request_timeout_ms == 0
                || provider.stream_idle_timeout_ms == 0
            {
                return Err(ConfigError::InvalidValue(format!(
                    "Provider {} timeouts must be greater than 0",
                    provider.id
                )));
            }
            if provider.connect_timeout_ms > provider.request_timeout_ms {
                return Err(ConfigError::InvalidValue(format!(
                    "Provider {} connect timeout cannot exceed request timeout",
                    provider.id
                )));
            }
            for (name, value) in &provider.headers {
                name.parse::<HeaderName>().map_err(|error| {
                    ConfigError::InvalidValue(format!(
                        "Provider {} header name is invalid: {error}",
                        provider.id
                    ))
                })?;
                value.parse::<HeaderValue>().map_err(|error| {
                    ConfigError::InvalidValue(format!(
                        "Provider {} header value for {name} is invalid: {error}",
                        provider.id
                    ))
                })?;
            }
            validate_parameters(
                &format!("Provider {} default parameters", provider.id),
                &provider.default_parameters,
            )?;
        }

        let mut upstream_ids = HashSet::new();
        for model in &self.upstream_models {
            validate_id("UpstreamModel", &model.id)?;
            if !upstream_ids.insert(model.id.as_str()) {
                return Err(ConfigError::DuplicateId {
                    kind: "UpstreamModel",
                    id: model.id.clone(),
                });
            }
            if !provider_ids.contains(model.provider_id.as_str()) {
                return Err(ConfigError::MissingReference {
                    owner_kind: "UpstreamModel",
                    owner_id: model.id.clone(),
                    target_kind: "Provider",
                    target_id: model.provider_id.clone(),
                });
            }
            if model.upstream_model_id.trim().is_empty() {
                return Err(ConfigError::InvalidValue(format!(
                    "UpstreamModel {} has an empty upstream model ID",
                    model.id
                )));
            }
            model.token_limits.validate().map_err(|error| {
                ConfigError::InvalidValue(format!("UpstreamModel {}: {error}", model.id))
            })?;
            if let Some(checkpoint_override) = &model.checkpoint_override {
                checkpoint_override.validate().map_err(|error| {
                    ConfigError::InvalidValue(format!("UpstreamModel {}: {error}", model.id))
                })?;
            }
            validate_parameters(
                &format!("UpstreamModel {} parameter overrides", model.id),
                &model.parameter_overrides,
            )?;
        }

        let mut virtual_ids = HashSet::new();
        let mut accepted_virtual_ids: HashMap<String, &str> = HashMap::new();
        for model in &self.virtual_models {
            validate_id("VirtualModel", &model.id)?;
            if !virtual_ids.insert(model.id.as_str()) {
                return Err(ConfigError::DuplicateId {
                    kind: "VirtualModel",
                    id: model.id.clone(),
                });
            }
            let host_model_id = model.effective_host_model_id().into_owned();
            validate_id("VirtualModel host model", &host_model_id)?;
            if !model.has_valid_host_model_id() {
                return Err(ConfigError::InvalidValue(format!(
                    "VirtualModel {} host model ID must match MODEL_PLACEHOLDER_M400..M599",
                    model.id
                )));
            }
            for accepted_id in model.accepted_ids() {
                if let Some(existing_model_id) = accepted_virtual_ids.get(accepted_id.as_ref()) {
                    if *existing_model_id != model.id.as_str() {
                        return Err(ConfigError::IdentifierConflict {
                            model_id: model.id.clone(),
                            existing_model_id: (*existing_model_id).to_string(),
                            identifier: accepted_id.into_owned(),
                        });
                    }
                } else {
                    accepted_virtual_ids.insert(accepted_id.into_owned(), model.id.as_str());
                }
            }
            let upstream = self
                .upstream_models
                .iter()
                .find(|upstream| upstream.id == model.upstream_model_id)
                .ok_or_else(|| ConfigError::MissingReference {
                    owner_kind: "VirtualModel",
                    owner_id: model.id.clone(),
                    target_kind: "UpstreamModel",
                    target_id: model.upstream_model_id.clone(),
                })?;
            if let Some(level) = model.default_reasoning_level {
                if upstream.capabilities.reasoning.mapping_for(level).is_none() {
                    return Err(ConfigError::UnsupportedReasoning {
                        model_id: model.id.clone(),
                        level: format!("{level:?}"),
                    });
                }
            }
            validate_parameters(
                &format!("VirtualModel {} parameter overrides", model.id),
                &model.parameter_overrides,
            )?;
        }

        for model in &self.virtual_models {
            if let Some(fallback_id) = &model.fallback_virtual_model_id {
                if fallback_id == &model.id {
                    return Err(ConfigError::InvalidValue(format!(
                        "VirtualModel {} cannot fallback to itself",
                        model.id
                    )));
                }
                if !virtual_ids.contains(fallback_id.as_str()) {
                    return Err(ConfigError::MissingReference {
                        owner_kind: "VirtualModel",
                        owner_id: model.id.clone(),
                        target_kind: "fallback",
                        target_id: fallback_id.clone(),
                    });
                }
            }
        }

        Ok(())
    }
}

fn validate_id(kind: &'static str, id: &str) -> Result<(), ConfigError> {
    if id.trim().is_empty() {
        return Err(ConfigError::InvalidValue(format!(
            "{kind} ID cannot be empty"
        )));
    }
    Ok(())
}

fn validate_endpoint(label: &str, endpoint: &str) -> Result<(), ConfigError> {
    if endpoint.trim().is_empty() {
        return Err(ConfigError::InvalidValue(format!(
            "{label} cannot be empty"
        )));
    }

    let url = reqwest::Url::parse(endpoint)
        .map_err(|error| ConfigError::InvalidValue(format!("{label} is invalid: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ConfigError::InvalidValue(format!(
            "{label} must use http or https"
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ConfigError::InvalidValue(format!(
            "{label} must not contain embedded credentials"
        )));
    }
    if url.fragment().is_some() {
        return Err(ConfigError::InvalidValue(format!(
            "{label} must not contain a URL fragment"
        )));
    }
    if url.scheme() == "http" {
        let host = url
            .host_str()
            .ok_or_else(|| ConfigError::InvalidValue(format!("{label} is missing a host")))?;
        let is_loopback = host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .map(|address| address.is_loopback())
                .unwrap_or(false);
        if !is_loopback {
            return Err(ConfigError::InvalidValue(format!(
                "{label} must use https unless it targets loopback"
            )));
        }
    }
    Ok(())
}

fn validate_parameters(label: &str, parameters: &ParameterOverrides) -> Result<(), ConfigError> {
    if parameters
        .temperature
        .is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        return Err(ConfigError::InvalidValue(format!(
            "{label} temperature must be finite and non-negative"
        )));
    }
    if parameters
        .top_p
        .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
    {
        return Err(ConfigError::InvalidValue(format!(
            "{label} top_p must be between 0 and 1"
        )));
    }
    if parameters.max_tokens == Some(0) {
        return Err(ConfigError::InvalidValue(format!(
            "{label} max_tokens must be greater than 0"
        )));
    }
    if parameters.top_k == Some(0) {
        return Err(ConfigError::InvalidValue(format!(
            "{label} top_k must be greater than 0"
        )));
    }
    Ok(())
}
