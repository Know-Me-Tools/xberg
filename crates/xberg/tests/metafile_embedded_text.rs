//! Native text records in embedded metafiles must not be sent to a raster OCR decoder.
#![cfg(all(feature = "office", feature = "ocr", feature = "tokio-runtime"))]

mod helpers;

use std::io::{Cursor, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;

use helpers::extract_bytes_document;
use xberg::core::config::{ExtractionConfig, ImageExtractionConfig, OcrConfig, OutputFormat};
use xberg::plugins::{OcrBackend, OcrBackendType, Plugin, register_ocr_backend, unregister_ocr_backend};

const RASTER_FAILURE: &str = "gh2297 counted raster control invoked";
const PPTX_MIME: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";

struct CountingRejectingBackend {
    name: &'static str,
    calls: Arc<AtomicUsize>,
}

impl Plugin for CountingRejectingBackend {
    fn name(&self) -> &str {
        self.name
    }
    fn version(&self) -> String {
        "0.0.0".to_string()
    }
    fn initialize(&self) -> xberg::Result<()> {
        Ok(())
    }
    fn shutdown(&self) -> xberg::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl OcrBackend for CountingRejectingBackend {
    fn backend_type(&self) -> OcrBackendType {
        OcrBackendType::Custom
    }
    fn supports_language(&self, _: &str) -> bool {
        true
    }
    async fn process_image(&self, _: &[u8], _: &OcrConfig) -> xberg::Result<xberg::ExtractedDocument> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(xberg::XbergError::Ocr {
            message: RASTER_FAILURE.to_string(),
            source: None,
        })
    }
}

struct BackendGuard {
    name: &'static str,
    calls: Arc<AtomicUsize>,
}

impl Drop for BackendGuard {
    fn drop(&mut self) {
        let _ = unregister_ocr_backend(self.name);
    }
}

fn register_backend(name: &'static str) -> BackendGuard {
    let calls = Arc::new(AtomicUsize::new(0));
    register_ocr_backend(Arc::new(CountingRejectingBackend {
        name,
        calls: calls.clone(),
    }))
    .expect("counted raster backend must register");
    BackendGuard { name, calls }
}

fn record(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = kind.to_le_bytes().to_vec();
    bytes.extend_from_slice(&((payload.len() + 8) as u32).to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn emf(text: Option<&str>) -> Vec<u8> {
    let mut records = Vec::new();
    if let Some(text) = text {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut payload = vec![0u8; 68];
        payload[16..20].copy_from_slice(&1u32.to_le_bytes());
        payload[36..40].copy_from_slice(&(wide.len() as u32).to_le_bytes());
        payload[40..44].copy_from_slice(&76u32.to_le_bytes());
        for character in wide {
            payload.extend_from_slice(&character.to_le_bytes());
        }
        while (payload.len() + 8) % 4 != 0 {
            payload.push(0);
        }
        records.extend(record(84, &payload));
    }
    records.extend(record(14, &[0u8; 12]));
    let mut header = vec![0u8; 88];
    header[0..4].copy_from_slice(&1u32.to_le_bytes());
    header[4..8].copy_from_slice(&88u32.to_le_bytes());
    header[40..44].copy_from_slice(b" EMF");
    header[44..48].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    let total_size = (header.len() + records.len()) as u32;
    header[48..52].copy_from_slice(&total_size.to_le_bytes());
    header[52..56].copy_from_slice(&(if text.is_some() { 3u32 } else { 2u32 }).to_le_bytes());
    header[56..58].copy_from_slice(&1u16.to_le_bytes());
    header.extend(records);
    header
}

fn wmf(text: &str) -> Vec<u8> {
    assert!(text.is_ascii());
    let mut payload = (text.len() as u16).to_le_bytes().to_vec();
    payload.extend_from_slice(text.as_bytes());
    if text.len() % 2 != 0 {
        payload.push(0);
    }
    payload.extend_from_slice(&[0u8; 4]);
    let words = (payload.len() + 6) / 2;
    let mut records = (words as u32).to_le_bytes().to_vec();
    records.extend_from_slice(&0x0521u16.to_le_bytes());
    records.extend(payload);
    records.extend_from_slice(&3u32.to_le_bytes());
    records.extend_from_slice(&0u16.to_le_bytes());
    let mut header = 1u16.to_le_bytes().to_vec();
    header.extend_from_slice(&9u16.to_le_bytes());
    header.extend_from_slice(&0x0300u16.to_le_bytes());
    header.extend_from_slice(&((18 + records.len()) as u32 / 2).to_le_bytes());
    header.extend_from_slice(&0u16.to_le_bytes());
    header.extend_from_slice(&(words as u32).to_le_bytes());
    header.extend_from_slice(&0u16.to_le_bytes());
    header.extend(records);
    header
}

fn pptx(image: &[u8], extension: &str) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    let parts = [
        ("[Content_Types].xml", r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>"#.to_string()),
        ("ppt/presentation.xml", r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#.to_string()),
        ("ppt/_rels/presentation.xml.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#.to_string()),
        ("ppt/slides/slide1.xml", r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:cSld><p:spTree><p:pic><p:nvPicPr><p:cNvPr id="2" name="chart"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId1"/></p:blipFill><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1000" cy="1000"/></a:xfrm></p:spPr></p:pic></p:spTree></p:cSld></p:sld>"#.to_string()),
        ("ppt/slides/_rels/slide1.xml.rels", format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.{extension}"/></Relationships>"#)),
    ];
    for (name, text) in parts {
        writer.start_file(name, options).unwrap();
        writer.write_all(text.as_bytes()).unwrap();
    }
    writer
        .start_file(format!("ppt/media/image1.{extension}"), options)
        .unwrap();
    writer.write_all(image).unwrap();
    writer.finish().unwrap().into_inner()
}

fn config(backend: &BackendGuard) -> ExtractionConfig {
    ExtractionConfig {
        use_cache: false,
        output_format: OutputFormat::Plain,
        images: Some(ImageExtractionConfig {
            extract_images: true,
            ..Default::default()
        }),
        ocr: Some(OcrConfig {
            backend: backend.name.to_string(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn assert_no_raster_failure(document: &xberg::ExtractedDocument) {
    assert!(
        !document
            .processing_warnings
            .iter()
            .any(|warning| warning.source == "image_ocr"),
        "{:?}",
        document.processing_warnings
    );
}

#[tokio::test]
async fn empty_emf_picture_should_not_attempt_raster_ocr_gh2297() {
    let backend = register_backend("gh2297-empty-emf-raster-control");
    let image = emf(None);
    let result = extract_bytes_document(&pptx(&image, "emf"), PPTX_MIME, &config(&backend))
        .await
        .unwrap();
    assert_eq!(
        backend.calls.load(Ordering::SeqCst),
        0,
        "metafiles must bypass raster OCR"
    );
    assert_no_raster_failure(&result);
    let images = result.images.as_ref().unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].data.as_ref(), image);
}

#[tokio::test]
async fn emf_picture_should_preserve_unicode_text_and_original_bytes_gh2297() {
    let backend = register_backend("gh2297-unicode-emf-raster-control");
    let text = "Revenue Δ 42 🧪";
    let image = emf(Some(text));
    let result = extract_bytes_document(&pptx(&image, "emf"), PPTX_MIME, &config(&backend))
        .await
        .unwrap();
    assert_eq!(
        backend.calls.load(Ordering::SeqCst),
        0,
        "metafiles must bypass raster OCR"
    );
    assert_no_raster_failure(&result);
    assert_eq!(result.content.matches(text).count(), 1, "{}", result.content);
    let images = result.images.as_ref().unwrap();
    assert_eq!(images[0].data.as_ref(), image);
    assert!(!matches!(
        images[0].ocr_result.as_ref().unwrap().metadata.format.as_ref(),
        Some(xberg::FormatMetadata::Ocr(_))
    ));
}

#[tokio::test]
async fn standard_wmf_picture_should_preserve_text_without_raster_ocr_gh2297() {
    let backend = register_backend("gh2297-wmf-raster-control");
    let text = "Quarterly chart 42";
    let result = extract_bytes_document(&pptx(&wmf(text), "wmf"), PPTX_MIME, &config(&backend))
        .await
        .unwrap();
    assert_eq!(
        backend.calls.load(Ordering::SeqCst),
        0,
        "metafiles must bypass raster OCR"
    );
    assert_no_raster_failure(&result);
    assert_eq!(result.content.matches(text).count(), 1, "{}", result.content);
}

#[tokio::test]
async fn rtf_metafile_picture_should_preserve_native_text_gh2297() {
    let backend = register_backend("gh2297-rtf-raster-control");
    for (kind, image, text) in [
        ("emfblip", emf(Some("EMF label")), "EMF label"),
        ("wmetafile8", wmf("WMF label"), "WMF label"),
    ] {
        let rtf = format!(
            r"{{\rtf1 Before\par {{\pict\{kind} {}}}\par After}}",
            hex::encode(image)
        );
        let result = extract_bytes_document(rtf.as_bytes(), "application/rtf", &config(&backend))
            .await
            .unwrap();
        assert_eq!(
            backend.calls.load(Ordering::SeqCst),
            0,
            "metafiles must bypass raster OCR"
        );
        assert_no_raster_failure(&result);
        assert_eq!(result.content.matches(text).count(), 1, "{}", result.content);
    }
}

#[tokio::test]
async fn malformed_metafile_should_report_native_parse_failure_without_raster_noise_gh2297() {
    let backend = register_backend("gh2297-malformed-raster-control");
    let mut image = emf(Some("label"));
    image[92..96].copy_from_slice(&u32::MAX.to_le_bytes());
    let result = extract_bytes_document(&pptx(&image, "emf"), PPTX_MIME, &config(&backend))
        .await
        .unwrap();
    assert_eq!(
        backend.calls.load(Ordering::SeqCst),
        0,
        "metafiles must bypass raster OCR"
    );
    assert_no_raster_failure(&result);
    assert!(
        result
            .processing_warnings
            .iter()
            .any(|warning| warning.source == "metafile_text"),
        "{:?}",
        result.processing_warnings
    );
}

#[tokio::test]
async fn raster_picture_should_still_reach_configured_ocr_backend_gh2297() {
    let backend = register_backend("gh2297-png-raster-control");
    let image = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\nIDATx\x9cc\x00\x01\x00\x00\x05\x00\x01\r\n-\xb4\x00\x00\x00\x00IEND\xaeB`\x82";
    let result = extract_bytes_document(&pptx(image, "png"), PPTX_MIME, &config(&backend))
        .await
        .unwrap();
    assert_eq!(
        backend.calls.load(Ordering::SeqCst),
        1,
        "the raster control must invoke the backend exactly once"
    );
    assert!(
        result
            .processing_warnings
            .iter()
            .any(|warning| warning.source == "image_ocr" && warning.message.contains(RASTER_FAILURE))
    );
}

fn zip_parts(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in parts {
        writer.start_file(*name, options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn docx(image: &[u8], extension: &str) -> Vec<u8> {
    let document = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
        xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
        xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
        xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"
        xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
      <w:body><w:p><w:r><w:drawing><wp:inline>
        <wp:extent cx="914400" cy="914400"/><wp:docPr id="1" name="Chart"/>
        <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">
          <pic:pic><pic:blipFill><a:blip r:embed="rIdImage"/></pic:blipFill></pic:pic>
        </a:graphicData></a:graphic>
      </wp:inline></w:drawing></w:r></w:p></w:body></w:document>"#;
    let types = format!(
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
      <Default Extension="xml" ContentType="application/xml"/>
      <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
      <Default Extension="{extension}" ContentType="image/x-{extension}"/>
      <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
    </Types>"#
    );
    let root_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
      <Relationship Id="rIdRoot" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
    </Relationships>"#;
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
      <Relationship Id="rIdImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.{extension}"/>
    </Relationships>"#
    );
    let image_path = format!("word/media/image1.{extension}");
    zip_parts(&[
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
        (&image_path, image),
    ])
}

fn odf(image: &[u8], extension: &str, presentation: bool) -> Vec<u8> {
    let mime = if presentation {
        "application/vnd.oasis.opendocument.presentation"
    } else {
        "application/vnd.oasis.opendocument.text"
    };
    let frame = format!(
        r#"<draw:frame draw:name="Chart" text:anchor-type="as-char" svg:width="2cm" svg:height="2cm">
      <draw:image xlink:href="Pictures/image1.{extension}" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>
    </draw:frame>"#
    );
    let body = if presentation {
        format!("<office:presentation><draw:page draw:name=\"Only slide\">{frame}</draw:page></office:presentation>")
    } else {
        format!("<office:text><text:p>{frame}</text:p></office:text>")
    };
    let content = format!(
        r#"<office:document-content office:version="1.2"
        xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
        xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
        xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"
        xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"
        xmlns:xlink="http://www.w3.org/1999/xlink">
      <office:body>{body}</office:body></office:document-content>"#
    );
    let manifest = format!(
        r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">
      <manifest:file-entry manifest:full-path="/" manifest:media-type="{mime}"/>
      <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
      <manifest:file-entry manifest:full-path="Pictures/image1.{extension}" manifest:media-type="image/x-{extension}"/>
    </manifest:manifest>"#
    );
    let image_path = format!("Pictures/image1.{extension}");
    zip_parts(&[
        ("mimetype", mime.as_bytes()),
        ("content.xml", content.as_bytes()),
        ("META-INF/manifest.xml", manifest.as_bytes()),
        (&image_path, image),
    ])
}

async fn assert_container_metafiles(name: &'static str, mime: &str, container: fn(&[u8], &str) -> Vec<u8>) {
    let backend = register_backend(name);
    for (extension, image, text) in [
        ("emf", emf(Some("Revenue Δ 42 🧪")), "Revenue Δ 42 🧪"),
        ("wmf", wmf("Quarterly chart 42"), "Quarterly chart 42"),
        ("emf", emf(None), ""),
        ("wmf", wmf(""), ""),
    ] {
        let result = extract_bytes_document(&container(&image, extension), mime, &config(&backend))
            .await
            .unwrap();
        assert_eq!(
            backend.calls.load(Ordering::SeqCst),
            0,
            "{mime}: {extension} must bypass raster OCR"
        );
        assert_no_raster_failure(&result);
        assert!(
            !result
                .processing_warnings
                .iter()
                .any(|warning| warning.source == "metafile_text"),
            "{:?}",
            result.processing_warnings
        );
        let images = result.images.as_ref().expect("requested image bytes must be retained");
        assert_eq!(images.len(), 1, "{mime}: exactly one embedded picture");
        assert_eq!(images[0].data.as_ref(), image, "{mime}: native image bytes");
        let native = images[0]
            .ocr_result
            .as_ref()
            .expect("the native text carrier must exist");
        assert_eq!(native.content, text, "{mime}: native text including the empty case");
        assert!(!matches!(
            native.metadata.format.as_ref(),
            Some(xberg::FormatMetadata::Ocr(_))
        ));
        if !text.is_empty() {
            assert_eq!(result.content.matches(text).count(), 1, "{mime}: {}", result.content);
        }
    }
}

#[tokio::test]
async fn docx_metafiles_should_preserve_native_text_bytes_and_empty_pictures_gh2297() {
    assert_container_metafiles(
        "gh2297-docx-metafile-control",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        docx,
    )
    .await;
}

#[tokio::test]
async fn odt_metafiles_should_preserve_native_text_bytes_and_empty_pictures_gh2297() {
    assert_container_metafiles(
        "gh2297-odt-metafile-control",
        "application/vnd.oasis.opendocument.text",
        |image, extension| odf(image, extension, false),
    )
    .await;
}

#[tokio::test]
async fn odp_metafiles_should_preserve_native_text_bytes_and_empty_pictures_gh2297() {
    assert_container_metafiles(
        "gh2297-odp-metafile-control",
        "application/vnd.oasis.opendocument.presentation",
        |image, extension| odf(image, extension, true),
    )
    .await;
}
