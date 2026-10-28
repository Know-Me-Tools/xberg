//! `xberg::extraction::docx::parser::Paragraph` is public and exhaustive, so a caller can build one
//! with a struct literal that names every field. This file does that from outside the crate: a
//! field added to `Paragraph` breaks the build here before it breaks a caller.

#![cfg(feature = "office")]

use xberg::extraction::docx::parser::Paragraph;

#[test]
fn a_paragraph_can_still_be_built_from_its_fields() {
    let paragraph = Paragraph {
        runs: Vec::new(),
        style: Some("ListParagraph".to_string()),
        numbering_id: Some(1),
        numbering_level: Some(0),
        bookmarks: Vec::new(),
        in_table_of_contents: false,
    };

    assert_eq!((paragraph.numbering_id, paragraph.numbering_level), (Some(1), Some(0)));
}
