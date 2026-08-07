use crate::domain::{
    ErrorCategory, MessageRole, NeutralChatRequest, NeutralContentBlock, ProviderProtocol,
    ProxyError, TiktokenEncoding, TokenLimitSource, TokenizerConfig,
};
use crate::routing::ResolvedRoute;
use serde_json::Value;
use std::collections::HashMap;
use tiktoken_rs::{cl100k_base_singleton, o200k_base_singleton, CoreBPE};

const CHAT_PRIMING_TOKENS: u64 = 3;
const MESSAGE_OVERHEAD_TOKENS: u64 = 4;
const STRUCTURED_BLOCK_OVERHEAD_TOKENS: u64 = 8;
const TOOL_DEFINITION_OVERHEAD_TOKENS: u64 = 8;
const SAFETY_MARGIN_PERCENT: u64 = 5;
const MINIMUM_SAFETY_MARGIN_TOKENS: u64 = 256;

pub(super) async fn validate_request(
    route: &ResolvedRoute,
    request: &NeutralChatRequest,
) -> Result<(), ProxyError> {
    if !matches!(
        route.provider.protocol,
        ProviderProtocol::OpenaiChatCompletions | ProviderProtocol::OpenaiResponses
    ) {
        return Ok(());
    }

    let limits = &route.upstream_model.token_limits;
    let Some(input_limit) = limits.input_token_limit else {
        return Ok(());
    };
    if !matches!(
        limits.input_token_limit_source,
        TokenLimitSource::Catalog | TokenLimitSource::Configured
    ) {
        return Ok(());
    }
    let Some(TokenizerConfig::Tiktoken { encoding }) = route.upstream_model.tokenizer else {
        return Ok(());
    };
    if request_contains_image(request) {
        tracing::debug!(
            model_id = %route.upstream_model.upstream_model_id,
            "Skipping local token hard check because image token accounting is unavailable"
        );
        return Ok(());
    }

    let request = request.clone();
    let extra_body = route.final_parameters.extra_body.clone();
    let protocol = route.provider.protocol.clone();
    let raw_input_tokens = tokio::task::spawn_blocking(move || {
        count_input_tokens(&request, extra_body.as_ref(), encoding, &protocol)
    })
    .await
    .map_err(|error| {
        ProxyError::new(
            ErrorCategory::Internal,
            format!("Local tokenizer task failed: {error}"),
            500,
        )
    })?;
    let protected_input_tokens = with_safety_margin(raw_input_tokens);

    tracing::debug!(
        model_id = %route.upstream_model.upstream_model_id,
        tokenizer = encoding_name(encoding),
        raw_input_tokens,
        protected_input_tokens,
        input_token_limit = input_limit,
        limit_source = ?limits.input_token_limit_source,
        "Completed local input token preflight"
    );

    if protected_input_tokens > u64::from(input_limit) {
        return Err(ProxyError::new(
            ErrorCategory::ContextLengthExceeded,
            format!(
                "Local token preflight rejected model {}: calibrated input estimate {} tokens ({} with safety margin) exceeds authoritative input limit {} using {}",
                route.upstream_model.upstream_model_id,
                raw_input_tokens,
                protected_input_tokens,
                input_limit,
                encoding_name(encoding),
            ),
            400,
        ));
    }

    Ok(())
}

fn request_contains_image(request: &NeutralChatRequest) -> bool {
    request.messages.iter().any(|message| {
        message
            .blocks
            .iter()
            .any(|block| matches!(block, NeutralContentBlock::Image { .. }))
    })
}

