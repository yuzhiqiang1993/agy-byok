use super::*;
use crate::tests::mock_provider::MockProviderServer;

/// 测试解码器仅用于触发传输层读取路径。
struct IgnoringStreamDecoder;

impl ProviderStreamDecoder for IgnoringStreamDecoder {
    fn decode_data(&mut self, _data: &str) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        Ok(Vec::new())
    }

    fn finish(&mut self) -> Result<Vec<NeutralStreamEvent>, ProxyError> {
        Ok(Vec::new())
    }
}

/// 测试接收器忽略事件，只保留流处理结果。
struct IgnoringEventSink;

#[async_trait]
impl NeutralEventSink for IgnoringEventSink {
    async fn send(&mut self, _event: NeutralStreamEvent) -> Result<(), ProxyError> {
        Ok(())
    }
}

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

#[tokio::test]
async fn stream_pipe_classifies_body_deadline_as_timeout() {
    let chunks = vec![
        (Duration::ZERO, b": heartbeat\n\n".to_vec()),
        (Duration::from_millis(150), b"data: ignored\n\n".to_vec()),
    ];
    let (url, _handle) = MockProviderServer::start_delayed_chunked(200, chunks).await;
    let response = reqwest::Client::new()
        .get(url)
        .timeout(Duration::from_millis(50))
        .send()
        .await
        .unwrap();
    let mut decoder = IgnoringStreamDecoder;
    let mut sink = IgnoringEventSink;

    let error = StreamPipe::process_stream_to(response, 500, &mut decoder, &mut sink)
        .await
        .unwrap_err();

    assert_eq!(error.category, ErrorCategory::Timeout);
    assert_eq!(error.status_code, 504);
    assert!(error.message.starts_with("Stream read timeout:"));
}

#[tokio::test]
async fn stream_pipe_still_enforces_idle_timeout_between_chunks() {
    let chunks = vec![
        (Duration::ZERO, b": heartbeat\n\n".to_vec()),
        (Duration::from_millis(150), b"data: ignored\n\n".to_vec()),
    ];
    let (url, _handle) = MockProviderServer::start_delayed_chunked(200, chunks).await;
    let response = reqwest::Client::new().get(url).send().await.unwrap();
    let mut decoder = IgnoringStreamDecoder;
    let mut sink = IgnoringEventSink;

    let error = StreamPipe::process_stream_to(response, 50, &mut decoder, &mut sink)
        .await
        .unwrap_err();

    assert_eq!(error.category, ErrorCategory::Timeout);
    assert_eq!(error.status_code, 504);
    assert!(error.message.starts_with("Stream idle timeout after 50 ms"));
}
