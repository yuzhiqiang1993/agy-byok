use std::convert::Infallible;
use std::net::SocketAddr;
use tokio::net::TcpListener;

pub struct MockProviderServer {
    pub addr: SocketAddr,
    pub response_body: String,
    pub response_status: u16,
}

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
}
