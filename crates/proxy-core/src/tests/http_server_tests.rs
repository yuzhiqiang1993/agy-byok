#[cfg(test)]
mod tests {
    use crate::domain::*;
    use crate::proxy::{HttpServerOptions, LoopbackHttpServer, ProxyServer};
    use crate::storage::{AppConfig, ConfigStore};
    use crate::tests::mock_provider::MockProviderServer;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    fn test_options() -> HttpServerOptions {
        HttpServerOptions {
            graceful_shutdown_timeout: Duration::from_secs(1),
            ..HttpServerOptions::default()
        }
    }

    fn model_config(generate_endpoint: String) -> AppConfig {
        AppConfig {
            providers: vec![Provider {
                id: "provider-1".to_string(),
                name: "Mock Provider".to_string(),
                protocol: ProviderProtocol::Openai,
                models_endpoint: "http://127.0.0.1/models".to_string(),
                generate_endpoint,
                api_key: "sk-test".to_string(),
                headers: HashMap::new(),
                default_parameters: ParameterOverrides::default(),
                connect_timeout_ms: 3000,
                request_timeout_ms: 5000,
                stream_idle_timeout_ms: 5000,
                enabled: true,
            }],
            upstream_models: vec![UpstreamModel {
                id: "upstream-1".to_string(),
                provider_id: "provider-1".to_string(),
                upstream_model_id: "gpt-test".to_string(),
                display_name: "GPT Test".to_string(),
                capabilities: ModelCapabilities {
                    vision: false,
                    tools: true,
                    reasoning: ReasoningCapability::default(),
                },
                parameter_overrides: ParameterOverrides::default(),
                enabled: true,
            }],
            virtual_models: vec![VirtualModel {
                id: "virtual-1".to_string(),
                host_model_id: None,
                upstream_model_id: "upstream-1".to_string(),
                display_name: "Virtual Test".to_string(),
                default_reasoning_level: None,
                parameter_overrides: ParameterOverrides::default(),
                fallback_virtual_model_id: None,
                enabled: true,
            }],
        }
    }

    async fn create_proxy(config: AppConfig, port: u16) -> (Arc<ProxyServer>, String) {
        let proxy = Arc::new(ProxyServer::new(ConfigStore::in_memory(config), port));
        let token = proxy.auth_manager().get_token().to_string();
        (proxy, token)
    }

