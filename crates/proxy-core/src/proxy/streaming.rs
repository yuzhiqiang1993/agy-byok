use crate::domain::{NeutralStreamEvent, ProxyError};
use reqwest::Response;
use std::time::Duration;
use tokio::time::timeout;

pub struct StreamPipe;

impl StreamPipe {
    /// 从 reqwest 流式 Response 中持续读取 SSE 文本数据块，包含空闲超时逻辑
    pub async fn process_stream<F>(
        mut response: Response,
        idle_timeout_ms: u64,
        mut on_chunk: F,
    ) -> Result<(), ProxyError>
    where
        F: FnMut(&str) -> Result<Vec<NeutralStreamEvent>, ProxyError> + Send,
    {
        let idle_duration = Duration::from_millis(if idle_timeout_ms == 0 {
            30000
        } else {
            idle_timeout_ms
        });

        let mut buffer = String::new();

        loop {
            let next_chunk = timeout(idle_duration, response.chunk()).await;
            match next_chunk {
                Ok(Ok(Some(bytes))) => {
                    let text = String::from_utf8_lossy(&bytes);
                    buffer.push_str(&text);

                    // 处理完成一整行或多行 SSE 数据
                    if buffer.contains("\n\n") || buffer.contains("\n") {
                        let current_buffer = buffer.clone();
                        buffer.clear();
                        let _events = on_chunk(&current_buffer)?;
                    }
                }
                Ok(Ok(None)) => {
                    // 流正常结束
                    if !buffer.is_empty() {
                        let _events = on_chunk(&buffer)?;
                    }
                    break;
                }
                Ok(Err(e)) => {
                    return Err(ProxyError::new(
                        crate::domain::ErrorCategory::StreamInterrupted,
                        format!("Stream read error: {}", e),
                        500,
                    ));
                }
                Err(_) => {
                    return Err(ProxyError::new(
                        crate::domain::ErrorCategory::Timeout,
                        format!("Stream idle timeout after {} ms", idle_timeout_ms),
                        504,
                    ));
                }
            }
        }

        Ok(())
    }
}
