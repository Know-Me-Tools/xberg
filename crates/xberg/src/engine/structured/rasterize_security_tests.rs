use std::io::Cursor;
use std::sync::Arc;

use super::{RasterizeError, pages_for_call, pages_for_call_with_security_limits, render_all_pages};
use crate::engine::parsed::ParsedDocument;
use crate::extractors::security::SecurityLimits;
use crate::heuristics::StructuredCallMode;

const WIDTH: u32 = 256;
const HEIGHT: u32 = 128;
const RESTRICTED_LIMIT: usize = 400_000;
const PERMITTED_LIMIT: usize = 600_000;

fn rgb_png() -> Vec<u8> {
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(WIDTH, HEIGHT, image::Rgb([12, 34, 56])));
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("encode a valid small RGB fixture");
    bytes
}

fn limits(max_content_size: usize) -> SecurityLimits {
    SecurityLimits {
        max_content_size,
        ..Default::default()
    }
}

fn assert_image_budget_error(error: RasterizeError, expected_limit: usize) {
    assert!(matches!(error, RasterizeError::Image(_)));
    let message = error.to_string();
    assert!(message.contains("security_limits.max_content_size"), "{message}");
    assert!(message.contains(&format!("({expected_limit} bytes)")), "{message}");
}

#[tokio::test]
async fn caller_limit_rejects_png_encoding_after_a_valid_small_image_decodes() {
    let bytes = rgb_png();
    let decoded =
        crate::extraction::image_decode::decode_standard_image_with_security_limits(&bytes, &limits(RESTRICTED_LIMIT))
            .expect("the configured budget permits decoding this valid input");
    assert_eq!(decoded.as_bytes().len(), (WIDTH * HEIGHT * 3) as usize);

    let error = pages_for_call_with_security_limits(
        &bytes,
        "image/png",
        StructuredCallMode::VisionOnly,
        200,
        &limits(RESTRICTED_LIMIT),
    )
    .await
    .expect_err("the caller's lower budget must reject PNG encoding");
    assert_image_budget_error(error, RESTRICTED_LIMIT);
}

#[tokio::test]
async fn caller_limit_permits_a_valid_rgb_page_without_resizing_or_pixel_changes() {
    let pages = pages_for_call_with_security_limits(
        &rgb_png(),
        "image/png",
        StructuredCallMode::TextPlusVision,
        200,
        &limits(PERMITTED_LIMIT),
    )
    .await
    .expect("the configured budget permits decoding and PNG encoding");
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].page_number, 1);
    let output = image::load_from_memory(&pages[0].png_bytes).expect("decode the real output PNG");
    assert_eq!((output.width(), output.height()), (WIDTH, HEIGHT));
    let rgb = output.to_rgb8();
    assert_eq!(rgb.get_pixel(0, 0), &image::Rgb([12, 34, 56]));
    assert_eq!(rgb.get_pixel(WIDTH - 1, HEIGHT - 1), &image::Rgb([12, 34, 56]));
}

#[tokio::test]
async fn caller_limit_rejects_declared_dimensions_before_missing_pixel_decoding() {
    let bytes = crate::extraction::image_decode::bmp_with_declared_dimensions(100, 100);
    let error =
        pages_for_call_with_security_limits(&bytes, "image/bmp", StructuredCallMode::VisionOnly, 200, &limits(1_024))
            .await
            .expect_err("the image header exceeds the caller's budget before allocating pixels");
    assert!(error.to_string().contains("100x100"));
    assert_image_budget_error(error, 1_024);
}

#[tokio::test]
async fn nonvision_modes_do_not_decode_invalid_input_even_with_zero_budget() {
    for mode in [
        StructuredCallMode::Skip,
        StructuredCallMode::TextOnly,
        StructuredCallMode::TextOnlyWithVisionFallback,
    ] {
        let pages = pages_for_call_with_security_limits(&[], "image/png", mode, 200, &limits(0))
            .await
            .expect("nonvision modes must remain lazy");
        assert_eq!(pages.len(), 0);
    }
}

