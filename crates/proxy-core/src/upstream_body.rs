use reqwest::Response;

pub(crate) const DEFAULT_MAX_BUFFERED_UPSTREAM_BODY_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct LimitedResponseBody {
    bytes: Vec<u8>,
    truncated: bool,
}

impl LimitedResponseBody {
    pub(crate) fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub(crate) fn into_text(self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

pub(crate) async fn read_limited_response_body(
    mut response: Response,
    limit: usize,
) -> Result<LimitedResponseBody, reqwest::Error> {
    if response
        .content_length()
        .is_some_and(|content_length| content_length > limit as u64)
    {
        return Ok(LimitedResponseBody {
            bytes: Vec::new(),
            truncated: true,
        });
    }

    let mut bytes = Vec::with_capacity(
        response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or_default()
            .min(limit),
    );
    while let Some(chunk) = response.chunk().await? {
        let remaining = limit.saturating_sub(bytes.len());
        if chunk.len() > remaining {
            bytes.extend_from_slice(&chunk[..remaining]);
            return Ok(LimitedResponseBody {
                bytes,
                truncated: true,
            });
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok(LimitedResponseBody {
        bytes,
        truncated: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::mock_provider::MockProviderServer;

    #[tokio::test]
    async fn limited_reader_accepts_exact_limit_and_truncates_chunked_overflow() {
        let (exact_url, _exact_handle) =
            MockProviderServer::start_chunked(200, vec![b"12".to_vec(), b"345".to_vec()]).await;
        let exact = read_limited_response_body(reqwest::get(exact_url).await.unwrap(), 5)
            .await
            .unwrap();
        assert!(!exact.is_truncated());
        assert_eq!(exact.into_bytes(), b"12345");

        let (overflow_url, _overflow_handle) = MockProviderServer::start_chunked(
            200,
            vec![b"12".to_vec(), b"345".to_vec(), b"6".to_vec()],
        )
        .await;
        let overflow = read_limited_response_body(reqwest::get(overflow_url).await.unwrap(), 5)
            .await
            .unwrap();
        assert!(overflow.is_truncated());
        assert_eq!(overflow.into_bytes(), b"12345");
    }

    #[tokio::test]
    async fn limited_reader_rejects_advertised_oversize_without_buffering() {
        let (url, _handle) = MockProviderServer::start(200, "123456").await;
        let body = read_limited_response_body(reqwest::get(url).await.unwrap(), 5)
            .await
            .unwrap();

        assert!(body.is_truncated());
        assert!(body.into_bytes().is_empty());
    }
}
