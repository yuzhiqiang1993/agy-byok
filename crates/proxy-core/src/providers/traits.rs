use crate::domain::{
    NeutralChatRequest, NeutralChatResponse, NeutralStreamEvent, Provider, ProxyError,
    UpstreamModel,
};
use crate::routing::ResolvedRoute;
use async_trait::async_trait;
use futures::Stream;
use std::collections::HashMap;
use std::pin::Pin;

pub type EventStream = Pin<Box<dyn Stream<Item = Result<NeutralStreamEvent, ProxyError>> + Send>>;

#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    /// 将 NeutralChatRequest 编码为对应 Provider 的 JSON Payload
    fn build_request_payload(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
    ) -> Result<serde_json::Value, ProxyError>;

    /// 构造包含 API Key 和自定义 Header 的 HTTP Headers
    fn build_headers(
        &self,
        provider: &Provider,
        api_key: &str,
    ) -> Result<HashMap<String, String>, ProxyError>;

    /// 解析上游非流式响应 Payload 为 NeutralChatResponse
    fn parse_response(
        &self,
        status: u16,
        body: &str,
        upstream_model: &UpstreamModel,
    ) -> Result<NeutralChatResponse, ProxyError>;

    /// 解析 SSE chunk 为 0~N 个 NeutralStreamEvent
    fn parse_stream_chunk(&self, chunk: &str) -> Result<Vec<NeutralStreamEvent>, ProxyError>;
}
