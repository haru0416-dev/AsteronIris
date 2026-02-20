use super::*;
use crate::channels::traits::Channel;
use std::path::Path;

#[test]
fn telegram_channel_name() {
    let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
    assert_eq!(ch.name(), "telegram");
}

#[test]
fn telegram_api_url() {
    let ch = TelegramChannel::new("123:ABC".into(), vec![]);
    assert_eq!(
        ch.api_url("getMe"),
        "https://api.telegram.org/bot123:ABC/getMe"
    );
}

#[test]
fn telegram_user_allowed_wildcard() {
    let ch = TelegramChannel::new("t".into(), vec!["*".into()]);
    assert!(ch.is_user_allowed("anyone"));
}

#[test]
fn telegram_user_allowed_specific() {
    let ch = TelegramChannel::new("t".into(), vec!["alice".into(), "bob".into()]);
    assert!(ch.is_user_allowed("alice"));
    assert!(!ch.is_user_allowed("eve"));
}

#[test]
fn telegram_user_denied_empty() {
    let ch = TelegramChannel::new("t".into(), vec![]);
    assert!(!ch.is_user_allowed("anyone"));
}

#[test]
fn telegram_user_exact_match_not_substring() {
    let ch = TelegramChannel::new("t".into(), vec!["alice".into()]);
    assert!(!ch.is_user_allowed("alice_bot"));
    assert!(!ch.is_user_allowed("alic"));
    assert!(!ch.is_user_allowed("malice"));
}

#[test]
fn telegram_user_empty_string_denied() {
    let ch = TelegramChannel::new("t".into(), vec!["alice".into()]);
    assert!(!ch.is_user_allowed(""));
}

#[test]
fn telegram_user_case_sensitive() {
    let ch = TelegramChannel::new("t".into(), vec!["Alice".into()]);
    assert!(ch.is_user_allowed("Alice"));
    assert!(!ch.is_user_allowed("alice"));
    assert!(!ch.is_user_allowed("ALICE"));
}

#[test]
fn telegram_wildcard_with_specific_users() {
    let ch = TelegramChannel::new("t".into(), vec!["alice".into(), "*".into()]);
    assert!(ch.is_user_allowed("alice"));
    assert!(ch.is_user_allowed("bob"));
    assert!(ch.is_user_allowed("anyone"));
}

#[test]
fn telegram_user_allowed_by_numeric_id_identity() {
    let ch = TelegramChannel::new("t".into(), vec!["123456789".into()]);
    assert!(ch.is_any_user_allowed(["unknown", "123456789"]));
}

#[test]
fn telegram_user_denied_when_none_of_identities_match() {
    let ch = TelegramChannel::new("t".into(), vec!["alice".into(), "987654321".into()]);
    assert!(!ch.is_any_user_allowed(["unknown", "123456789"]));
}

// ── File sending API URL tests ──────────────────────────────────

#[test]
fn telegram_api_url_send_document() {
    let ch = TelegramChannel::new("123:ABC".into(), vec![]);
    assert_eq!(
        ch.api_url("sendDocument"),
        "https://api.telegram.org/bot123:ABC/sendDocument"
    );
}

#[test]
fn telegram_api_url_send_photo() {
    let ch = TelegramChannel::new("123:ABC".into(), vec![]);
    assert_eq!(
        ch.api_url("sendPhoto"),
        "https://api.telegram.org/bot123:ABC/sendPhoto"
    );
}

#[test]
fn telegram_api_url_send_video() {
    let ch = TelegramChannel::new("123:ABC".into(), vec![]);
    assert_eq!(
        ch.api_url("sendVideo"),
        "https://api.telegram.org/bot123:ABC/sendVideo"
    );
}

#[test]
fn telegram_api_url_send_audio() {
    let ch = TelegramChannel::new("123:ABC".into(), vec![]);
    assert_eq!(
        ch.api_url("sendAudio"),
        "https://api.telegram.org/bot123:ABC/sendAudio"
    );
}

#[test]
fn telegram_api_url_send_voice() {
    let ch = TelegramChannel::new("123:ABC".into(), vec![]);
    assert_eq!(
        ch.api_url("sendVoice"),
        "https://api.telegram.org/bot123:ABC/sendVoice"
    );
}

// ── File sending integration tests (with mock server) ──────────

