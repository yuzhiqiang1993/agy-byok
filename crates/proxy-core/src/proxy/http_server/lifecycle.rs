use super::forwarding::validate_official_endpoint;
use super::routing::handle_request;
use super::types::{HttpServerOptions, INTERNAL_PROBE_HEADER};
use crate::domain::{ErrorCategory, ProxyError};
use crate::proxy::server::ProxyServer;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::timeout;

pub struct HttpServerHandle {
    local_addr: SocketAddr,
    shutdown_sender: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), ProxyError>>>,
}

impl HttpServerHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub async fn shutdown(mut self) -> Result<(), ProxyError> {
        if let Some(sender) = self.shutdown_sender.take() {
            let _ = sender.send(());
        }
        let task = self.task.take().ok_or_else(|| {
            ProxyError::new(ErrorCategory::Internal, "HTTP server task is missing", 500)
        })?;
        task.await.map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("HTTP server task failed: {error}"),
                500,
            )
        })?
    }
}

impl Drop for HttpServerHandle {
    fn drop(&mut self) {
        if let Some(sender) = self.shutdown_sender.take() {
            let _ = sender.send(());
        }
    }
}

pub struct LoopbackHttpServer;

impl LoopbackHttpServer {
    pub fn normalize_path(path: &str) -> String {
        let mut p = path;
        if let Some(idx) = p.find("/dummy_path_padding") {
            p = &p[idx + "/dummy_path_padding".len()..];
        }
        if p.starts_with("/v1internal/") {
            let rest = &p["/v1internal/".len()..];
            if let Some(slash_idx) = rest.find('/') {
                let seg = &rest[..slash_idx];
                if !seg.is_empty()
                    && seg
                        .chars()
                        .all(|c| c == 'x' || c.is_alphanumeric() || c == '_' || c == '-')
                {
                    p = &rest[slash_idx..];
                }
            }
        }
        if p.is_empty() {
            "/".to_string()
        } else {
            p.to_string()
        }
    }

    pub fn normalize_path_and_query(uri: &hyper::Uri) -> String {
        let norm_path = Self::normalize_path(uri.path());
        if let Some(query) = uri.query() {
            format!("{norm_path}?{query}")
        } else {
            norm_path
        }
    }

    pub async fn start(
        proxy: Arc<ProxyServer>,
        options: HttpServerOptions,
    ) -> Result<HttpServerHandle, ProxyError> {
        if options.max_body_bytes == 0
            || options.max_concurrent_requests == 0
            || options.stream_buffer_capacity == 0
        {
            return Err(ProxyError::new(
                ErrorCategory::InvalidRequest,
                "HTTP server limits must be greater than zero",
                400,
            ));
        }

        if let Some(endpoint) = &options.official_cloud_code_endpoint {
            validate_official_endpoint(endpoint)?;
        }

        let bind_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, proxy.port()));
        let listener = match TcpListener::bind(bind_addr).await {
            Ok(listener) => listener,
            Err(primary_error)
                if options.fallback_to_random_port_on_bind_error && proxy.port() != 0 =>
            {
                let fallback_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
                tracing::warn!(
                    "Preferred proxy port {bind_addr} is unavailable; selecting a random loopback port"
                );
                TcpListener::bind(fallback_addr).await.map_err(|fallback_error| {
                    ProxyError::new(
                        ErrorCategory::ConnectionFailed,
                        format!(
                            "Failed to bind preferred loopback address {bind_addr}: {primary_error}; \
                             random loopback fallback also failed: {fallback_error}"
                        ),
                        500,
                    )
                })?
            }
            Err(error) => {
                return Err(ProxyError::new(
                    ErrorCategory::ConnectionFailed,
                    format!("Failed to bind loopback HTTP server on {bind_addr}: {error}"),
                    500,
                ));
            }
        };
        let local_addr = listener.local_addr().map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("Failed to read loopback server address: {error}"),
                500,
            )
        })?;
        if !local_addr.ip().is_loopback() {
            return Err(ProxyError::new(
                ErrorCategory::Internal,
                "HTTP server refused to bind a non-loopback address",
                500,
            ));
        }

        let semaphore = Arc::new(Semaphore::new(options.max_concurrent_requests));
        let (shutdown_sender, mut shutdown_receiver) = oneshot::channel();
        let task_options = options.clone();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    _ = &mut shutdown_receiver => break,
                    accept_result = listener.accept() => {
                        let (stream, peer_addr) = accept_result.map_err(|error| {
                            ProxyError::new(
                                ErrorCategory::ConnectionFailed,
                                format!("Loopback HTTP accept failed: {error}"),
                                500,
                            )
                        })?;
                        if !peer_addr.ip().is_loopback() {
                            tracing::warn!("Rejected non-loopback peer on loopback listener");
                            continue;
                        }

                        let proxy = proxy.clone();
                        let options = task_options.clone();
                        let semaphore = semaphore.clone();
                        connections.spawn(async move {
                            let service = hyper::service::service_fn(move |request| {
                                handle_request(
                                    request,
                                    proxy.clone(),
                                    options.clone(),
                                    semaphore.clone(),
                                )
                            });
                            if let Err(error) = hyper_util::server::conn::auto::Builder::new(
                                hyper_util::rt::TokioExecutor::new(),
                            )
                            .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                            .await
                            {
                                tracing::debug!("Loopback HTTP connection closed with error: {error}");
                            }
                        });
                    }
                    Some(join_result) = connections.join_next(), if !connections.is_empty() => {
                        if let Err(error) = join_result {
                            tracing::debug!("Loopback HTTP connection task failed: {error}");
                        }
                    }
                }
            }

            let drain_result = timeout(task_options.graceful_shutdown_timeout, async {
                while let Some(join_result) = connections.join_next().await {
                    if let Err(error) = join_result {
                        tracing::debug!("Loopback HTTP connection task failed: {error}");
                    }
                }
            })
            .await;
            if drain_result.is_err() {
                connections.abort_all();
                while connections.join_next().await.is_some() {}
            }
            Ok(())
        });

        if let Err(error) = probe_health(local_addr).await {
            let _ = shutdown_sender.send(());
            let _ = task.await;
            return Err(error);
        }

        Ok(HttpServerHandle {
            local_addr,
            shutdown_sender: Some(shutdown_sender),
            task: Some(task),
        })
    }
}

async fn probe_health(local_addr: SocketAddr) -> Result<(), ProxyError> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|error| {
            ProxyError::new(
                ErrorCategory::Internal,
                format!("Failed to build internal health client: {error}"),
                500,
            )
        })?;
    let response = client
        .get(format!("http://{local_addr}/health"))
        .header(INTERNAL_PROBE_HEADER, "1")
        .send()
        .await
        .map_err(|error| {
            ProxyError::new(
                ErrorCategory::ConnectionFailed,
                format!("Internal health probe failed: {error}"),
                500,
            )
        })?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(ProxyError::new(
            ErrorCategory::Internal,
            format!("Internal health probe returned {}", response.status()),
            500,
        ));
    }
    Ok(())
}
