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
