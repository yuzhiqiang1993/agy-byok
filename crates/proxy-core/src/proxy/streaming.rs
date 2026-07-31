use crate::domain::{ErrorCategory, NeutralStreamEvent, ProxyError};
use crate::providers::ProviderStreamDecoder;
use async_trait::async_trait;
use reqwest::Response;
use std::time::Duration;
use tokio::time::timeout;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseFrame {
    pub event: Option<String>,
    pub data: String,
    pub id: Option<String>,
    pub retry_ms: Option<u64>,
}

#[derive(Debug, Default)]
pub struct SseFrameDecoder {
    pending_utf8: Vec<u8>,
    text_buffer: String,
    data_lines: Vec<String>,
    event_type: Option<String>,
    last_event_id: Option<String>,
    retry_ms: Option<u64>,
    bom_checked: bool,
    finished: bool,
}

impl SseFrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<SseFrame>, ProxyError> {
        if self.finished {
            return Err(Self::stream_error("SSE decoder already finished"));
        }

        self.pending_utf8.extend_from_slice(bytes);
        self.decode_pending_utf8(false)?;
        Ok(self.process_available_lines(false))
    }

    pub fn finish(&mut self) -> Result<Vec<SseFrame>, ProxyError> {
        if self.finished {
            return Ok(Vec::new());
        }

        self.decode_pending_utf8(true)?;
        let frames = self.process_available_lines(false);
        if !self.text_buffer.is_empty() || !self.data_lines.is_empty() {
            return Err(Self::stream_error(
                "Upstream SSE ended before the current event was terminated by a blank line",
            ));
        }
        self.finished = true;
        Ok(frames)
    }

    fn decode_pending_utf8(&mut self, eof: bool) -> Result<(), ProxyError> {
        if self.pending_utf8.is_empty() {
            return Ok(());
        }

        match std::str::from_utf8(&self.pending_utf8) {
            Ok(text) => {
                let decoded = text.to_string();
                self.append_decoded_text(&decoded);
                self.pending_utf8.clear();
                Ok(())
            }
            Err(error) if error.error_len().is_some() => {
                Err(Self::stream_error("Upstream SSE contains invalid UTF-8"))
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                if valid_up_to > 0 {
                    let decoded = std::str::from_utf8(&self.pending_utf8[..valid_up_to])
                        .expect("valid UTF-8 prefix")
                        .to_string();
                    self.append_decoded_text(&decoded);
                    self.pending_utf8.drain(..valid_up_to);
                }

                if eof && !self.pending_utf8.is_empty() {
                    return Err(Self::stream_error(
                        "Upstream SSE ended with incomplete UTF-8",
                    ));
                }
                Ok(())
            }
        }
    }

    fn append_decoded_text(&mut self, decoded: &str) {
        if self.bom_checked {
            self.text_buffer.push_str(decoded);
            return;
        }

        self.bom_checked = true;
        self.text_buffer
            .push_str(decoded.strip_prefix('\u{feff}').unwrap_or(decoded));
    }

    fn process_available_lines(&mut self, eof: bool) -> Vec<SseFrame> {
        let mut frames = Vec::new();
        while let Some(line) = self.take_next_line(eof) {
            if let Some(frame) = self.process_line(&line) {
                frames.push(frame);
            }
        }
        frames
    }

    fn take_next_line(&mut self, eof: bool) -> Option<String> {
        let delimiter_position = self
            .text_buffer
            .as_bytes()
            .iter()
            .position(|byte| matches!(byte, b'\r' | b'\n'));

        let Some(position) = delimiter_position else {
            if eof && !self.text_buffer.is_empty() {
                return Some(std::mem::take(&mut self.text_buffer));
            }
            return None;
        };

        let bytes = self.text_buffer.as_bytes();
        if bytes[position] == b'\r' && position + 1 == bytes.len() && !eof {
            return None;
        }

        let delimiter_len =
            if bytes[position] == b'\r' && bytes.get(position + 1).copied() == Some(b'\n') {
                2
            } else {
                1
            };
        let line = self.text_buffer[..position].to_string();
        self.text_buffer.drain(..position + delimiter_len);
        Some(line)
    }

    fn process_line(&mut self, line: &str) -> Option<SseFrame> {
        if line.is_empty() {
            return self.dispatch_frame();
        }
        if line.starts_with(':') {
            return None;
        }

        let (field, value) = line
            .split_once(':')
            .map(|(field, value)| (field, value.strip_prefix(' ').unwrap_or(value)))
            .unwrap_or((line, ""));

        match field {
            "data" => self.data_lines.push(value.to_string()),
            "event" => self.event_type = Some(value.to_string()),
            "id" if !value.contains('\0') => self.last_event_id = Some(value.to_string()),
            "retry" => self.retry_ms = value.parse().ok(),
            _ => {}
        }
        None
    }

    fn dispatch_frame(&mut self) -> Option<SseFrame> {
        if self.data_lines.is_empty() {
            self.event_type = None;
            self.retry_ms = None;
            return None;
        }

        Some(SseFrame {
            event: self.event_type.take(),
            data: self.data_lines.drain(..).collect::<Vec<_>>().join("\n"),
            id: self.last_event_id.clone(),
            retry_ms: self.retry_ms.take(),
        })
    }

    fn stream_error(message: impl Into<String>) -> ProxyError {
        ProxyError::new(ErrorCategory::StreamInterrupted, message, 502)
    }
}