    #[tokio::test]
    async fn loopback_server_exposes_safe_health_and_protected_models() {
        let config = model_config("http://127.0.0.1/generate".to_string());
        let (proxy, token) = create_proxy(config, 0).await;
        let handle = LoopbackHttpServer::start(proxy, test_options())
            .await
            .unwrap();
        let base_url = format!("http://{}", handle.local_addr());
        let client = reqwest::Client::new();

        let health = client
            .get(format!("{base_url}/health"))
            .send()
            .await
            .unwrap();
        assert_eq!(health.status(), reqwest::StatusCode::OK);
        let health_json: serde_json::Value = health.json().await.unwrap();
        assert_eq!(health_json["product"], "agy-byok");
        assert!(health_json.get("providers").is_none());
        assert!(health_json.get("token").is_none());

        let unauthorized = client
            .get(format!("{base_url}/v1/models"))
            .send()
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

        let host_catalog = client
            .post(format!("{base_url}/v1internal:fetchAvailableModels"))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(host_catalog.status(), reqwest::StatusCode::OK);

        let models = client
            .get(format!("{base_url}/v1/models"))
            .header("x-agy-byok-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(models.status(), reqwest::StatusCode::OK);
        let models_json: serde_json::Value = models.json().await.unwrap();
        assert_eq!(models_json["models"][0]["id"], "virtual-1");

        drop(client);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn loopback_server_enforces_body_limit_and_port_binding() {
        let (proxy, token) = create_proxy(AppConfig::default(), 0).await;
        let mut options = test_options();
        options.max_body_bytes = 32;
        let handle = LoopbackHttpServer::start(proxy, options.clone())
            .await
            .unwrap();
        let client = reqwest::Client::new();
        let response = client
            .post(format!(
                "http://{}/v1internal:generateContent",
                handle.local_addr()
            ))
            .header("x-agy-byok-token", token)
            .body("x".repeat(33))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);

        let occupied_port = handle.local_addr().port();
        let (second_proxy, _) = create_proxy(AppConfig::default(), occupied_port).await;
        let error = LoopbackHttpServer::start(second_proxy, options)
            .await
            .err()
            .expect("occupied port must fail");
        assert_eq!(error.category, ErrorCategory::ConnectionFailed);

        drop(client);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn loopback_server_handles_non_streaming_generation() {
        let upstream_body = json!({
            "id": "response-1",
            "model": "gpt-test",
            "choices": [{
                "index": 0,
                "message": { "content": "HTTP response" },
                "finish_reason": "stop"
            }]
        })
        .to_string();
        let (mock_url, _mock_handle) = MockProviderServer::start(200, &upstream_body).await;
        let (proxy, token) =
            create_proxy(model_config(format!("{mock_url}/v1/chat/completions")), 0).await;
        let handle = LoopbackHttpServer::start(proxy, test_options())
            .await
            .unwrap();
        let client = reqwest::Client::new();

        let response = client
            .post(format!(
                "http://{}/v1internal:generateContent",
                handle.local_addr()
            ))
            .header("x-agy-byok-token", token)
            .json(&json!({
                "project": "antigravity-internal-project",
                "requestId": "request-1",
                "model": "virtual-1",
                "request": {
                    "contents": [{ "role": "user", "parts": [{ "text": "hello" }] }],
                    "generationConfig": { "temperature": 0.2 }
                }
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(
            body["response"]["candidates"][0]["content"]["parts"][0]["text"],
            "HTTP response"
        );

        drop(client);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn loopback_server_streams_sse_to_http_client() {
        let upstream_sse = format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({
                "id": "stream-1",
                "model": "gpt-test",
                "choices": [{
                    "index": 0,
                    "delta": { "content": "streamed" },
                    "finish_reason": null
                }]
            }),
            json!({
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": "stop"
                }]
            })
        );
        let midpoint = upstream_sse.len() / 2;
        let chunks = vec![
            upstream_sse.as_bytes()[..midpoint].to_vec(),
            upstream_sse.as_bytes()[midpoint..].to_vec(),
        ];
        let (mock_url, _mock_handle) = MockProviderServer::start_chunked(200, chunks).await;
        let (proxy, token) =
            create_proxy(model_config(format!("{mock_url}/v1/chat/completions")), 0).await;
        let handle = LoopbackHttpServer::start(proxy, test_options())
            .await
            .unwrap();
        let client = reqwest::Client::new();

        let response = client
            .post(format!(
                "http://{}/v1internal:streamGenerateContent",
                handle.local_addr()
            ))
            .header("x-agy-byok-token", token)
            .json(&json!({
                "project": "antigravity-internal-project",
                "requestId": "request-stream-1",
                "model": "virtual-1",
                "request": {
                    "contents": [{ "role": "user", "parts": [{ "text": "hello" }] }]
                }
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(
            response.headers()[reqwest::header::CONTENT_TYPE],
            "text/event-stream; charset=utf-8"
        );
        let body = response.text().await.unwrap();
        assert!(body.contains("streamed"));
        assert!(body.contains("\"response\""));
        assert!(body.contains("\"finishReason\":\"STOP\""));
        assert!(!body.contains("data: [DONE]"));

        drop(client);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn fetch_available_models_merges_official_and_custom_catalogs() {
        let official_catalog = json!({
            "catalogVersion": "v10",
            "models": {
                "native-model": {
                    "displayName": "Native Model",
                    "model": "MODEL_NATIVE"
                }
            },
            "agentModelSorts": [{
                "sortId": "recommended",
                "groups": [
                    {
                        "groupId": "primary",
                        "modelIds": ["native-model", "custom-virtual-1"]
                    },
                    {
                        "groupId": "secondary",
                        "modelIds": ["native-model"]
                    }
                ]
            }]
        })
        .to_string();
        let (official_url, _official_handle) =
            MockProviderServer::start(200, &official_catalog).await;
        let (proxy, token) =
            create_proxy(model_config("http://127.0.0.1/unused".to_string()), 0).await;
        let mut options = test_options();
        options.official_cloud_code_endpoint = Some(official_url);
        let handle = LoopbackHttpServer::start(proxy, options).await.unwrap();
        let client = reqwest::Client::new();

        let response = client
            .post(format!(
                "http://{}/v1internal:fetchAvailableModels",
                handle.local_addr()
            ))
            .header("x-agy-byok-token", token)
            .json(&json!({ "project": "test-project" }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let catalog: serde_json::Value = response.json().await.unwrap();
        assert_eq!(catalog["catalogVersion"], "v10");
        assert_eq!(catalog["models"].as_object().unwrap().len(), 2);
        assert_eq!(catalog["models"]["native-model"]["model"], "MODEL_NATIVE");
        assert_eq!(catalog["agentModelSorts"][0]["sortId"], "recommended");
        assert_eq!(
            catalog["agentModelSorts"][0]["groups"][0]["modelIds"],
            json!(["native-model", "custom-virtual-1"])
        );
        assert_eq!(
            catalog["agentModelSorts"][0]["groups"][1]["modelIds"],
            json!(["native-model", "custom-virtual-1"])
        );
        let host_model_id = catalog["models"]["custom-virtual-1"]["requestedModel"]
            .as_str()
            .unwrap();
        assert!(host_model_id.starts_with("MODEL_PLACEHOLDER_M"));
        let custom_model = &catalog["models"]["custom-virtual-1"];
        assert_eq!(custom_model["model"], custom_model["requestedModel"]);
        assert!(custom_model["supportedMimeTypes"].is_object());
        assert!(custom_model.get("id").is_none());
        assert!(custom_model.get("name").is_none());

        drop(client);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn native_generation_is_forwarded_without_byok_envelope() {
        let official_response = json!({
            "response": { "candidates": [{ "native": true }] },
            "traceId": "official"
        })
        .to_string();
        let (official_url, _official_handle) =
            MockProviderServer::start(200, &official_response).await;
        let (proxy, token) =
            create_proxy(model_config("http://127.0.0.1/unused".to_string()), 0).await;
        let mut options = test_options();
        options.official_cloud_code_endpoint = Some(official_url);
        let handle = LoopbackHttpServer::start(proxy, options).await.unwrap();
        let client = reqwest::Client::new();

        let response = client
            .post(format!(
                "http://{}/v1internal:generateContent",
                handle.local_addr()
            ))
            .header("x-agy-byok-token", token)
            .json(&json!({
                "model": "MODEL_NATIVE",
                "request": { "contents": [] }
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), official_response);

        drop(client);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn unknown_cloud_code_routes_are_forwarded_with_vendor_auth() {
        let official_response = json!({ "project": "native-project" }).to_string();
        let (official_url, _official_handle, recorded_request) =
            MockProviderServer::start_recording(200, &official_response).await;
        let (proxy, local_token) = create_proxy(AppConfig::default(), 0).await;
        let mut options = test_options();
        options.official_cloud_code_endpoint = Some(official_url);
        let handle = LoopbackHttpServer::start(proxy, options).await.unwrap();
        let client = reqwest::Client::new();

        let response = client
            .put(format!(
                "http://{}/v1internal:loadCodeAssist?alt=json",
                handle.local_addr()
            ))
            .header("authorization", "Bearer vendor-token")
            .header("x-agy-byok-token", local_token)
            .body("native-request")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), official_response);
        let recorded = recorded_request.await.unwrap();
        assert_eq!(recorded.method, reqwest::Method::PUT);
        assert_eq!(
            recorded.path_and_query,
            "/v1internal:loadCodeAssist?alt=json"
        );
        assert_eq!(
            recorded.authorization.as_deref(),
            Some("Bearer vendor-token")
        );
        assert_eq!(recorded.local_token, None);
        assert_eq!(recorded.body, "native-request");

        drop(client);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn host_model_id_routes_to_the_custom_provider() {
        let upstream_body = json!({
            "choices": [{
                "index": 0,
                "message": { "content": "host id routed" },
                "finish_reason": "stop"
            }]
        })
        .to_string();
        let (mock_url, _mock_handle) = MockProviderServer::start(200, &upstream_body).await;
        let mut config = model_config(format!("{mock_url}/v1/chat/completions"));
        config.virtual_models[0].host_model_id = Some("MODEL_PLACEHOLDER_M400".to_string());
        let (proxy, token) = create_proxy(config, 0).await;
        let handle = LoopbackHttpServer::start(proxy, test_options())
            .await
            .unwrap();
        let client = reqwest::Client::new();

        let response = client
            .post(format!(
                "http://{}/v1internal:generateContent",
                handle.local_addr()
            ))
            .header("x-agy-byok-token", token)
            .json(&json!({
                "model": "MODEL_PLACEHOLDER_M400",
                "request": {
                    "contents": [{ "role": "user", "parts": [{ "text": "hello" }] }]
                }
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(
            body["response"]["candidates"][0]["content"]["parts"][0]["text"],
            "host id routed"
        );

        drop(client);
        handle.shutdown().await.unwrap();
    }
}
