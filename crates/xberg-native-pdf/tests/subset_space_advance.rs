use xberg_native_pdf::PdfDocument;

fn subset_pdf(left: &str, right: &str, gap: f32, space_width: Option<u16>) -> Vec<u8> {
    let first = if space_width.is_some() { 32 } else { 33 };
    let mut widths = vec![600u16; 123 - first];
    if let Some(width) = space_width {
        widths[0] = width;
    }
    let widths = widths.iter().map(u16::to_string).collect::<Vec<_>>().join(" ");
    let right_x = 72.0 + left.len() as f32 * 6.0 + gap;
    let content = format!("BT /F1 10 Tf 1 0 0 1 72 700 Tm [({left})] TJ 1 0 0 1 {right_x} 700 Tm [({right})] TJ ET");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        format!("<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+SubsetProportional /Encoding /WinAnsiEncoding /FirstChar {first} /LastChar 122 /Widths [{widths}] >>"),
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0usize];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in &offsets[1..] {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    bytes
}

#[test]
fn should_preserve_word_gaps_when_subset_widths_omit_the_space_glyph_gh2303() {
    for (left, right) in [("ALPHA", "BETA"), ("December", "31"), ("and", "Subsidiaries")] {
        let document = PdfDocument::from_bytes(subset_pdf(left, right, 2.49, None)).unwrap();
        let text = document.extract_text(0).unwrap();
        assert_eq!(text.trim(), format!("{left} {right}"));
    }
}

#[test]
fn should_preserve_kerning_and_explicit_wider_space_metrics_gh2303() {
    for (gap, space_width) in [(0.0, None), (0.1, None), (2.49, Some(700))] {
        let document = PdfDocument::from_bytes(subset_pdf("PART", "IAL", gap, space_width)).unwrap();
        let text = document.extract_text(0).unwrap();
        assert_eq!(text.trim(), "PARTIAL");
    }
}

#[test]
#[ignore = "requires the explicitly provisioned Berkshire Hathaway 2023 annual report"]
fn should_preserve_actual_berkshire_balance_sheet_word_boundaries_gh2303() {
    use sha2::{Digest, Sha256};

    let path = std::env::var_os("XBERG_BERKSHIRE_2023_PDF")
        .expect("set XBERG_BERKSHIRE_2023_PDF to the original annual report before selecting this test");
    let bytes = std::fs::read(path).expect("read explicitly provisioned Berkshire 2023 annual report");
    let actual_sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        actual_sha256, "2132b85f9c472a6f0b141551adb08d93f88db28e6a3794e7e2b13ca3f8a57b5b",
        "the manual regression requires the unchanged original report"
    );
    let document = PdfDocument::from_bytes(bytes).expect("parse the original report");
    assert!(document.page_count().expect("count report pages") >= 94);
    let text = document.extract_text(93).expect("extract physical page 94");
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    for (expected, welded) in [
        ("and Subsidiaries", "andSubsidiaries"),
        ("CONSOLIDATED BALANCE SHEETS", "CONSOLIDATEDBALANCE SHEETS"),
        ("December 31", "December31"),
        ("in U.S. Treasury Bills", "inU.S. Treasury Bills"),
    ] {
        assert!(
            collapsed.contains(expected),
            "missing word boundary {expected:?}: {collapsed}"
        );
        assert!(
            !collapsed.contains(welded),
            "retained welded phrase {welded:?}: {collapsed}"
        );
    }
}
