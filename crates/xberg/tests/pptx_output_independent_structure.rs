#![cfg(feature = "office")]

mod helpers;

use std::io::{Cursor, Write};

use helpers::extract_bytes_document;
use xberg::core::config::{ExtractionConfig, OutputFormat};
use xberg::types::document_structure::NodeContent;

const MIME: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";

fn presentation(region: &str) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    let slide = format!(
        r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                   xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
          <p:cSld><p:spTree>
            <p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr/>
              <p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
              <p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="6000000" cy="500000"/></a:xfrm></p:spPr>
              <p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr b="1"/>
                <a:t>Quarterly Results</a:t></a:r></a:p></p:txBody></p:sp>
            <p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="3" name="Table"/>
              <p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>
              <p:xfrm><a:off x="0" y="600000"/><a:ext cx="6000000" cy="3000000"/></p:xfrm>
              <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table">
                <a:tbl><a:tblPr/><a:tblGrid><a:gridCol w="3000000"/><a:gridCol w="3000000"/></a:tblGrid>
                  <a:tr h="300000"><a:tc><a:txBody><a:bodyPr/><a:lstStyle/>
                    <a:p><a:r><a:t>Region</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc>
                    <a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>Revenue</a:t></a:r></a:p>
                    </a:txBody><a:tcPr/></a:tc></a:tr>
                  <a:tr h="300000"><a:tc><a:txBody><a:bodyPr/><a:lstStyle/>
                    <a:p><a:r><a:t>{region}</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc>
                    <a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>100</a:t></a:r></a:p>
                    </a:txBody><a:tcPr/></a:tc></a:tr>
                </a:tbl></a:graphicData></a:graphic></p:graphicFrame>
          </p:spTree></p:cSld></p:sld>"#
    );
    let parts = [
        (
            "[Content_Types].xml",
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>"#,
        ),
        (
            "ppt/presentation.xml",
            r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#,
        ),
        (
            "ppt/_rels/presentation.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#,
        ),
        ("ppt/slides/slide1.xml", slide.as_str()),
    ];
    for (name, text) in parts {
        archive.start_file(name, options).unwrap();
        archive.write_all(text.as_bytes()).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

async fn extract(format: OutputFormat, region: &str) -> xberg::ExtractedDocument {
    let config = ExtractionConfig {
        use_cache: false,
        include_document_structure: true,
        output_format: format,
        ..Default::default()
    };
    extract_bytes_document(&presentation(region), MIME, &config)
        .await
        .expect("genuine one-slide PPTX extracts")
}

fn assert_structure(result: &xberg::ExtractedDocument, region: &str) {
    let document = result.document.as_ref().expect("requested structure");
    let headings: Vec<_> = document
        .nodes
        .iter()
        .filter_map(|node| match &node.content {
            NodeContent::Heading { level, text } => Some((*level, text.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(headings.len(), 1);
    assert!(headings[0].1.contains("Quarterly Results"));
    assert_eq!(
        document
            .nodes
            .iter()
            .filter(|n| matches!(n.content, NodeContent::Table { .. }))
            .count(),
        1
    );
    assert_eq!(result.tables.len(), 1);
    assert_eq!(result.tables[0].cells, [vec!["Region", "Revenue"], vec![region, "100"]]);
}

#[tokio::test]
async fn should_keep_plain_slide_title_and_table_as_structural_nodes_gh2302() {
    let result = extract(OutputFormat::Plain, "North").await;
    assert_structure(&result, "North");
    assert_eq!(result.content.matches("Quarterly Results").count(), 1);
    assert!(!result.content.contains("**"));
    assert!(!result.content.contains("## "));
    assert!(!result.content.contains("| ---"));
}

#[tokio::test]
async fn should_keep_markdown_slide_title_and_table_control_gh2302() {
    assert_structure(&extract(OutputFormat::Markdown, "North").await, "North");
}

#[tokio::test]
async fn should_preserve_literal_pipe_characters_in_plain_table_cells_gh2302() {
    assert_structure(&extract(OutputFormat::Plain, "North | East").await, "North | East");
}
