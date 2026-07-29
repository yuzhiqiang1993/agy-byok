use crate::domain::{
    NeutralChatRequest, NeutralChatResponse, NeutralStreamEvent, Provider, ProxyError,
    UpstreamModel,
};
use crate::routing::ResolvedRoute;
use async_trait::async_trait;
use std::collections::HashMap;

pub trait ProviderStreamDecoder: Send {
    fn decode_data(&mut self, data: &str) -> Result<Vec<NeutralStreamEvent>, ProxyError>;

    fn finish(&mut self) -> Result<Vec<NeutralStreamEvent>, ProxyError>;
}

#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    /// 将 NeutralChatRequest 编码为对应 Provider 的 JSON Payload
    fn build_request_payload(
        &self,
        route: &ResolvedRoute,
        request: &NeutralChatRequest,
    ) -> Result<serde_json::Value, ProxyError>;

    /// 根据 Provider 配置构造 API Key 和自定义 Header
    fn build_headers(&self, provider: &Provider) -> Result<HashMap<String, String>, ProxyError>;

    /// 解析上游非流式响应 Payload 为 NeutralChatResponse
    fn parse_response(
        &self,
        status: u16,
        body: &str,
        upstream_model: &UpstreamModel,
    ) -> Result<NeutralChatResponse, ProxyError>;

    /// 为单次上游请求创建独立的有状态流解码器
    fn create_stream_decoder(
        &self,
        upstream_model: &UpstreamModel,
    ) -> Box<dyn ProviderStreamDecoder>;
}
