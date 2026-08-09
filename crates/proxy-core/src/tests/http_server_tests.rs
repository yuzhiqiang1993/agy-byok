#[cfg(test)]
mod tests {
    use crate::domain::AppConfig;
    use crate::domain::*;
    use crate::proxy::activity::{ActivityOperation, ActivityProtocol};
    use crate::proxy::{HttpServerOptions, LoopbackHttpServer, ProxyServer};
    use crate::storage::ConfigStore;
    use crate::tests::mock_provider::MockProviderServer;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    fn test_options() -> HttpServerOptions {
        HttpServerOptions {
            require_auth: true,
            graceful_shutdown_timeout: Duration::from_secs(1),
            ..HttpServerOptions::default()
        }
    }

    fn model_config(generate_endpoint: String) -> AppConfig {
        AppConfig {
            proxy_port: 51234,
            providers: vec![Provider {
                id: "provider-1".to_string(),
                name: "Mock Provider".to_string(),
                protocol: ProviderProtocol::OpenaiChatCompletions,
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
                token_limits: ModelTokenLimits::default(),
                compression_policy: None,
                tokenizer: None,
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
            model_compression_policies: Default::default(),
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
        let handle = LoopbackHttpServer::start(proxy.clone(), test_options())
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

        let activities = proxy.activity_log().get_recent();
        assert_eq!(activities.len(), 4);
        assert!(activities
            .iter()
            .any(|activity| activity.as_http().is_some_and(|item| {
                item.operation == ActivityOperation::HealthCheck
                    && item.request_path == "/health"
                    && item.response_summary.as_deref() == Some("status=ok")
            })));
        assert!(activities
            .iter()
            .any(|activity| activity.as_http().is_some_and(|item| {
                item.operation == ActivityOperation::ListModels && item.common.status_code == 401
            })));
        assert!(activities
            .iter()
            .any(|activity| activity.as_http().is_some_and(|item| {
                item.operation == ActivityOperation::ListModels
                    && item.common.status_code == 200
                    && item.response_summary.as_deref() == Some("models=1")
            })));
        assert!(activities
            .iter()
            .any(|activity| activity.as_http().is_some_and(|item| {
                item.operation == ActivityOperation::FetchAvailableModels
                    && item.request_path == "/v1internal:fetchAvailableModels"
                    && item
                        .response_summary
                        .as_deref()
                        .is_some_and(|summary| summary.starts_with("catalog_models=1;"))
            })));

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

        let (fallback_proxy, _) = create_proxy(AppConfig::default(), occupied_port).await;
        let fallback_handle = LoopbackHttpServer::start(
            fallback_proxy,
            HttpServerOptions {
                fallback_to_random_port_on_bind_error: true,
                ..test_options()
            },
        )
        .await
        .unwrap();
        assert_ne!(fallback_handle.local_addr().port(), occupied_port);

        drop(client);
        fallback_handle.shutdown().await.unwrap();
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
            "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
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
            }),
            json!({
                "choices": [],
                "usage": {
                    "prompt_tokens": 11,
                    "completion_tokens": 7,
                    "total_tokens": 18,
                    "prompt_tokens_details": { "cached_tokens": 4 },
                    "completion_tokens_details": { "reasoning_tokens": 3 }
                }
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
        assert_eq!(body.matches("\"usageMetadata\"").count(), 1);
        assert!(body.contains("\"promptTokenCount\":11"));
        assert!(body.contains("\"candidatesTokenCount\":4"));
        assert!(body.contains("\"cachedContentTokenCount\":4"));
        assert!(body.contains("\"thoughtsTokenCount\":3"));
        let usage_frame = body
            .split("\n\n")
            .find(|frame| frame.contains("\"usageMetadata\""))
            .unwrap();
        let payload: serde_json::Value =
            serde_json::from_str(usage_frame.strip_prefix("data: ").unwrap()).unwrap();
        assert!(!payload["response"]["candidates"]
            .as_array()
            .unwrap()
            .is_empty());
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
    async fn oversized_official_catalog_falls_back_to_custom_models() {
        let oversized_catalog = "x".repeat(513);
        let (official_url, _official_handle) =
            MockProviderServer::start(200, &oversized_catalog).await;
        let (proxy, token) =
            create_proxy(model_config("http://127.0.0.1/unused".to_string()), 0).await;
        let mut options = test_options();
        options.max_body_bytes = 512;
        options.official_cloud_code_endpoint = Some(official_url);
        let handle = LoopbackHttpServer::start(proxy, options).await.unwrap();
        let client = reqwest::Client::new();

        let response = client
            .post(format!(
                "http://{}/v1internal:fetchAvailableModels",
                handle.local_addr()
            ))
            .header("x-agy-byok-token", token)
            .json(&json!({}))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let catalog: serde_json::Value = response.json().await.unwrap();
        assert_eq!(catalog["models"].as_object().unwrap().len(), 1);
        assert!(catalog["models"]["custom-virtual-1"].is_object());

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
        let handle = LoopbackHttpServer::start(proxy.clone(), options)
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
                "model": "MODEL_NATIVE",
                "request": { "contents": [] }
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), official_response);
        let activities = proxy.activity_log().get_recent();
        assert_eq!(activities.len(), 1);
        let activity = activities[0].as_chat().expect("expected chat activity");
        assert_eq!(activity.virtual_model_id, "MODEL_NATIVE");
        assert_eq!(activity.upstream_model_id.as_deref(), Some("MODEL_NATIVE"));
        assert_eq!(activity.provider_id, "antigravity-official");
        assert_eq!(activity.provider_protocol, Some(ActivityProtocol::Native));
        assert_eq!(activity.common.status_code, 200);

        drop(client);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn native_non_streaming_response_enforces_buffer_limit() {
        let oversized_response = "x".repeat(513);
        let (official_url, _official_handle) =
            MockProviderServer::start(200, &oversized_response).await;
        let (proxy, token) =
            create_proxy(model_config("http://127.0.0.1/unused".to_string()), 0).await;
        let mut options = test_options();
        options.max_body_bytes = 512;
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

        assert_eq!(response.status(), reqwest::StatusCode::BAD_GATEWAY);
        let error: serde_json::Value = response.json().await.unwrap();
        assert_eq!(error["error"]["category"], "native_forwarding_failed");

        drop(client);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn custom_model_namespace_is_rejected_locally_when_unavailable() {
        let mut config = model_config("http://127.0.0.1/unused".to_string());
        config.virtual_models[0].enabled = false;
        let (proxy, token) = create_proxy(config, 0).await;
        let handle = LoopbackHttpServer::start(proxy, test_options())
            .await
            .unwrap();
        let client = reqwest::Client::new();

        for model_id in ["virtual-1", "custom-stale-model"] {
            let response = client
                .post(format!(
                    "http://{}/v1internal:generateContent",
                    handle.local_addr()
                ))
                .header("x-agy-byok-token", &token)
                .json(&json!({
                    "model": model_id,
                    "request": { "contents": [] }
                }))
                .send()
                .await
                .unwrap();

            assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
        }

        drop(client);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn unknown_cloud_code_routes_are_forwarded_with_vendor_auth() {
        let official_response = json!({ "project": "native-project" }).to_string();
        let (official_url, _official_handle, recorded_request) =
            MockProviderServer::start_recording(200, &official_response).await;
        let official_host = official_url
            .strip_prefix("http://")
            .expect("mock endpoint uses HTTP")
            .to_string();
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
        assert_eq!(recorded.host.as_deref(), Some(official_host.as_str()));
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
    async fn forwarded_native_responses_rewrite_official_cloud_code_urls_to_proxy_address() {
        let official_response = json!({
            "codeAssistEndpoint": "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
            "userInfoEndpoint": "https://cloudcode-pa.googleapis.com/v1internal:fetchUserInfo",
            "generativeEndpoint": "https://generativelanguage.googleapis.com/v1beta/models"
        })
        .to_string();
        let (official_url, _official_handle) =
            MockProviderServer::start(200, &official_response).await;
        let (proxy, local_token) = create_proxy(AppConfig::default(), 0).await;
        let mut options = test_options();
        options.official_cloud_code_endpoint = Some(official_url);
        let handle = LoopbackHttpServer::start(proxy, options).await.unwrap();
        let client = reqwest::Client::new();

        let response = client
            .post(format!(
                "http://{}/v1internal:loadCodeAssist",
                handle.local_addr()
            ))
            .header("authorization", "Bearer vendor-token")
            .header("x-agy-byok-token", local_token)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = response.json().await.unwrap();
        let expected_host = format!("http://{}", handle.local_addr());
        assert_eq!(
            body["codeAssistEndpoint"],
            format!("{expected_host}/v1internal:loadCodeAssist")
        );
        assert_eq!(
            body["userInfoEndpoint"],
            format!("{expected_host}/v1internal:fetchUserInfo")
        );
        assert_eq!(
            body["generativeEndpoint"],
            format!("{expected_host}/v1beta/models")
        );

        drop(client);
        handle.shutdown().await.unwrap();
    }

    #[test]
    fn only_configured_placeholder_ids_are_classified_as_custom_models() {
        let unconfigured_proxy = ProxyServer::new(ConfigStore::in_memory(AppConfig::default()), 0);
        assert!(!unconfigured_proxy.is_custom_model_id("MODEL_PLACEHOLDER_M400"));
        assert!(unconfigured_proxy.is_custom_model_id("custom-missing"));

        let mut config = model_config("http://127.0.0.1/generate".to_string());
        config.virtual_models[0].host_model_id = Some("MODEL_PLACEHOLDER_M400".to_string());
        let configured_proxy = ProxyServer::new(ConfigStore::in_memory(config), 0);
        assert!(configured_proxy.is_custom_model_id("MODEL_PLACEHOLDER_M400"));
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
                "request": {
                    "requestedModel": "MODEL_PLACEHOLDER_M400",
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

    #[tokio::test]
    async fn binary_patched_paths_and_padding_are_normalized_and_routed_correctly() {
        assert_eq!(
            LoopbackHttpServer::normalize_path(
                "/v1internal/xxxxxxx/v1internal:fetchAvailableModels"
            ),
            "/v1internal:fetchAvailableModels"
        );
        assert_eq!(
            LoopbackHttpServer::normalize_path(
                "/dummy_path_padding/v1internal:streamGenerateContent"
            ),
            "/v1internal:streamGenerateContent"
        );

        let official_response = json!({
            "codeAssistEndpoint": "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist"
        })
        .to_string();
        let (official_url, _official_handle, recorded_request) =
            MockProviderServer::start_recording(200, &official_response).await;
        let (proxy, local_token) = create_proxy(AppConfig::default(), 0).await;
        let mut options = test_options();
        options.official_cloud_code_endpoint = Some(official_url);
        let handle = LoopbackHttpServer::start(proxy, options).await.unwrap();
        let client = reqwest::Client::new();

        let response = client
            .post(format!(
                "http://{}/v1internal/xxxxxxx/v1internal:loadCodeAssist",
                handle.local_addr()
            ))
            .header("authorization", "Bearer vendor-token")
            .header("x-agy-byok-token", local_token)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let recorded = recorded_request.await.unwrap();
        assert_eq!(recorded.path_and_query, "/v1internal:loadCodeAssist");

        drop(client);
        handle.shutdown().await.unwrap();
    }
}
