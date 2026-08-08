use super::*;
use crate::domain::ProxyError;

#[test]
fn classifies_openai_compatible_context_limit_identifiers() {
    for (status, body) in [
        (
            400,
            r#"{"error":{"code":"context_length_exceeded","type":"invalid_request_error","message":"Request rejected"}}"#,
        ),
        (
            413,
            r#"{"error":{"type":"too_many_tokens","message":"Request rejected"}}"#,
        ),
        (
            422,
            r#"{"error":{"status":"CONTEXT_WINDOW_EXCEEDED","message":"Request rejected"}}"#,
        ),
    ] {
        assert_eq!(
            classify_response_error(status, body),
            ErrorCategory::ContextLengthExceeded,
            "body: {body}"
        );
    }
}

#[test]
fn classifies_anthropic_prompt_token_overflow() {
    let body = r#"{
        "type":"error",
        "error":{
            "type":"invalid_request_error",
            "message":"prompt is too long: 213079 tokens > 200000 maximum"
        }
    }"#;

    assert_eq!(
        classify_response_error(400, body),
        ErrorCategory::ContextLengthExceeded
    );
}

#[test]
fn classifies_gemini_input_token_overflow() {
    let body = r#"{
        "error":{
            "code":400,
            "message":"The input token count (1049575) exceeds the maximum number of tokens allowed (1048576).",
            "status":"INVALID_ARGUMENT"
        }
    }"#;

    assert_eq!(
        classify_response_error(400, body),
        ErrorCategory::ContextLengthExceeded
    );
}

#[test]
fn ordinary_413_errors_remain_invalid_requests() {
    for body in [
        r#"{"error":{"type":"request_too_large","message":"Payload too large"}}"#,
        r#"{"type":"error","error":{"type":"request_too_large","message":"Request exceeds the maximum allowed number of bytes"}}"#,
        r#"{"error":{"code":413,"message":"Request payload size exceeds the 20 MB limit","status":"RESOURCE_EXHAUSTED"}}"#,
    ] {
        assert_eq!(
            classify_response_error(413, body),
            ErrorCategory::InvalidRequest,
            "body: {body}"
        );
    }
}

#[test]
fn output_token_parameter_errors_remain_invalid_requests() {
    for body in [
        r#"{"error":{"code":"max_tokens_exceeded","message":"max_tokens exceeds the maximum allowed value"}}"#,
        r#"{"error":{"code":"reasoning_tokens_exceeded","message":"reasoning token budget exceeds the output limit"}}"#,
    ] {
        assert_eq!(
            classify_response_error(400, body),
            ErrorCategory::InvalidRequest,
            "body: {body}"
        );
    }
}

#[test]
fn preserves_existing_special_status_categories() {
    let explicit_context_error = r#"{"error":{"code":"context_length_exceeded"}}"#;

    for (status, expected) in [
        (401, ErrorCategory::Authentication),
        (403, ErrorCategory::Authentication),
        (404, ErrorCategory::ModelNotFound),
        (429, ErrorCategory::RateLimit),
        (500, ErrorCategory::UpstreamServerError),
        (599, ErrorCategory::UpstreamServerError),
    ] {
        assert_eq!(
            classify_response_error(status, explicit_context_error),
            expected
        );
    }
}

#[test]
fn context_length_errors_are_not_retryable_for_fallback() {
    let error = ProxyError::new(ErrorCategory::ContextLengthExceeded, "too many tokens", 400);

    assert!(!error.is_retryable_for_fallback());
}