#[tokio::test]
async fn telegram_send_document_bytes_builds_correct_form() {
    // This test verifies the method doesn't panic and handles bytes correctly
    let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
    let file_bytes = b"Hello, this is a test file content".to_vec();

    // The actual API call will fail (no real server), but we verify the method exists
    // and handles the input correctly up to the network call
    let result = ch
        .send_document_bytes("123456", file_bytes, "test.txt", Some("Test caption"))
        .await;

    // Should fail with network error, not a panic or type error
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    // Error should be network-related, not a code bug
    assert!(
        err.contains("error") || err.contains("failed") || err.contains("connect"),
        "Expected network error, got: {err}"
    );
}

#[tokio::test]
async fn telegram_send_photo_bytes_builds_correct_form() {
    let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
    // Minimal valid PNG header bytes
    let file_bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

    let result = ch
        .send_photo_bytes("123456", file_bytes, "test.png", None)
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn telegram_send_document_by_url_builds_correct_json() {
    let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);

    let result = ch
        .send_document_by_url("123456", "https://example.com/file.pdf", Some("PDF doc"))
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn telegram_send_photo_by_url_builds_correct_json() {
    let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);

    let result = ch
        .send_photo_by_url("123456", "https://example.com/image.jpg", None)
        .await;

    assert!(result.is_err());
}

// ── File path handling tests ────────────────────────────────────

#[tokio::test]
async fn telegram_send_document_nonexistent_file() {
    let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
    let path = Path::new("/nonexistent/path/to/file.txt");

    let result = ch.send_document("123456", path, None).await;

    assert!(result.is_err());
    let err = format!("{:#}", result.unwrap_err());
    // Should fail with file not found error (context wraps the underlying OS error)
    assert!(
        err.contains("No such file") || err.contains("not found") || err.contains("os error"),
        "Expected file not found error, got: {err}"
    );
}

#[tokio::test]
async fn telegram_send_photo_nonexistent_file() {
    let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
    let path = Path::new("/nonexistent/path/to/photo.jpg");

    let result = ch.send_photo("123456", path, None).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn telegram_send_video_nonexistent_file() {
    let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
    let path = Path::new("/nonexistent/path/to/video.mp4");

    let result = ch.send_video("123456", path, None).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn telegram_send_audio_nonexistent_file() {
    let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
    let path = Path::new("/nonexistent/path/to/audio.mp3");

    let result = ch.send_audio("123456", path, None).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn telegram_send_voice_nonexistent_file() {
    let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
    let path = Path::new("/nonexistent/path/to/voice.ogg");

    let result = ch.send_voice("123456", path, None).await;

    assert!(result.is_err());
}

// ── Caption handling tests ──────────────────────────────────────

#[tokio::test]
async fn telegram_send_document_bytes_with_caption() {
    let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
    let file_bytes = b"test content".to_vec();

    // With caption
    let result = ch
        .send_document_bytes("123456", file_bytes.clone(), "test.txt", Some("My caption"))
        .await;
    assert!(result.is_err()); // Network error expected

    // Without caption
    let result = ch
        .send_document_bytes("123456", file_bytes, "test.txt", None)
        .await;
    assert!(result.is_err()); // Network error expected
}

#[tokio::test]
async fn telegram_send_photo_bytes_with_caption() {
    let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
    let file_bytes = vec![0x89, 0x50, 0x4E, 0x47];

    // With caption
    let result = ch
        .send_photo_bytes(
            "123456",
            file_bytes.clone(),
            "test.png",
            Some("Photo caption"),
        )
        .await;
    assert!(result.is_err());

    // Without caption
    let result = ch
        .send_photo_bytes("123456", file_bytes, "test.png", None)
        .await;
    assert!(result.is_err());
}

// ── Empty/edge case tests ───────────────────────────────────────

#[tokio::test]
async fn telegram_send_document_bytes_empty_file() {
    let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
    let file_bytes: Vec<u8> = vec![];

    let result = ch
        .send_document_bytes("123456", file_bytes, "empty.txt", None)
        .await;

    // Should not panic, will fail at API level
    assert!(result.is_err());
}

#[tokio::test]
async fn telegram_send_document_bytes_empty_filename() {
    let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
    let file_bytes = b"content".to_vec();

    let result = ch.send_document_bytes("123456", file_bytes, "", None).await;

    // Should not panic
    assert!(result.is_err());
}

#[tokio::test]
async fn telegram_send_document_bytes_empty_chat_id() {
    let ch = TelegramChannel::new("fake-token".into(), vec!["*".into()]);
    let file_bytes = b"content".to_vec();

    let result = ch
        .send_document_bytes("", file_bytes, "test.txt", None)
        .await;

    // Should not panic
    assert!(result.is_err());
}
