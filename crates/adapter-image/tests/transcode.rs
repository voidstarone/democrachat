//! Behaviour of the re-encoding transcoder: stills are re-encoded (alpha→PNG,
//! opaque→JPEG), animated GIFs pass through, HEIC becomes JPEG, and junk is
//! rejected.

use std::io::Cursor;

use app::{ImageTranscoder, MediaError};
use adapter_image::ReencodingTranscoder;
use image::{DynamicImage, ImageFormat, RgbImage, RgbaImage};

fn encode(img: &DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), format).unwrap();
    buf
}

/// An opaque image (no alpha) is re-encoded to JPEG.
#[test]
fn opaque_png_becomes_jpeg() {
    let png = encode(&DynamicImage::ImageRgb8(RgbImage::new(16, 16)), ImageFormat::Png);
    let (ct, bytes) = ReencodingTranscoder::new().normalize("image/png", &png).unwrap();
    assert_eq!(ct, "image/jpeg");
    assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::Jpeg);
}

/// An image with an alpha channel stays PNG so transparency is preserved.
#[test]
fn transparent_png_stays_png() {
    let mut rgba = RgbaImage::new(16, 16);
    rgba.put_pixel(0, 0, image::Rgba([255, 0, 0, 0])); // a transparent pixel
    let png = encode(&DynamicImage::ImageRgba8(rgba), ImageFormat::Png);
    let (ct, bytes) = ReencodingTranscoder::new().normalize("image/png", &png).unwrap();
    assert_eq!(ct, "image/png");
    let decoded = image::load_from_memory(&bytes).unwrap();
    assert!(decoded.color().has_alpha(), "alpha channel survives the round-trip");
}

/// A real JPEG is still re-encoded to a JPEG (metadata dropped) and stays valid.
#[test]
fn jpeg_is_reencoded() {
    let jpeg = encode(&DynamicImage::ImageRgb8(RgbImage::new(24, 24)), ImageFormat::Jpeg);
    let (ct, bytes) = ReencodingTranscoder::new().normalize("image/jpeg", &jpeg).unwrap();
    assert_eq!(ct, "image/jpeg");
    assert!(image::load_from_memory(&bytes).is_ok());
}

/// An animated GIF is passed through untouched so the animation survives.
#[test]
fn animated_gif_passes_through() {
    // Two frames → the encoder writes the NETSCAPE looping extension.
    let mut buf = Vec::new();
    {
        use image::codecs::gif::GifEncoder;
        use image::{Delay, Frame};
        let mut enc = GifEncoder::new(&mut buf);
        enc.set_repeat(image::codecs::gif::Repeat::Infinite).unwrap();
        for _ in 0..2 {
            let frame = Frame::from_parts(RgbaImage::new(8, 8), 0, 0, Delay::from_numer_denom_ms(100, 1));
            enc.encode_frame(frame).unwrap();
        }
    }
    let (ct, bytes) = ReencodingTranscoder::new().normalize("image/gif", &buf).unwrap();
    assert_eq!(ct, "image/gif");
    assert_eq!(bytes, buf, "animated GIF bytes are stored verbatim");
}

/// A HEIC file (which browsers cannot display) is converted to JPEG.
///
/// Skips (rather than fails) on a broken system decoder: libheif ≥ 1.23 imports
/// libde265 ≥ 1.1.1's new per-decoder pixel limit but fails to raise it, so HEIC
/// decode is unavailable on that (bleeding-edge, e.g. Homebrew) stack. The deploy
/// target — Debian Trixie's libheif 1.19.8 + libde265 1.0.x — predates the limit
/// and decodes fine (this test passes there).
#[test]
fn heic_becomes_jpeg() {
    let heic = include_bytes!("fixtures/sample.heic");
    match ReencodingTranscoder::new().normalize("image/heic", heic) {
        Ok((ct, bytes)) => {
            assert_eq!(ct, "image/jpeg");
            let decoded = image::load_from_memory(&bytes).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (32, 32));
        }
        Err(MediaError::Undecodable) => {
            eprintln!("skipping HEIC decode: system libheif/libde265 cannot decode HEIC here");
        }
        Err(e) => panic!("unexpected error decoding HEIC: {e:?}"),
    }
}

/// Bytes that don't decode as a real image are rejected, not stored.
#[test]
fn undecodable_bytes_are_rejected() {
    let err = ReencodingTranscoder::new()
        .normalize("image/png", b"\x89PNG\r\n\x1a\n not really a png")
        .unwrap_err();
    assert!(matches!(err, MediaError::Undecodable));
    // Empty input is likewise refused, not stored as a zero-byte blob.
    assert!(matches!(
        ReencodingTranscoder::new().normalize("image/png", &[]),
        Err(MediaError::Undecodable),
    ));
}

/// Decode-bomb guard: a tiny file that *declares* a canvas past the per-side limit
/// is rejected on the cheap dimension read, before any full decode/allocation. The
/// encoded PNG here is a few hundred bytes but claims 12_001 px of width.
#[test]
fn an_oversized_canvas_is_rejected_before_decode() {
    // MAX_DIMENSION is 12_000; one pixel tall keeps the fixture small to encode.
    let bomb = encode(&DynamicImage::ImageRgb8(RgbImage::new(12_001, 1)), ImageFormat::Png);
    assert!(matches!(
        ReencodingTranscoder::new().normalize("image/png", &bomb),
        Err(MediaError::Undecodable),
    ));
}

/// The re-encode is driven by the *bytes*, not the caller's declared content type:
/// a real PNG mislabelled `image/gif` is still decoded on its true bytes and
/// re-encoded to JPEG. A spoofed header can't route an image around the encoder.
#[test]
fn a_spoofed_content_type_does_not_steer_the_encoder() {
    let png = encode(&DynamicImage::ImageRgb8(RgbImage::new(8, 8)), ImageFormat::Png);
    let (ct, bytes) = ReencodingTranscoder::new().normalize("image/gif", &png).unwrap();
    assert_eq!(ct, "image/jpeg", "the true bytes decide, not the label");
    assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::Jpeg);
}
