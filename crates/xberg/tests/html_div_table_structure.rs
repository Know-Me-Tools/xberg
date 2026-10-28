#![cfg(feature = "html")]

mod helpers;

use helpers::extract_bytes_document;
use xberg::core::config::{ExtractionConfig, OutputFormat};
use xberg::types::document_structure::NodeContent;

async fn assert_prose_around_table(format: OutputFormat, tag: &str) {
    let html = format!(
        "<html><body><{tag}>PROSE ONE</{tag}><table><tr><td>cell1</td><td>cell2</td></tr></table>\
         <{tag}>PROSE TWO</{tag}></body></html>"
    );
    let config = ExtractionConfig {
        use_cache: false,
        include_document_structure: true,
        output_format: format,
        ..Default::default()
    };
    let result = extract_bytes_document(html.as_bytes(), "text/html", &config)
        .await
        .expect("valid HTML extracts");
    let document = result.document.expect("requested document structure");
    let sequence: Vec<&str> = document
        .nodes
        .iter()
        .filter_map(|node| match &node.content {
            NodeContent::Paragraph { text } => Some(text.as_str()),
            NodeContent::Table { .. } => Some("TABLE"),
            _ => None,
        })
        .collect();
    assert_eq!(sequence, ["PROSE ONE", "TABLE", "PROSE TWO"]);
    assert_eq!(result.tables.len(), 1);
    assert_eq!(result.tables[0].cells, [vec!["cell1".to_string(), "cell2".to_string()]]);
    for text in ["PROSE ONE", "cell1", "cell2", "PROSE TWO"] {
        assert_eq!(
            result.content.matches(text).count(),
            1,
            "{text} must occur exactly once"
        );
    }
}

#[tokio::test]
async fn should_keep_div_prose_and_table_in_plain_document_structure_gh2301() {
    assert_prose_around_table(OutputFormat::Plain, "div").await;
}

#[tokio::test]
async fn should_keep_paragraph_and_markdown_controls_gh2301() {
    assert_prose_around_table(OutputFormat::Plain, "p").await;
    assert_prose_around_table(OutputFormat::Markdown, "div").await;
}