fn count_input_tokens(
    request: &NeutralChatRequest,
    extra_body: Option<&HashMap<String, Value>>,
    encoding: TiktokenEncoding,
    protocol: &ProviderProtocol,
) -> u64 {
    let bpe = tokenizer(encoding);
    let mut tokens = CHAT_PRIMING_TOKENS;

    if let Some(system_instruction) = &request.system_instruction {
        tokens = tokens
            .saturating_add(MESSAGE_OVERHEAD_TOKENS)
            .saturating_add(count_text(bpe, "system"))
            .saturating_add(count_text(bpe, system_instruction));
    }

    for message in &request.messages {
        tokens = tokens
            .saturating_add(MESSAGE_OVERHEAD_TOKENS)
            .saturating_add(count_text(bpe, role_name(&message.role)));
        for block in &message.blocks {
            tokens = tokens.saturating_add(count_block(bpe, block, protocol));
        }
    }

    for tool in &request.tools {
        let serialized = serde_json::to_string(tool).unwrap_or_default();
        tokens = tokens
            .saturating_add(TOOL_DEFINITION_OVERHEAD_TOKENS)
            .saturating_add(count_text(bpe, &serialized));
    }

    if let Some(extra_body) = extra_body {
        for (key, value) in extra_body {
            tokens = tokens
                .saturating_add(STRUCTURED_BLOCK_OVERHEAD_TOKENS)
                .saturating_add(count_text(bpe, key))
                .saturating_add(count_text(bpe, &value.to_string()));
        }
    }

    tokens
}

fn count_block(bpe: &CoreBPE, block: &NeutralContentBlock, protocol: &ProviderProtocol) -> u64 {
    match block {
        NeutralContentBlock::Text(text) => count_text(bpe, text),
        NeutralContentBlock::Image { .. } => 0,
        NeutralContentBlock::ToolCall {
            id,
            name,
            arguments_json,
        } => STRUCTURED_BLOCK_OVERHEAD_TOKENS
            .saturating_add(count_text(bpe, id))
            .saturating_add(count_text(bpe, name))
            .saturating_add(count_text(bpe, arguments_json)),
        NeutralContentBlock::ToolResult {
            tool_call_id,
            name,
            content,
        } => STRUCTURED_BLOCK_OVERHEAD_TOKENS
            .saturating_add(count_text(bpe, tool_call_id))
            .saturating_add(name.as_deref().map_or(0, |name| count_text(bpe, name)))
            .saturating_add(count_text(bpe, content)),
        NeutralContentBlock::Thinking { text, .. } => {
            if matches!(protocol, ProviderProtocol::OpenaiResponses) {
                0
            } else {
                STRUCTURED_BLOCK_OVERHEAD_TOKENS.saturating_add(count_text(bpe, text))
            }
        }
    }
}

fn tokenizer(encoding: TiktokenEncoding) -> &'static CoreBPE {
    match encoding {
        TiktokenEncoding::Cl100kBase => cl100k_base_singleton(),
        TiktokenEncoding::O200kBase => o200k_base_singleton(),
    }
}

fn count_text(bpe: &CoreBPE, text: &str) -> u64 {
    bpe.encode_with_special_tokens(text).len() as u64
}

fn role_name(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
}

fn with_safety_margin(tokens: u64) -> u64 {
    let percentage = tokens
        .saturating_mul(SAFETY_MARGIN_PERCENT)
        .saturating_add(99)
        / 100;
    tokens.saturating_add(percentage.max(MINIMUM_SAFETY_MARGIN_TOKENS))
}

