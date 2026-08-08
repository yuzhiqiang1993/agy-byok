use http_body_util::BodyExt;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

#[derive(Debug)]
pub struct RecordedRequest {
    pub method: hyper::Method,
    pub path_and_query: String,
    pub host: Option<String>,
    pub authorization: Option<String>,
    pub local_token: Option<String>,
    pub body: bytes::Bytes,
}

pub struct MockProviderServer;

impl MockProviderServer {
    pub async fn start(status: u16, body: &str) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{}", addr);

        let body_owned = body.to_string();
        let handle = tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let io = hyper_util::rt::TokioIo::new(stream);
                let body_clone = body_owned.clone();

                let service = hyper::service::service_fn(move |_req| {
                    let b = body_clone.clone();
                    async move {
                        let res = hyper::Response::builder()
                            .status(status)
                            .header("Content-Type", "application/json")
                            .body(http_body_util::Full::new(bytes::Bytes::from(b)))
                            .unwrap();
                        Ok::<_, Infallible>(res)
                    }
                });

                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            }
        });

        (url, handle)
    }

    pub async fn start_recording(
        status: u16,
        body: &str,
    ) -> (
        String,
        tokio::task::JoinHandle<()>,
        oneshot::Receiver<RecordedRequest>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{}", addr);
        let response_body = body.to_string();
        let (request_sender, request_receiver) = oneshot::channel();
        let request_sender = Arc::new(Mutex::new(Some(request_sender)));

        let handle = tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let io = hyper_util::rt::TokioIo::new(stream);
                let service = hyper::service::service_fn(
                    move |request: hyper::Request<hyper::body::Incoming>| {
                        let response_body = response_body.clone();
                        let request_sender = request_sender.clone();
                        async move {
                            let (parts, body) = request.into_parts();
                            let body = body.collect().await.unwrap().to_bytes();
                            let recorded = RecordedRequest {
                                method: parts.method,
                                path_and_query: parts
                                    .uri
                                    .path_and_query()
                                    .map(ToString::to_string)
                                    .unwrap_or_default(),
                                host: parts
                                    .headers
                                    .get(hyper::header::HOST)
                                    .and_then(|value| value.to_str().ok())
                                    .map(ToOwned::to_owned),
                                authorization: parts
                                    .headers
                                    .get(hyper::header::AUTHORIZATION)
                                    .and_then(|value| value.to_str().ok())
                                    .map(ToOwned::to_owned),
                                local_token: parts
                                    .headers
                                    .get("x-agy-byok-token")
                                    .and_then(|value| value.to_str().ok())
                                    .map(ToOwned::to_owned),
                                body,
                            };
                            if let Some(sender) = request_sender.lock().unwrap().take() {
                                let _ = sender.send(recorded);
                            }
                            let response = hyper::Response::builder()
                                .status(status)
                                .header("Content-Type", "application/json")
                                .body(http_body_util::Full::new(bytes::Bytes::from(response_body)))
                                .unwrap();
                            Ok::<_, Infallible>(response)
                        }
                    },
                );
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            }
        });

        (url, handle, request_receiver)
    }

    pub async fn start_chunked(
        status: u16,
        chunks: Vec<Vec<u8>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{}", addr);

        let handle = tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let io = hyper_util::rt::TokioIo::new(stream);
                let service = hyper::service::service_fn(move |_request| {
                    let chunks = chunks.clone();
                    async move {
                        let frames = chunks.into_iter().map(|chunk| {
                            Ok::<_, Infallible>(hyper::body::Frame::data(bytes::Bytes::from(chunk)))
                        });
                        let body = http_body_util::StreamBody::new(futures::stream::iter(frames));
                        let response = hyper::Response::builder()
                            .status(status)
                            .header("Content-Type", "text/event-stream")
                            .body(body)
                            .unwrap();
                        Ok::<_, Infallible>(response)
                    }
                });

                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            }
        });

        (url, handle)
    }
}
