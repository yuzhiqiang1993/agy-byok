use crate::domain::{ErrorCategory, ProxyError};
use serde_json::{json, Value};

pub struct CloudCodeEnvelopeEncoder;

impl CloudCodeEnvelopeEncoder {
    pub fn wrap_response(response_json: &str) -> Result<String, ProxyError> {
        let response: Value = serde_json::from_str(response_json).map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("Failed to encode Cloud Code response envelope: {error}"),
                500,
            )
        })?;
        Ok(Self::envelope(response).to_string())
    }

    pub fn wrap_stream_frame(frame: &str) -> Result<Option<String>, ProxyError> {
        let trimmed = frame.trim();
        if trimmed == "data: [DONE]" {
            return Ok(None);
        }

        let data = trimmed.strip_prefix("data:").ok_or_else(|| {
            ProxyError::new(
                ErrorCategory::Internal,
                "Antigravity stream frame is missing the data field",
                500,
            )
        })?;
        let response: Value = serde_json::from_str(data.trim()).map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("Failed to encode Cloud Code stream envelope: {error}"),
                500,
            )
        })?;
        Ok(Some(format!("data: {}\n\n", Self::envelope(response))))
    }

    fn envelope(response: Value) -> Value {
        json!({
            "response": response,
            "traceId": "",
            "metadata": {}
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_non_streaming_response() {
        let wrapped: Value = serde_json::from_str(
            &CloudCodeEnvelopeEncoder::wrap_response(r#"{"candidates":[{"index":0}]}"#).unwrap(),
        )
        .unwrap();

        assert_eq!(wrapped["response"]["candidates"][0]["index"], 0);
        assert_eq!(wrapped["traceId"], "");
        assert!(wrapped["metadata"].is_object());
    }

    #[test]
    fn wraps_stream_data_and_consumes_done_marker() {
        let wrapped = CloudCodeEnvelopeEncoder::wrap_stream_frame(
            "data: {\"candidates\":[{\"index\":2}]}\n\n",
        )
        .unwrap()
        .unwrap();
        assert!(wrapped.contains("\"response\""));
        assert!(wrapped.contains("\"index\":2"));
        assert_eq!(
            CloudCodeEnvelopeEncoder::wrap_stream_frame("data: [DONE]\n\n").unwrap(),
            None
        );
    }
}
