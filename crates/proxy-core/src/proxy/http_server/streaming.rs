use super::types::HttpFrame;
use crate::antigravity::CloudCodeEnvelopeEncoder;
use crate::domain::{ErrorCategory, ProxyError};
use crate::proxy::server::EncodedFrameSink;
use async_trait::async_trait;
use bytes::Bytes;
use hyper::body::Frame;
use tokio::sync::mpsc;

pub(super) struct HttpFrameSink {
    pub(super) sender: mpsc::Sender<HttpFrame>,
}

#[async_trait]
impl EncodedFrameSink for HttpFrameSink {
    async fn send(&mut self, frame: String) -> Result<(), ProxyError> {
        let Some(envelope) = CloudCodeEnvelopeEncoder::wrap_stream_frame(&frame)? else {
            return Ok(());
        };
        self.sender
            .send(Ok(Frame::data(Bytes::from(envelope))))
            .await
            .map_err(|_| {
                ProxyError::new(
                    ErrorCategory::StreamInterrupted,
                    "Downstream SSE receiver is closed",
                    499,
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn http_frame_sink_waits_for_bounded_channel_capacity() {
        let (sender, mut receiver) = mpsc::channel(1);
        let mut sink = HttpFrameSink { sender };
        let frame = "data: {\"candidates\":[]}\n\n".to_string();

        sink.send(frame.clone()).await.unwrap();

        let mut second_send = tokio::spawn(async move { sink.send(frame).await });
        assert!(
            timeout(Duration::from_millis(25), &mut second_send)
                .await
                .is_err(),
            "the second frame must wait while the bounded channel is full"
        );

        receiver.recv().await.unwrap().unwrap();
        timeout(Duration::from_secs(1), second_send)
            .await
            .expect("the second send should resume after capacity is available")
            .expect("the second send task should complete")
            .expect("the second frame should be sent successfully");
        receiver.recv().await.unwrap().unwrap();
    }
}
