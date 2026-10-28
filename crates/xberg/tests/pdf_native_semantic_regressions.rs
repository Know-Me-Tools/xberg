#![cfg(feature = "pdf")]

mod helpers;

use helpers::extract_bytes_document_blocking;
use lopdf::{Document, Object, Stream, dictionary};
use xberg::core::config::{ContentFilterConfig, ExtractionConfig, OutputFormat, PdfConfig};
use xberg::types::document_structure::{ContentLayer, NodeContent};

fn pdf(pages: &[String]) -> Vec<u8> {
    let mut document = Document::with_version("1.7");
    let parent = document.new_object_id();
    let regular = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let bold = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica-Bold",
    });
    let children = pages
        .iter()
        .map(|content| {
            let stream = document.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
            document
                .add_object(dictionary! {
                    "Type" => "Page", "Parent" => parent, "Contents" => stream,
                    "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                    "Resources" => dictionary! { "Font" => dictionary! { "F1" => regular, "F2" => bold } },
                })
                .into()
        })
        .collect::<Vec<Object>>();
    document.objects.insert(
        parent,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Count" => pages.len() as i64, "Kids" => children,
        }),
    );
    let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => parent });
    document.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    bytes
}

fn extract(bytes: &[u8], format: OutputFormat, filter: Option<ContentFilterConfig>) -> xberg::ExtractedDocument {
    let config = ExtractionConfig {
        output_format: format,
        include_document_structure: true,
        disable_ocr: true,
        use_cache: false,
        enable_quality_processing: false,
        content_filter: filter,
        pdf_options: Some(PdfConfig {
            extract_tables: false,
            ..Default::default()
        }),
        ..Default::default()
    };
    extract_bytes_document_blocking(bytes, "application/pdf", &config).unwrap()
}

fn heading_pdf(sizes: &[u8]) -> Vec<u8> {
    let headings = sizes
        .iter()
        .enumerate()
        .map(|(index, size)| (*size, format!("Section {}", index + 1)))
        .collect::<Vec<_>>();
    named_heading_pdf(&headings)
}

fn named_heading_pdf(headings: &[(u8, String)]) -> Vec<u8> {
    let mut content = String::new();
    let mut y = 740;
    for (index, (size, title)) in headings.iter().enumerate() {
        content.push_str(&format!("BT /F2 {size} Tf 72 {y} Td ({title}) Tj ET\n"));
        for line in 1..=3 {
            y -= 15;
            content.push_str(&format!(
                "BT /F1 9 Tf 72 {y} Td (Ordinary body prose explains item {} with enough words to establish the body font.) Tj ET\n",
                index * 3 + line
            ));
        }
        y -= 35;
    }
    pdf(&[content])
}

fn assert_heading_levels(sizes: &[u8]) {
    let bytes = heading_pdf(sizes);
    let expected = (1..=sizes.len())
        .map(|level| (level as u8, format!("Section {level}")))
        .collect::<Vec<_>>();
    for format in [OutputFormat::Plain, OutputFormat::Markdown] {
        let result = extract(&bytes, format, None);
        let headings = result
            .document
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter_map(|node| match &node.content {
                NodeContent::Heading { level, text } => Some((*level, text.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            headings, expected,
            "font tiers must retain distinct heading depths: {}",
            result.content
        );
        assert_eq!(result.content.matches("Ordinary body prose").count(), sizes.len() * 3);
    }
}

#[test]
fn should_preserve_four_font_heading_levels_in_plain_and_markdown_gh2305() {
    assert_heading_levels(&[30, 22, 16, 13]);
}

#[test]
fn should_preserve_six_font_heading_levels_without_promoting_body_text_gh2305() {
    assert_heading_levels(&[30, 26, 22, 18, 15, 13]);
}

#[test]
fn should_preserve_same_font_semantic_subsections_beside_distinct_font_tiers_gh2305() {
    let headings = [
        (30, "Report Title"),
        (22, "1. Main Section"),
        (22, "Inner detail"),
        (22, "2. Next Section"),
        (22, "3. Final Section"),
        (16, "Deep Topic"),
    ]
    .map(|(size, title)| (size, title.to_owned()));
    let bytes = named_heading_pdf(&headings);
    let result = extract(&bytes, OutputFormat::Markdown, None);
    let nodes = &result.document.as_ref().unwrap().nodes;
    let level = |needle: &str| {
        nodes.iter().find_map(|node| match &node.content {
            NodeContent::Heading { level, text } if text == needle => Some(*level),
            _ => None,
        })
    };
    assert_eq!(level("1. Main Section"), Some(2));
    assert_eq!(
        level("Inner detail"),
        Some(3),
        "same-font semantic nesting must survive font ranking"
    );
    assert_eq!(level("2. Next Section"), Some(2));
    assert_eq!(level("3. Final Section"), Some(2));
}

fn repeated_line_pdf(pages: usize, line_y: usize) -> Vec<u8> {
    let contents = (1..=pages).map(|page| {
        let mut content = format!("BT /F1 9 Tf 72 {line_y} Td (ACME Confidential Annual Report) Tj ET\n");
        for line in 1..=6 {
            let y = 650 - line * 24;
            content.push_str(&format!(
                "BT /F1 11 Tf 72 {y} Td (Page {page} body row {line} describes delivery and storage of ordinary materials.) Tj ET\n"
            ));
        }
        content
    }).collect::<Vec<_>>();
    pdf(&contents)
}

#[test]
fn should_classify_and_filter_three_page_running_headers_independently_of_deduplication_gh2304() {
    let bytes = repeated_line_pdf(3, 760);
    let mut cases = 0;
    for format in [OutputFormat::Plain, OutputFormat::Markdown] {
        for strip_repeating_text in [false, true] {
            for include_headers in [false, true] {
                let result = extract(
                    &bytes,
                    format.clone(),
                    Some(ContentFilterConfig {
                        include_headers,
                        strip_repeating_text,
                        ..Default::default()
                    }),
                );
                assert_eq!(
                    result.content.matches("ACME Confidential Annual Report").count(),
                    if include_headers { 3 } else { 0 },
                    "format={format:?}, strip={strip_repeating_text}, include_headers={include_headers}"
                );
                for page in 1..=3 {
                    for row in 1..=6 {
                        assert_eq!(
                            result.content.matches(&format!("Page {page} body row {row}")).count(),
                            1
                        );
                    }
                }
                let headers = result
                    .document
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .filter(|node| node.content_layer == ContentLayer::Header)
                    .count();
                assert_eq!(headers, if include_headers { 3 } else { 0 });
                let public_structure = serde_json::to_string(result.document.as_ref().unwrap()).unwrap();
                assert!(!public_structure.contains("xberg:internal:render-retained-furniture"));
                assert!(!result.content.contains("xberg:internal:render-retained-furniture"));
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 8);
}

#[test]
fn should_preserve_repeated_body_prose_and_headers_without_enough_pages_gh2304() {
    for (pages, y) in [(3, 690), (2, 760), (1, 760)] {
        let bytes = repeated_line_pdf(pages, y);
        let result = extract(
            &bytes,
            OutputFormat::Plain,
            Some(ContentFilterConfig {
                include_headers: false,
                strip_repeating_text: true,
                ..Default::default()
            }),
        );
        assert_eq!(result.content.matches("ACME Confidential Annual Report").count(), pages);
    }
}
