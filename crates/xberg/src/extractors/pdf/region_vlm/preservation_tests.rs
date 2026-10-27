use super::{RegionVlmResult, inject_region_results};
use crate::pdf::structure::types::{LayoutHint, LayoutHintClass};
use crate::types::BoundingBox;
use crate::types::internal::{ElementKind, InternalDocument, InternalElement};

const NUMERIC_CELLS: [[&str; 4]; 3] = [
    ["10", "20", "30", "40"],
    ["11", "21", "31", "41"],
    ["12", "22", "32", "42"],
];

fn native_box() -> BoundingBox {
    BoundingBox {
        x0: 60.0,
        y0: 340.0,
        x1: 540.0,
        y1: 580.0,
    }
}

fn apply_answer(document: &mut InternalDocument, class_name: LayoutHintClass, answer: &str) {
    inject_region_results(
        document,
        vec![RegionVlmResult {
            page_index: 0,
            markdown: answer.to_string(),
            hint: LayoutHint {
                class_name,
                confidence: 0.9,
                left: 50.0,
                bottom: 300.0,
                right: 550.0,
                top: 600.0,
            },
        }],
    );
}

fn table_document(cells: [[&str; 4]; 3]) -> InternalDocument {
    let mut document = InternalDocument::default();
    let table_index = document.push_table(crate::types::Table {
        cells: cells.map(|row| row.map(str::to_string).to_vec()).to_vec(),
        markdown: "native table".to_string(),
        page_number: 1,
        bounding_box: Some(native_box()),
        ..Default::default()
    });
    let mut element = InternalElement::text(ElementKind::Table { table_index }, "", 0).with_page(1);
    element.bbox = Some(native_box());
    document.elements.push(element);
    document
}

fn paragraph_document(text: &str) -> InternalDocument {
    let mut document = InternalDocument::default();
    let mut element = InternalElement::text(ElementKind::Paragraph, text, 0).with_page(1);
    element.bbox = Some(native_box());
    document.elements.push(element);
    document
}

fn texts(document: &InternalDocument) -> Vec<&str> {
    document
        .elements
        .iter()
        .map(|element| match element.kind {
            ElementKind::Table { .. } => "native table",
            _ => element.text.as_str(),
        })
        .collect()
}

#[test]
fn should_keep_a_numeric_table_when_the_answer_has_unrelated_tokens_gh2269() {
    let mut document = table_document(NUMERIC_CELLS);
    assert_eq!(
        document.tables[0]
            .cells
            .iter()
            .flatten()
            .flat_map(|cell| super::super::normalized_pdf_tokens(cell))
            .count(),
        12
    );
    apply_answer(&mut document, LayoutHintClass::Table, "Unrelated text");
    assert_eq!(texts(&document), ["Unrelated text", "native table"]);
    assert_eq!(
        document.tables[0].cells,
        NUMERIC_CELLS.map(|row| row.map(str::to_string).to_vec()).to_vec()
    );
}

#[test]
fn should_keep_a_symbol_table_when_the_answer_has_no_verifiable_tokens_gh2269() {
    let cells = [["*", "---", "+", "="], ["!", "?", "#", "%"], ["/", "\\", "<", ">"]];
    let mut document = table_document(cells);
    assert_eq!(
        document.tables[0]
            .cells
            .iter()
            .flatten()
            .flat_map(|cell| super::super::normalized_pdf_tokens(cell))
            .count(),
        0
    );
    apply_answer(&mut document, LayoutHintClass::Table, "Unrelated text");
    assert_eq!(texts(&document), ["Unrelated text", "native table"]);
    assert_eq!(
        document.tables[0].cells,
        cells.map(|row| row.map(str::to_string).to_vec()).to_vec()
    );
}

#[test]
fn should_keep_symbol_paragraphs_when_the_answer_cannot_be_checked_gh2269() {
    let native = "* * * ---";
    assert_eq!(super::super::normalized_pdf_tokens(native).count(), 0);
    for class_name in [LayoutHintClass::Table, LayoutHintClass::Other] {
        for answer in ["Unrelated text", native] {
            let mut document = paragraph_document(native);
            apply_answer(&mut document, class_name, answer);
            assert_eq!(texts(&document), [answer, native], "{class_name:?}: {answer}");
        }
    }
}

#[test]
fn should_replace_a_numeric_table_when_the_answer_carries_its_values_gh2259() {
    let answer = "|10|20|30|40|\n|11|21|31|41|\n|12|22|32|42|";
    let mut document = table_document(NUMERIC_CELLS);
    apply_answer(&mut document, LayoutHintClass::Table, answer);
    assert_eq!(texts(&document), [answer]);
}

#[test]
fn should_replace_paragraphs_when_the_answer_carries_verifiable_tokens_gh2259() {
    for class_name in [LayoutHintClass::Table, LayoutHintClass::Other] {
        for (native, answer) in [("42 17", "|42|17|"), ("North revenue", "north REVENUE")] {
            let mut document = paragraph_document(native);
            apply_answer(&mut document, class_name, answer);
            assert_eq!(texts(&document), [answer], "{class_name:?}: {native}");
        }
    }
}
