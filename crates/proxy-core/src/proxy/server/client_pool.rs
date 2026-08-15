use super::ProxyServer;
use crate::domain::{ErrorCategory, ProxyError};
use reqwest::Client;
use std::time::{Duration, Instant};

pub(super) const MAX_PROVIDER_HTTP_CLIENTS: usize = 8;

impl ProxyServer {
    pub(crate) fn http_client(&self) -> &Client {
        &self.http_client
    }

    pub(super) fn provider_http_client(
        &self,
        connect_timeout_ms: u64,
    ) -> Result<Client, ProxyError> {
        let mut clients = self.provider_http_clients.lock().map_err(|_| {
            ProxyError::new(
                ErrorCategory::Internal,
                "Provider HTTP client cache lock is poisoned",
                500,
            )
        })?;
        if let Some((client, last_accessed)) = clients.get_mut(&connect_timeout_ms) {
            *last_accessed = Instant::now();
            return Ok(client.clone());
        }

        // 当超出容量上限时，仅淘汰最久未访问的单项连接池（LRU），避免全量清空现有活动连接
        if clients.len() >= MAX_PROVIDER_HTTP_CLIENTS {
            if let Some((&oldest_key, _)) = clients
                .iter()
                .min_by_key(|(_, (_, last_accessed))| *last_accessed)
            {
                clients.remove(&oldest_key);
            }
        }

        // Provider 客户端不设置整请求总超时，由具体请求按流式语义选择超时策略。
        let client = Client::builder()
            .connect_timeout(Duration::from_millis(connect_timeout_ms))
            .build()
            .map_err(|error| {
                ProxyError::new(
                    ErrorCategory::Internal,
                    format!("Failed to create Provider HTTP client: {error}"),
                    500,
                )
            })?;
        clients.insert(connect_timeout_ms, (client.clone(), Instant::now()));
        Ok(client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::ConfigStore;

    #[test]
    fn test_provider_client_pool_lru_eviction() {
        let server = ProxyServer::new(ConfigStore::in_memory(Default::default()), 0);

        // 填满连接池容量 8 项
        for i in 1..=MAX_PROVIDER_HTTP_CLIENTS as u64 {
            assert!(server.provider_http_client(i * 1000).is_ok());
        }
        {
            let clients = server.provider_http_clients.lock().unwrap();
            assert_eq!(clients.len(), MAX_PROVIDER_HTTP_CLIENTS);
            assert!(clients.contains_key(&1000));
        }

        // 再次访问 1000ms，刷新其访问时间
        assert!(server.provider_http_client(1000).is_ok());

        // 插入第 9 项，应当淘汰最久未访问的 2000ms 项，而不是 1000ms，更不会清空整个池
        assert!(server.provider_http_client(9999).is_ok());
        {
            let clients = server.provider_http_clients.lock().unwrap();
            assert_eq!(clients.len(), MAX_PROVIDER_HTTP_CLIENTS);
            assert!(clients.contains_key(&1000));
            assert!(clients.contains_key(&9999));
            assert!(!clients.contains_key(&2000));
        }
    }
}
