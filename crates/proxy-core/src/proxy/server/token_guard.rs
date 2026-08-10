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
    if request_contains_inline_data(request) {
        tracing::debug!(
            model_id = %route.upstream_model.upstream_model_id,
            "Skipping local token hard check because inline media token accounting is unavailable"
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

fn request_contains_inline_data(request: &NeutralChatRequest) -> bool {
    request.messages.iter().any(|message| {
        message
            .blocks
            .iter()
            .any(|block| matches!(block, NeutralContentBlock::InlineData { .. }))
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
        NeutralContentBlock::InlineData { .. } => 0,
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
mod tests;
