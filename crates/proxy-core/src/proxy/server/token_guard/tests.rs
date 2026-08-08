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
            connect_timeout_ms: 5_000,
            request_timeout_ms: 60_000,
            stream_idle_timeout_ms: 30_000,
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