fn encoding_name(encoding: TiktokenEncoding) -> &'static str {
    match encoding {
        TiktokenEncoding::Cl100kBase => "cl100k_base",
        TiktokenEncoding::O200kBase => "o200k_base",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        ModelCapabilities, ModelTokenLimits, NeutralMessage, NeutralTool, NeutralToolFunction,
        ParameterOverrides, Provider, UpstreamModel, VirtualModel,
    };
    use serde_json::json;

    fn route(source: TokenLimitSource, input_limit: u32) -> ResolvedRoute {
        ResolvedRoute {
            virtual_model: VirtualModel {
                id: "virtual".to_string(),
                host_model_id: None,
                upstream_model_id: "upstream".to_string(),
                display_name: "Virtual".to_string(),
                default_reasoning_level: None,
                parameter_overrides: ParameterOverrides::default(),
                fallback_virtual_model_id: None,
                enabled: true,
            },
            upstream_model: UpstreamModel {
                id: "upstream".to_string(),
                provider_id: "provider".to_string(),
                upstream_model_id: "gpt-test".to_string(),
                display_name: "GPT Test".to_string(),
                capabilities: ModelCapabilities::default(),
                token_limits: ModelTokenLimits {
                    context_window: None,
                    context_window_source: TokenLimitSource::Unknown,
                    input_token_limit: Some(input_limit),
                    input_token_limit_source: source,
                    output_token_limit: None,
                    output_token_limit_source: TokenLimitSource::Unknown,
                },
                checkpoint_override: None,
                tokenizer: Some(TokenizerConfig::Tiktoken {
                    encoding: TiktokenEncoding::O200kBase,
                }),
                parameter_overrides: ParameterOverrides::default(),
                enabled: true,
            },
            provider: Provider {
                id: "provider".to_string(),
                name: "Provider".to_string(),
                protocol: ProviderProtocol::OpenaiChatCompletions,
                models_endpoint: String::new(),
                generate_endpoint: "https://example.com/v1/chat/completions".to_string(),
                api_key: String::new(),
                headers: HashMap::new(),
                default_parameters: ParameterOverrides::default(),
                connect_timeout_ms: 0,
                request_timeout_ms: 0,
                stream_idle_timeout_ms: 0,
                enabled: true,
            },
            final_parameters: ParameterOverrides::default(),
            final_reasoning_level: None,
        }
    }

    fn request_with_blocks(blocks: Vec<NeutralContentBlock>) -> NeutralChatRequest {
        NeutralChatRequest {
            virtual_model_id: "virtual".to_string(),
            messages: vec![NeutralMessage {
                role: MessageRole::User,
                blocks,
            }],
            system_instruction: Some("Follow the user request.".to_string()),
            tools: Vec::new(),
            reasoning_level: None,
            stream: false,
            generation_parameters: ParameterOverrides::default(),
            extra_body: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn rejects_oversized_text_when_limit_and_tokenizer_are_authoritative() {
        let request = request_with_blocks(vec![NeutralContentBlock::Text("token ".repeat(2_000))]);
        let raw = count_input_tokens(
            &request,
            None,
            TiktokenEncoding::O200kBase,
            &ProviderProtocol::OpenaiChatCompletions,
        );
        let limit = u32::try_from(with_safety_margin(raw) - 1).unwrap();
        let error = validate_request(&route(TokenLimitSource::Catalog, limit), &request)
            .await
            .unwrap_err();

        assert_eq!(error.category, ErrorCategory::ContextLengthExceeded);
        assert!(error.message.contains("safety margin"));
    }

    #[tokio::test]
    async fn estimated_limits_do_not_hard_reject_requests() {
        let request = request_with_blocks(vec![NeutralContentBlock::Text("token ".repeat(2_000))]);

        validate_request(&route(TokenLimitSource::Estimated, 1), &request)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn image_requests_skip_local_hard_check() {
        let request = request_with_blocks(vec![NeutralContentBlock::Image {
            mime_type: "image/png".to_string(),
            data_base64: "large-image-data".to_string(),
        }]);

        validate_request(&route(TokenLimitSource::Configured, 1), &request)
            .await
            .unwrap();
    }

    #[test]
    fn tool_definitions_are_included_in_input_accounting() {
        let plain = request_with_blocks(vec![NeutralContentBlock::Text("Hello".to_string())]);
        let mut with_tool = plain.clone();
        with_tool.tools.push(NeutralTool {
            function: NeutralToolFunction {
                name: "lookup".to_string(),
                description: Some("Look up a record".to_string()),
                parameters_schema: json!({
                    "type": "object",
                    "properties": {"query": {"type": "string"}}
                }),
            },
        });

        let plain_tokens = count_input_tokens(
            &plain,
            None,
            TiktokenEncoding::Cl100kBase,
            &ProviderProtocol::OpenaiChatCompletions,
        );
        let tool_tokens = count_input_tokens(
            &with_tool,
            None,
            TiktokenEncoding::Cl100kBase,
            &ProviderProtocol::OpenaiChatCompletions,
        );

        assert!(tool_tokens > plain_tokens);
    }
}