#[tokio::test]
async fn caller_selected_budget_retains_unsupported_mime_errors() {
    let error = pages_for_call_with_security_limits(
        &[],
        "application/zip",
        StructuredCallMode::VisionOnly,
        200,
        &limits(PERMITTED_LIMIT),
    )
    .await
    .expect_err("an image budget cannot enable an unsupported format");
    assert!(matches!(error, RasterizeError::UnsupportedMime(ref mime) if mime == "application/zip"));
}

#[tokio::test]
async fn trusted_sixteen_megapixel_jpeg_renders_only_with_explicit_larger_budget() {
    const LARGE_WIDTH: u32 = 4_625;
    const LARGE_HEIGHT: u32 = 3_496;
    const TRUSTED_BUDGET: usize = 128 * 1024 * 1024;
    let bytes = {
        let rgb = image::RgbImage::from_fn(LARGE_WIDTH, LARGE_HEIGHT, |x, y| {
            image::Rgb([(x % 251) as u8, (y % 241) as u8, 128])
        });
        let mut encoded = Vec::new();
        image::DynamicImage::ImageRgb8(rgb)
            .write_to(&mut Cursor::new(&mut encoded), image::ImageFormat::Jpeg)
            .expect("encode the bounded synthetic phone-photo fixture");
        encoded
    };
    let error = pages_for_call(&bytes, "image/jpeg", StructuredCallMode::VisionOnly, 200)
        .await
        .expect_err("the existing 100 MiB default remains enforced");
    assert!(error.to_string().contains("113445144 live image-processing bytes"));
    assert_image_budget_error(error, SecurityLimits::default().max_content_size);
    let pages = pages_for_call_with_security_limits(
        &bytes,
        "image/jpeg",
        StructuredCallMode::VisionOnly,
        200,
        &limits(TRUSTED_BUDGET),
    )
    .await
    .expect("a caller can explicitly permit the trusted image's 113445144-byte encoding charge");
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].page_number, 1);
    let output = image::load_from_memory(&pages[0].png_bytes).expect("the returned page is a real PNG");
    assert_eq!((output.width(), output.height()), (LARGE_WIDTH, LARGE_HEIGHT));
}

#[test]
fn parse_memos_keep_distinct_budgets_for_identical_source_and_cached_pages() {
    let bytes: Arc<[u8]> = Arc::from(rgb_png());
    let permissive = ParsedDocument::from_bytes_with_security_limits(bytes.clone(), &limits(PERMITTED_LIMIT))
        .expect("construct a permissive memo without rendering");
    let strict = ParsedDocument::from_bytes_with_security_limits(bytes, &limits(RESTRICTED_LIMIT))
        .expect("construct a strict memo without rendering");
    let first = permissive
        .rendered_pages(200)
        .expect("render within the permissive budget");
    let second = permissive
        .rendered_pages(200)
        .expect("retrieve the permissive memo's cached pages");
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_eq!(first[0].png_bytes, second[0].png_bytes);
    for dpi in [200, 150] {
        let error = strict
            .rendered_pages(dpi)
            .expect_err("the strict memo cannot reuse permissive pages");
        assert_image_budget_error(error, RESTRICTED_LIMIT);
    }
}

#[test]
fn parse_memo_owns_its_limits_independently_of_the_callers_later_changes() {
    let mut selected = limits(PERMITTED_LIMIT);
    let memo = ParsedDocument::from_bytes_with_security_limits(Arc::from(rgb_png()), &selected)
        .expect("construct a memo with its own limits snapshot");
    selected.max_content_size = 0;
    assert_eq!(selected.max_content_size, 0);
    let pages = memo
        .rendered_pages(200)
        .expect("the memo retains the originally selected limits");
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].page_number, 1);
}

#[test]
fn legacy_rendering_and_default_parse_memo_remain_byte_identical() {
    let bytes = rgb_png();
    let memo = ParsedDocument::from_bytes(Arc::from(bytes.as_slice())).expect("construct a default memo");
    let direct = render_all_pages(&bytes, "image/png", 200, &SecurityLimits::default())
        .expect("the existing default primitive succeeds");
    let cached = memo.rendered_pages(200).expect("the default memo succeeds");
    assert_eq!(direct.len(), 1);
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[0].png_bytes, direct[0].png_bytes);
}
