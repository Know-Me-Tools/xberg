use super::*;

fn record(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = kind.to_le_bytes().to_vec();
    bytes.extend_from_slice(&((payload.len() + 8) as u32).to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn emf(records: Vec<Vec<u8>>) -> Vec<u8> {
    let count = records.len() as u32 + 2;
    let mut data = vec![0u8; 88];
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    data[4..8].copy_from_slice(&88u32.to_le_bytes());
    data[40..44].copy_from_slice(b" EMF");
    data[44..48].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    data[52..56].copy_from_slice(&count.to_le_bytes());
    data[56..58].copy_from_slice(&16u16.to_le_bytes());
    for record in records {
        data.extend(record);
    }
    data.extend(record(14, &[0u8; 12]));
    let size = data.len() as u32;
    data[48..52].copy_from_slice(&size.to_le_bytes());
    data
}

fn text_record(text: &str) -> Vec<u8> {
    let wide: Vec<_> = text.encode_utf16().collect();
    let mut payload = vec![0u8; 68];
    payload[36..40].copy_from_slice(&(wide.len() as u32).to_le_bytes());
    payload[40..44].copy_from_slice(&76u32.to_le_bytes());
    for value in wide {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    while !payload.len().is_multiple_of(4) {
        payload.push(0);
    }
    record(84, &payload)
}

fn wmf_record(function: u16, payload: &[u8]) -> Vec<u8> {
    assert_eq!(payload.len() % 2, 0);
    let mut bytes = ((payload.len() + 6) as u32 / 2).to_le_bytes().to_vec();
    bytes.extend_from_slice(&function.to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn wmf(records: Vec<Vec<u8>>, placeable: bool) -> Vec<u8> {
    let mut data = vec![0u8; 18];
    data[..2].copy_from_slice(&1u16.to_le_bytes());
    data[2..4].copy_from_slice(&9u16.to_le_bytes());
    data[4..6].copy_from_slice(&0x0300u16.to_le_bytes());
    data[10..12].copy_from_slice(&16u16.to_le_bytes());
    for record in records {
        data.extend(record);
    }
    data.extend(wmf_record(0, &[]));
    let size = data.len() as u32 / 2;
    data[6..10].copy_from_slice(&size.to_le_bytes());
    if !placeable {
        return data;
    }
    let mut header = vec![0u8; 22];
    header[..4].copy_from_slice(&[0xd7, 0xcd, 0xc6, 0x9a]);
    header[10..12].copy_from_slice(&100u16.to_le_bytes());
    header[12..14].copy_from_slice(&100u16.to_le_bytes());
    header[14..16].copy_from_slice(&1440u16.to_le_bytes());
    let checksum = header[..20]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0u16, |value, word| value ^ u16::from_le_bytes([word[0], word[1]]));
    header[20..22].copy_from_slice(&checksum.to_le_bytes());
    header.extend(data);
    header
}

fn wmf_text(text: &[u8]) -> Vec<u8> {
    let mut payload = (text.len() as u16).to_le_bytes().to_vec();
    payload.extend_from_slice(text);
    if !text.len().is_multiple_of(2) {
        payload.push(0);
    }
    payload.extend_from_slice(&[0u8; 4]);
    wmf_record(0x0521, &payload)
}

fn parse(data: &[u8], limits: &SecurityLimits) -> Result<MetafileText> {
    let kind = kind(data, "unknown").expect("native metafile signature");
    extract(data, kind, limits, &mut SecurityBudget::from_limits(limits))
}

#[test]
fn should_preserve_repeated_unicode_emf_records_gh2297() {
    let text = "Revenue Δ 42 🧪";
    let data = emf(vec![text_record(text), text_record(text)]);
    let result = parse(&data, &SecurityLimits::default()).unwrap();
    assert_eq!(result.text, format!("{text}\n{text}"));
    assert!(!result.unsupported_text);
}

#[test]
fn should_read_small_emf_low_unicode_characters_gh2297() {
    let mut data = vec![0u8; 36];
    data[..4].copy_from_slice(&108u32.to_le_bytes());
    data[16..20].copy_from_slice(&4u32.to_le_bytes());
    data[20..24].copy_from_slice(&0x0300u32.to_le_bytes());
    data.extend_from_slice(b"caf\xe9");
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    assert_eq!(
        parse(&emf(vec![data]), &SecurityLimits::default()).unwrap().text,
        "café"
    );
}

#[test]
fn should_read_every_emf_poly_text_descriptor_gh2297() {
    let mut data = vec![0u8; 120];
    data[..4].copy_from_slice(&97u32.to_le_bytes());
    data[36..40].copy_from_slice(&2u32.to_le_bytes());
    for (descriptor, string_offset) in [(40, 120u32), (80, 122)] {
        data[descriptor + 8..descriptor + 12].copy_from_slice(&1u32.to_le_bytes());
        data[descriptor + 12..descriptor + 16].copy_from_slice(&string_offset.to_le_bytes());
    }
    data.extend_from_slice(&[b'A', 0, b'B', 0]);
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    assert_eq!(
        parse(&emf(vec![data]), &SecurityLimits::default()).unwrap().text,
        "A\nB"
    );
}

#[test]
fn should_read_standard_and_placeable_wmf_without_losing_duplicate_labels_gh2297() {
    for placeable in [false, true] {
        let data = wmf(vec![wmf_text(b"Chart"), wmf_text(b"Chart")], placeable);
        assert_eq!(parse(&data, &SecurityLimits::default()).unwrap().text, "Chart\nChart");
    }
}

#[test]
fn should_read_wmf_extended_text_after_its_optional_clipping_rectangle_gh2297() {
    for options in [0u16, 2, 4, 6] {
        let mut payload = vec![0u8; 8];
        payload[4..6].copy_from_slice(&3u16.to_le_bytes());
        payload[6..8].copy_from_slice(&options.to_le_bytes());
        if options != 0 {
            payload.extend_from_slice(&[0u8; 8]);
        }
        payload.extend_from_slice(b"ABC\0");
        let data = wmf(vec![wmf_record(0x0a32, &payload)], false);
        assert_eq!(parse(&data, &SecurityLimits::default()).unwrap().text, "ABC");
    }
}

#[test]
fn should_report_glyph_text_without_inventing_unicode_and_keep_other_records_gh2297() {
    let mut glyph = text_record("wrong");
    glyph[52..56].copy_from_slice(&0x10u32.to_le_bytes());
    let result = parse(&emf(vec![glyph, text_record("valid")]), &SecurityLimits::default()).unwrap();
    assert_eq!(result.text, "valid");
    assert!(result.unsupported_text);
}

#[test]
fn should_refuse_ambiguous_ansi_and_symbol_font_text_gh2297() {
    let high_byte = parse(&wmf(vec![wmf_text(b"caf\xe9")], false), &SecurityLimits::default()).unwrap();
    assert_eq!(high_byte.text, "");
    assert!(high_byte.unsupported_text);
    let mut font = vec![0u8; 18];
    font[13] = 2;
    let data = wmf(
        vec![
            wmf_record(0x02fb, &font),
            wmf_record(0x012d, &0u16.to_le_bytes()),
            wmf_text(b"alpha"),
        ],
        false,
    );
    let symbol = parse(&data, &SecurityLimits::default()).unwrap();
    assert_eq!(symbol.text, "");
    assert!(symbol.unsupported_text);
}

#[test]
fn should_reject_broken_offsets_utf16_and_record_lengths_before_text_growth_gh2297() {
    for mutation in [0, 1, 2] {
        let mut text = text_record("A");
        match mutation {
            0 => text[48..52].copy_from_slice(&u32::MAX.to_le_bytes()),
            1 => text[76..78].copy_from_slice(&0xd800u16.to_le_bytes()),
            _ => text[4..8].copy_from_slice(&u32::MAX.to_le_bytes()),
        }
        assert!(
            parse(&emf(vec![text]), &SecurityLimits::default()).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn should_enforce_decoded_text_entity_iteration_and_input_limits_gh2297() {
    let data = emf(vec![text_record("AB"), text_record("CD")]);
    let exact = SecurityLimits {
        max_content_size: 5,
        ..Default::default()
    };
    assert_eq!(parse(&data, &exact).unwrap().text, "AB\nCD");
    let short = SecurityLimits {
        max_content_size: 4,
        ..Default::default()
    };
    assert!(
        parse(&data, &short)
            .unwrap_err()
            .to_string()
            .contains("Content too large")
    );
    for low in [
        SecurityLimits {
            max_entity_length: 1,
            ..Default::default()
        },
        SecurityLimits {
            max_iterations: 1,
            ..Default::default()
        },
        SecurityLimits {
            max_archive_size: data.len() - 1,
            ..Default::default()
        },
    ] {
        assert!(parse(&data, &low).is_err());
    }
}

fn wmf_font(charset: u8) -> Vec<u8> {
    let mut font = vec![0u8; 50];
    font[13] = charset;
    font[18..24].copy_from_slice(b"Arial\0");
    wmf_record(0x02fb, &font)
}

fn emf_font(handle: u32, charset: u8) -> Vec<u8> {
    let mut font = vec![0u8; 324];
    font[..4].copy_from_slice(&handle.to_le_bytes());
    font[27] = charset;
    record(82, &font)
}

fn emf_ansi(text: &str) -> Vec<u8> {
    let mut text = text_record(text);
    text[..4].copy_from_slice(&83u32.to_le_bytes());
    let utf16: Vec<u8> = text[76..].iter().step_by(2).copied().collect();
    text[76..76 + utf16.len()].copy_from_slice(&utf16);
    text
}

#[test]
fn should_preserve_wmf_text_when_symbol_font_is_never_selected_gh2297() {
    let data = wmf(vec![wmf_font(2), wmf_text(b"Default label")], false);
    let result = parse(&data, &SecurityLimits::default()).unwrap();
    assert_eq!(result.text, "Default label");
    assert!(!result.unsupported_text);
}

#[test]
fn should_scope_wmf_symbol_rejection_to_selected_font_gh2297() {
    let data = wmf(
        vec![
            wmf_font(2),
            wmf_font(0),
            wmf_record(0x012d, &0u16.to_le_bytes()),
            wmf_text(b"not Latin"),
            wmf_record(0x012d, &1u16.to_le_bytes()),
            wmf_text(b"Arial label"),
        ],
        false,
    );
    let result = parse(&data, &SecurityLimits::default()).unwrap();
    assert_eq!(result.text, "Arial label");
    assert!(result.unsupported_text);
}

#[test]
fn should_reuse_wmf_handles_after_deletion_and_ignore_brush_selection_gh2297() {
    let data = wmf(
        vec![
            wmf_font(2),
            wmf_record(0x01f0, &0u16.to_le_bytes()),
            wmf_record(0x02fc, &[0u8; 8]),
            wmf_font(0),
            wmf_record(0x012d, &1u16.to_le_bytes()),
            wmf_record(0x012d, &0u16.to_le_bytes()),
            wmf_text(b"Retained font"),
        ],
        false,
    );
    let result = parse(&data, &SecurityLimits::default()).unwrap();
    assert_eq!(result.text, "Retained font");
    assert!(!result.unsupported_text);
}

#[test]
fn should_restore_wmf_font_and_accept_explicit_ansi_stock_font_gh2297() {
    let data = wmf(
        vec![
            wmf_font(2),
            wmf_record(0x001e, &[]),
            wmf_record(0x012d, &0u16.to_le_bytes()),
            wmf_text(b"not Latin"),
            wmf_record(0x0127, &(-1i16).to_le_bytes()),
            wmf_text(b"Restored"),
            wmf_record(0x012d, &0u16.to_le_bytes()),
            wmf_record(0x012d, &0x800bu16.to_le_bytes()),
            wmf_text(b"Stock ANSI"),
        ],
        false,
    );
    let result = parse(&data, &SecurityLimits::default()).unwrap();
    assert_eq!(result.text, "Restored\nStock ANSI");
    assert!(result.unsupported_text);
}

#[test]
fn should_scope_emf_ansi_text_to_selected_font_and_preserve_unicode_gh2297() {
    let data = emf(vec![
        emf_font(1, 2),
        emf_font(2, 0),
        emf_ansi("Unused Symbol"),
        record(37, &1u32.to_le_bytes()),
        emf_ansi("not Latin"),
        text_record("Unicode Δ"),
        record(37, &2u32.to_le_bytes()),
        emf_ansi("Arial label"),
    ]);
    let result = parse(&data, &SecurityLimits::default()).unwrap();
    assert_eq!(result.text, "Unused Symbol\nUnicode Δ\nArial label");
    assert!(result.unsupported_text);
}

#[test]
fn should_restore_emf_fonts_reuse_handles_and_select_ansi_stock_font_gh2297() {
    let data = emf(vec![
        emf_font(1, 2),
        record(33, &[]),
        record(37, &1u32.to_le_bytes()),
        emf_ansi("not Latin"),
        record(34, &(-1i32).to_le_bytes()),
        emf_ansi("Restored"),
        record(40, &1u32.to_le_bytes()),
        emf_font(1, 0),
        record(37, &1u32.to_le_bytes()),
        emf_ansi("Reused Arial"),
        record(37, &0x8000000bu32.to_le_bytes()),
        emf_ansi("Stock ANSI"),
    ]);
    let result = parse(&data, &SecurityLimits::default()).unwrap();
    assert_eq!(result.text, "Restored\nReused Arial\nStock ANSI");
    assert!(result.unsupported_text);
}

#[test]
fn should_report_unknown_selected_font_charset_without_guessing_gh2297() {
    for data in [
        wmf(
            vec![
                wmf_font(1),
                wmf_record(0x012d, &0u16.to_le_bytes()),
                wmf_text(b"Ambiguous"),
            ],
            false,
        ),
        emf(vec![
            emf_font(1, 1),
            record(37, &1u32.to_le_bytes()),
            emf_ansi("Ambiguous"),
        ]),
    ] {
        let result = parse(&data, &SecurityLimits::default()).unwrap();
        assert_eq!(result.text, "");
        assert!(result.unsupported_text);
    }
}

#[test]
fn should_reject_font_handles_outside_declared_tables_and_missing_saved_contexts_gh2297() {
    for data in [
        wmf(vec![wmf_record(0x012d, &16u16.to_le_bytes())], false),
        emf(vec![emf_font(16, 0)]),
        wmf(vec![wmf_record(0x0127, &(-1i16).to_le_bytes())], false),
        emf(vec![record(34, &(-1i32).to_le_bytes())]),
    ] {
        assert!(parse(&data, &SecurityLimits::default()).is_err());
    }
}

#[test]
fn should_reuse_emf_color_space_handles_for_fonts_after_either_deletion_record_gh2297() {
    for (creation, size) in [(99, 328u32), (122, 588)] {
        for deletion in [40, 101] {
            let mut colorspace = vec![0u8; 4 + size as usize + usize::from(creation == 122) * 8];
            colorspace[..4].copy_from_slice(&1u32.to_le_bytes());
            colorspace[4..8].copy_from_slice(&0x50534f43u32.to_le_bytes());
            colorspace[8..12].copy_from_slice(&0x400u32.to_le_bytes());
            colorspace[12..16].copy_from_slice(&size.to_le_bytes());
            colorspace[16..20].copy_from_slice(&0x73524742u32.to_le_bytes());
            colorspace[20..24].copy_from_slice(&4u32.to_le_bytes());
            let data = emf(vec![
                record(creation, &colorspace),
                record(deletion, &1u32.to_le_bytes()),
                emf_font(1, 0),
                record(37, &1u32.to_le_bytes()),
                emf_ansi("Reused color space handle"),
            ]);
            let result = parse(&data, &SecurityLimits::default()).unwrap();
            assert_eq!(result.text, "Reused color space handle");
            assert!(!result.unsupported_text);
        }
    }
}