#[async_trait]
pub(crate) trait NeutralEventSink: Send {
    async fn send(&mut self, event: NeutralStreamEvent) -> Result<(), ProxyError>;
}

struct CallbackNeutralEventSink<F> {
    callback: F,
}

#[async_trait]
impl<F> NeutralEventSink for CallbackNeutralEventSink<F>
where
    F: FnMut(NeutralStreamEvent) -> Result<(), ProxyError> + Send,
{
    async fn send(&mut self, event: NeutralStreamEvent) -> Result<(), ProxyError> {
        (self.callback)(event)
    }
}

pub struct StreamPipe;

impl StreamPipe {
    pub async fn process_stream<F>(
        response: Response,
        idle_timeout_ms: u64,
        provider_decoder: &mut dyn ProviderStreamDecoder,
        on_event: F,
    ) -> Result<(), ProxyError>
    where
        F: FnMut(NeutralStreamEvent) -> Result<(), ProxyError> + Send,
    {
        let mut event_sink = CallbackNeutralEventSink { callback: on_event };
        Self::process_stream_to(response, idle_timeout_ms, provider_decoder, &mut event_sink).await
    }

    pub(crate) async fn process_stream_to(
        mut response: Response,
        idle_timeout_ms: u64,
        provider_decoder: &mut dyn ProviderStreamDecoder,
        event_sink: &mut dyn NeutralEventSink,
    ) -> Result<(), ProxyError> {
        let idle_duration = Duration::from_millis(if idle_timeout_ms == 0 {
            30000
        } else {
            idle_timeout_ms
        });
        let mut sse_decoder = SseFrameDecoder::new();

        loop {
            match timeout(idle_duration, response.chunk()).await {
                Ok(Ok(Some(bytes))) => {
                    for frame in sse_decoder.push(&bytes)? {
                        for event in provider_decoder.decode_data(&frame.data)? {
                            let response_ended = matches!(event, NeutralStreamEvent::ResponseEnd);
                            event_sink.send(event).await?;
                            if response_ended {
                                return Ok(());
                            }
                        }
                    }
                }
                Ok(Ok(None)) => {
                    for frame in sse_decoder.finish()? {
                        for event in provider_decoder.decode_data(&frame.data)? {
                            let response_ended = matches!(event, NeutralStreamEvent::ResponseEnd);
                            event_sink.send(event).await?;
                            if response_ended {
                                return Ok(());
                            }
                        }
                    }
                    for event in provider_decoder.finish()? {
                        event_sink.send(event).await?;
                    }
                    return Ok(());
                }
                Ok(Err(error)) => {
                    return Err(ProxyError::new(
                        ErrorCategory::StreamInterrupted,
                        format!("Stream read error: {error}"),
                        502,
                    ));
                }
                Err(_) => {
                    return Err(ProxyError::new(
                        ErrorCategory::Timeout,
                        format!("Stream idle timeout after {} ms", idle_duration.as_millis()),
                        504,
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_handles_utf8_split_across_network_chunks() {
        let mut decoder = SseFrameDecoder::new();
        let payload = "data: 你\n\n".as_bytes();

        assert!(decoder.push(&payload[..7]).unwrap().is_empty());
        let frames = decoder.push(&payload[7..]).unwrap();

        assert_eq!(
            frames,
            vec![SseFrame {
                data: "你".to_string(),
                ..SseFrame::default()
            }]
        );
    }

    #[test]
    fn decoder_supports_multiline_data_comments_and_crlf() {
        let mut decoder = SseFrameDecoder::new();
        let frames = decoder
            .push(
                b": heartbeat\r\nevent: message\r\ndata:first\r\ndata: second\r\nid: 42\r\nretry: 1000\r\n\r\n",
            )
            .unwrap();

        assert_eq!(
            frames,
            vec![SseFrame {
                event: Some("message".to_string()),
                data: "first\nsecond".to_string(),
                id: Some("42".to_string()),
                retry_ms: Some(1000),
            }]
        );
    }

    #[test]
    fn decoder_rejects_event_without_terminating_blank_line() {
        let mut decoder = SseFrameDecoder::new();

        assert!(decoder.push(b"data: final\n").unwrap().is_empty());
        let error = decoder.finish().unwrap_err();

        assert_eq!(error.category, ErrorCategory::StreamInterrupted);
    }

    #[test]
    fn decoder_ignores_one_utf8_bom_at_stream_start() {
        let mut decoder = SseFrameDecoder::new();
        let frames = decoder.push("\u{feff}data: first\n\n".as_bytes()).unwrap();

        assert_eq!(
            frames,
            vec![SseFrame {
                data: "first".to_string(),
                ..SseFrame::default()
            }]
        );
    }

    #[test]
    fn decoder_rejects_incomplete_utf8_at_eof() {
        let mut decoder = SseFrameDecoder::new();

        decoder
            .push(&[b'd', b'a', b't', b'a', b':', b' ', 0xE4])
            .unwrap();
        let error = decoder.finish().unwrap_err();

        assert_eq!(error.category, ErrorCategory::StreamInterrupted);
    }
}
