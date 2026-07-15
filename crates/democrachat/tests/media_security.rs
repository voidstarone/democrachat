//! Security tests for the media-upload guard ([`Services::store_media`] and the
//! attachment cap). The upload path is a classic XSS / content-sniffing surface:
//! a browser served an attacker-chosen SVG or HTML blob same-origin would run it.
//! These tests pin the refusals that stop that — declared-type *and* byte-sniffed
//! markup rejection, the size and MIME allow-list, and the per-message attachment
//! cap — independently of the (production-only) re-encoding transcoder, which the
//! in-memory bundle stands in for with a passthrough.

use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{MediaError, MessageError, Services};
use domain::{Attachment, MediaKind, Timestamp};

const DAY: i64 = 86_400;
/// Mirrors `MAX_MEDIA_BYTES` in the chat service (25 MiB).
const MAX_MEDIA_BYTES: usize = 25 * 1024 * 1024;
/// Mirrors `MAX_ATTACHMENTS` in the chat service.
const MAX_ATTACHMENTS: usize = 10;

struct Fixture {
    services: Services,
}

fn fixture() -> Fixture {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(1_000 * DAY)));
    Fixture { services: Services::new(clock, store.as_stores()) }
}

/// A founder with a #general to post into.
async fn founded() -> Fixture {
    let f = fixture();
    f.services.register_account("boss").await.unwrap();
    f.services.found_server("boss", "Town").await.unwrap();
    f
}

// ─────────────────────────────────────────────────────────────────────────────
// Size and emptiness
// ─────────────────────────────────────────────────────────────────────────────

/// An empty upload is refused, never stored as a zero-byte blob.
#[tokio::test]
async fn an_empty_upload_is_refused() {
    let f = fixture();
    assert!(matches!(f.services.chat().store_media("image/png", &[]), Err(MediaError::Empty)));
}

/// An upload over the size cap is refused before anything is stored.
#[tokio::test]
async fn an_oversized_upload_is_refused() {
    let f = fixture();
    let huge = vec![0u8; MAX_MEDIA_BYTES + 1];
    assert!(matches!(f.services.chat().store_media("image/jpeg", &huge), Err(MediaError::TooLarge)));
}

// ─────────────────────────────────────────────────────────────────────────────
// Markup / XSS rejection — the crown jewel of the upload guard
// ─────────────────────────────────────────────────────────────────────────────

/// An SVG is refused outright: nominally an image, it can carry script, and served
/// same-origin it is an XSS vector.
#[tokio::test]
async fn an_svg_upload_is_refused() {
    let f = fixture();
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#;
    assert!(matches!(
        f.services.chat().store_media("image/svg+xml", svg),
        Err(MediaError::UnsupportedType(_)),
    ));
}

/// Byte-sniffing beats a spoofed content type: markup declared as `image/png` is
/// still recognized by its opening tag and refused. Covers each opener the guard
/// screens, plus the BOM- and whitespace-prefixed evasions.
#[tokio::test]
async fn markup_is_rejected_even_when_declared_an_image() {
    let f = fixture();
    let cases: &[&[u8]] = &[
        b"<script>alert(1)</script>",
        b"<html><body>hi</body></html>",
        b"<!doctype html>",
        b"<?xml version=\"1.0\"?><svg/>",
        b"   \n\t<svg xmlns=\"http://www.w3.org/2000/svg\"/>", // leading whitespace
        b"\xEF\xBB\xBF<script>alert(1)</script>",             // UTF-8 BOM prefix
    ];
    for bytes in cases {
        assert!(
            matches!(f.services.chat().store_media("image/png", bytes), Err(MediaError::UnsupportedType(_))),
            "markup blob {:?} must be refused",
            String::from_utf8_lossy(&bytes[..bytes.len().min(16)]),
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MIME allow-list
// ─────────────────────────────────────────────────────────────────────────────

/// Only image/video/audio are accepted; any other top-level type is refused.
#[tokio::test]
async fn non_media_mime_types_are_refused() {
    let f = fixture();
    for ct in ["application/zip", "text/html", "application/octet-stream", "text/plain"] {
        assert!(
            matches!(f.services.chat().store_media(ct, b"\x00\x01\x02not-markup"), Err(MediaError::UnsupportedType(_))),
            "`{ct}` is not a media type and must be refused",
        );
    }
}

/// A charset/parameter suffix on the content type is stripped before the type is
/// classified, and does not defeat acceptance of a real media type.
#[tokio::test]
async fn a_content_type_parameter_is_stripped() {
    let f = fixture();
    // Passthrough transcoder: the (non-markup) bytes are stored verbatim under the
    // base type, proving the `; charset=…` suffix was ignored, not rejected.
    let (key, stored_ct, kind) = f
        .services.chat()
        .store_media("image/png; charset=binary", b"\x89PNG\r\n\x1a\nnot-a-real-decode")
        .unwrap();
    assert_eq!(stored_ct, "image/png", "the parameter is dropped from the stored type");
    assert_eq!(kind, MediaKind::Image);
    // And it round-trips out of the blob store.
    let (blob_ct, _bytes) = f.services.chat().media_blob(&key).expect("the blob is retrievable");
    assert_eq!(blob_ct, "image/png");
}

/// Video and audio are accepted and stored verbatim (only images are re-encoded).
#[tokio::test]
async fn video_and_audio_are_accepted() {
    let f = fixture();
    let (_k, ct, kind) = f.services.chat().store_media("video/mp4", b"\x00\x00\x00\x18ftypmp42").unwrap();
    assert_eq!((ct.as_str(), kind), ("video/mp4", MediaKind::Video));
    let (_k, ct, kind) = f.services.chat().store_media("audio/mpeg", b"ID3\x03\x00\x00\x00").unwrap();
    assert_eq!((ct.as_str(), kind), ("audio/mpeg", MediaKind::Audio));
}

// ─────────────────────────────────────────────────────────────────────────────
// Attachment cap
// ─────────────────────────────────────────────────────────────────────────────

/// A message may not carry more than the attachment cap — a flood of references is
/// refused before the message is recorded.
#[tokio::test]
async fn too_many_attachments_are_refused() {
    let f = founded().await;
    let att = |i: usize| Attachment::new(format!("key{i}"), "image/png", MediaKind::Image, "", false);
    let over: Vec<Attachment> = (0..=MAX_ATTACHMENTS).map(att).collect();
    assert_eq!(over.len(), MAX_ATTACHMENTS + 1);
    assert!(matches!(
        f.services.chat().post_message_with_attachments("boss", "town", "general", "", over).await,
        Err(MessageError::TooManyAttachments(_)),
    ));
    // Exactly the cap is allowed.
    let ok: Vec<Attachment> = (0..MAX_ATTACHMENTS).map(att).collect();
    assert!(f
        .services.chat()
        .post_message_with_attachments("boss", "town", "general", "look", ok)
        .await
        .is_ok());
}
