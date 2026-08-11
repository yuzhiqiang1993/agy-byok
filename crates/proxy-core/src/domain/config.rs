use super::{
    is_supported_inline_image_mime_type, ModelCompressionPolicy, ParameterOverrides, Provider,
    ProviderProtocol, UpstreamModel, VirtualModel,
};
use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
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

use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomHostPaths {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ide: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub proxy_port: u16,
    pub providers: Vec<Provider>,
    pub upstream_models: Vec<UpstreamModel>,
    pub virtual_models: Vec<VirtualModel>,
    pub model_compression_policies: BTreeMap<String, ModelCompressionPolicy>,
    #[serde(default)]
    pub disabled_official_models: HashSet<String>,
    #[serde(default)]
    pub custom_host_paths: CustomHostPaths,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            proxy_port: DEFAULT_PROXY_PORT,
            providers: Vec::new(),
            upstream_models: Vec::new(),
            virtual_models: Vec::new(),
            model_compression_policies: BTreeMap::new(),
            disabled_official_models: HashSet::new(),
            custom_host_paths: CustomHostPaths::default(),
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
        for (model_id, policy) in &self.model_compression_policies {
            if model_id.trim().is_empty() {
                return Err(ConfigError::InvalidValue(
                    "model_compression_policies cannot contain an empty model ID".to_string(),
                ));
            }
            policy
                .validate(&format!("model_compression_policies[{model_id}]"))
                .map_err(ConfigError::InvalidValue)?;
        }

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
            let provider = self
                .providers
                .iter()
                .find(|provider| provider.id == model.provider_id)
                .expect("validated provider reference must exist");
            validate_model_media_capabilities(model, &provider.protocol)?;
            validate_model_reasoning_capabilities(model, &provider.protocol)?;
            if model.upstream_model_id.trim().is_empty() {
                return Err(ConfigError::InvalidValue(format!(
                    "UpstreamModel {} has an empty upstream model ID",
                    model.id
                )));
            }

            model.token_limits.validate().map_err(|error| {
                ConfigError::InvalidValue(format!("UpstreamModel {}: {error}", model.id))
            })?;
            if let Some(compression_policy) = &model.compression_policy {
                compression_policy
                    .validate(&format!("UpstreamModel {} compression_policy", model.id))
                    .map_err(ConfigError::InvalidValue)?;
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

fn validate_model_media_capabilities(
    model: &UpstreamModel,
    protocol: &ProviderProtocol,
) -> Result<(), ConfigError> {
    let mut seen = HashSet::new();
    let mut has_image = false;
    for mime_type in &model.capabilities.supported_mime_types {
        let normalized = mime_type.trim().to_ascii_lowercase();
        if normalized.is_empty() || !normalized.contains('/') {
            return Err(ConfigError::InvalidValue(format!(
                "UpstreamModel {} has invalid supported MIME type {mime_type:?}",
                model.id
            )));
        }
        if !seen.insert(normalized.clone()) {
            return Err(ConfigError::InvalidValue(format!(
                "UpstreamModel {} has duplicate supported MIME type {normalized}",
                model.id
            )));
        }
        if normalized.starts_with("image/") {
            has_image = true;
        }
        if !matches!(protocol, ProviderProtocol::GeminiGenerateContent)
            && !is_supported_inline_image_mime_type(&normalized)
        {
            return Err(ConfigError::InvalidValue(format!(
                "UpstreamModel {} cannot use inline MIME type {normalized} with provider protocol {:?}",
                model.id, protocol
            )));
        }
    }
    if model.capabilities.vision != has_image {
        return Err(ConfigError::InvalidValue(format!(
            "UpstreamModel {} vision capability must match its image MIME types",
            model.id
        )));
    }
    Ok(())
}

fn validate_model_reasoning_capabilities(
    model: &UpstreamModel,
    protocol: &ProviderProtocol,
) -> Result<(), ConfigError> {
    let reasoning = &model.capabilities.reasoning;
    let has_model_budget =
        reasoning.thinking_budget.is_some() || reasoning.min_thinking_budget.is_some();
    if reasoning.thinking_budget.is_some_and(|budget| budget < -1) {
        return Err(ConfigError::InvalidValue(format!(
            "UpstreamModel {} thinking_budget must be -1, 0, or a positive integer",
            model.id
        )));
    }
    if has_model_budget && !matches!(protocol, ProviderProtocol::GeminiGenerateContent) {
        return Err(ConfigError::InvalidValue(format!(
            "UpstreamModel {} can use model-level thinking budgets only with Gemini",
            model.id
        )));
    }
    if reasoning.supported == Some(false) && (has_model_budget || !reasoning.levels.is_empty()) {
        return Err(ConfigError::InvalidValue(format!(
            "UpstreamModel {} cannot disable reasoning while keeping reasoning configuration",
            model.id
        )));
    }
    if reasoning.min_thinking_budget == Some(0) {
        return Err(ConfigError::InvalidValue(format!(
            "UpstreamModel {} min_thinking_budget must be greater than 0",
            model.id
        )));
    }
    if let (Some(minimum), Some(default)) =
        (reasoning.min_thinking_budget, reasoning.thinking_budget)
    {
        if default == 0 || (default > 0 && i64::from(minimum) > i64::from(default)) {
            return Err(ConfigError::InvalidValue(format!(
                "UpstreamModel {} min_thinking_budget must not exceed thinking_budget",
                model.id
            )));
        }
    }
    if let Some(minimum) = reasoning.min_thinking_budget {
        for mapping in reasoning.levels.values() {
            if let crate::domain::ReasoningMapping::BudgetTokens(tokens) = mapping {
                if *tokens < minimum {
                    return Err(ConfigError::InvalidValue(format!(
                        "UpstreamModel {} reasoning budget {tokens} is below min_thinking_budget {minimum}",
                        model.id
                    )));
                }
            }
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ModelCapabilities, ModelTokenLimits, ReasoningCapability};

    fn media_config(protocol: ProviderProtocol, mime_types: &[&str]) -> AppConfig {
        let provider_id = "provider".to_string();
        AppConfig {
            providers: vec![Provider {
                id: provider_id.clone(),
                name: "Provider".to_string(),
                protocol,
                models_endpoint: "http://127.0.0.1/models".to_string(),
                generate_endpoint: "http://127.0.0.1/generate".to_string(),
                api_key: String::new(),
                headers: HashMap::new(),
                default_parameters: ParameterOverrides::default(),
                connect_timeout_ms: 1_000,
                request_timeout_ms: 2_000,
                stream_idle_timeout_ms: 1_000,
                enabled: true,
            }],
            upstream_models: vec![UpstreamModel {
                id: "upstream".to_string(),
                provider_id,
                upstream_model_id: "model".to_string(),
                display_name: "Model".to_string(),
                capabilities: ModelCapabilities {
                    vision: false,
                    tools: true,
                    supported_mime_types: mime_types
                        .iter()
                        .map(|mime_type| (*mime_type).to_string())
                        .collect(),
                    reasoning: ReasoningCapability::default(),
                },
                token_limits: ModelTokenLimits::default(),
                compression_policy: None,
                tokenizer: None,
                parameter_overrides: ParameterOverrides::default(),
                enabled: true,
            }],
            ..AppConfig::default()
        }
    }

    #[test]
    fn video_mime_requires_gemini_protocol() {
        assert!(
            media_config(ProviderProtocol::GeminiGenerateContent, &["video/mp4"])
                .validate()
                .is_ok()
        );

        let error = media_config(ProviderProtocol::OpenaiChatCompletions, &["video/mp4"])
            .validate()
            .unwrap_err();
        assert!(error.to_string().contains("video/mp4"));
    }

    #[test]
    fn unsupported_image_mime_requires_gemini_protocol() {
        let mut gemini_config =
            media_config(ProviderProtocol::GeminiGenerateContent, &["image/heic"]);
        gemini_config.upstream_models[0].capabilities.vision = true;
        assert!(gemini_config.validate().is_ok());

        let error = media_config(ProviderProtocol::AnthropicMessages, &["image/heic"])
            .validate()
            .unwrap_err();
        assert!(error.to_string().contains("image/heic"));
    }

    #[test]
    fn vision_requires_an_explicit_image_mime_type() {
        let mut config = media_config(ProviderProtocol::GeminiGenerateContent, &[]);
        config.upstream_models[0].capabilities.vision = true;

        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("vision capability"));
    }

    #[test]
    fn minimum_thinking_budget_cannot_exceed_default_budget() {
        let mut config = media_config(ProviderProtocol::GeminiGenerateContent, &[]);
        config.upstream_models[0]
            .capabilities
            .reasoning
            .thinking_budget = Some(128);
        config.upstream_models[0]
            .capabilities
            .reasoning
            .min_thinking_budget = Some(256);

        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("min_thinking_budget"));
    }

    #[test]
    fn model_thinking_budget_requires_gemini() {
        let mut config = media_config(ProviderProtocol::AnthropicMessages, &[]);
        config.upstream_models[0]
            .capabilities
            .reasoning
            .thinking_budget = Some(10_001);

        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("only with Gemini"));
    }

    #[test]
    fn disabled_reasoning_rejects_budget_configuration() {
        let mut config = media_config(ProviderProtocol::GeminiGenerateContent, &[]);
        let reasoning = &mut config.upstream_models[0].capabilities.reasoning;
        reasoning.supported = Some(false);
        reasoning.thinking_budget = Some(10_001);

        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("cannot disable reasoning"));
    }

    #[test]
    fn dynamic_thinking_budget_accepts_a_positive_minimum() {
        let mut config = media_config(ProviderProtocol::GeminiGenerateContent, &[]);
        let reasoning = &mut config.upstream_models[0].capabilities.reasoning;
        reasoning.supported = Some(true);
        reasoning.thinking_budget = Some(-1);
        reasoning.min_thinking_budget = Some(128);

        assert!(config.validate().is_ok());
    }

    #[test]
    fn thinking_budget_rejects_values_below_dynamic_sentinel() {
        let mut config = media_config(ProviderProtocol::GeminiGenerateContent, &[]);
        config.upstream_models[0]
            .capabilities
            .reasoning
            .thinking_budget = Some(-2);

        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("must be -1, 0"));
    }
}
