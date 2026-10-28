use super::*;

const GLYPH_INDEX_FLAG: u32 = 0x10;
const NO_RECT_FLAG: u32 = 0x100;
const SMALL_CHARS_FLAG: u32 = 0x200;

pub(super) fn extract(data: &[u8], sink: &mut TextSink<'_>) -> Result<()> {
    if u32_at(data, 0)? != 1 || u32_at(data, 4)? < 88 || span(data, 40, 4)? != b" EMF" {
        return Err(invalid("invalid EMF header"));
    }
    let data = span(data, 0, u32_at(data, 48)? as usize)?;
    let mut fonts = wmf::Fonts::new(u16_at(data, 56)? as usize);
    let mut position = 0;
    let mut records = 0;
    while position < data.len() {
        sink.budget.step()?;
        let record_type = u32_at(data, position)?;
        let size = u32_at(data, position + 4)? as usize;
        if size < 8 || !size.is_multiple_of(4) {
            return Err(invalid("invalid EMF record size"));
        }
        let record = span(data, position, size)?;
        position += size;
        records += 1;
        if record_type == 14 {
            if size < 20 || position != data.len() || records != u32_at(data, 52)? as usize {
                return Err(invalid("invalid EMF end record or record count"));
            }
            return Ok(());
        }
        fonts.emf_record(record, record_type)?;
        if matches!(record_type, 83 | 96) && !fonts.ascii {
            sink.output.unsupported_text = true;
        } else {
            record_text(record, record_type, sink)?;
        }
    }
    Err(invalid("missing EMF end record"))
}

fn record_text(record: &[u8], record_type: u32, sink: &mut TextSink<'_>) -> Result<()> {
    // ~keep MS-EMF 2.3.5: EXTTEXTOUTA/W, POLYTEXTOUTA/W and SMALLTEXTOUT carry native strings.
    match record_type {
        83 | 84 => text(
            record,
            36,
            36 + text_descriptor_size(record, 36)?,
            record_type == 84,
            sink,
        ),
        96 | 97 => poly_text(record, record_type == 97, sink),
        108 => small_text(record, sink),
        _ => Ok(()),
    }
}

fn text_descriptor_size(record: &[u8], offset: usize) -> Result<usize> {
    let options = u32_at(record, offset + 16)?;
    let size = if options & NO_RECT_FLAG != 0 { 24 } else { 40 };
    span(record, offset, size)?;
    Ok(size)
}

fn text(record: &[u8], offset: usize, string_floor: usize, wide: bool, sink: &mut TextSink<'_>) -> Result<()> {
    if u32_at(record, offset + 16)? & GLYPH_INDEX_FLAG != 0 {
        sink.output.unsupported_text = true;
        return Ok(());
    }
    let count = u32_at(record, offset + 8)? as usize;
    if count == 0 {
        return Ok(());
    }
    let string_offset = u32_at(record, offset + 12)? as usize;
    if string_offset < string_floor {
        return Err(invalid("EMF string overlaps its record header"));
    }
    let bytes = count
        .checked_mul(if wide { 2 } else { 1 })
        .ok_or_else(|| invalid("EMF string size overflow"))?;
    sink.append(
        span(record, string_offset, bytes)?,
        if wide { Encoding::Utf16 } else { Encoding::Ascii },
    )
}

fn poly_text(record: &[u8], wide: bool, sink: &mut TextSink<'_>) -> Result<()> {
    let count = u32_at(record, 36)? as usize;
    let mut end = 40;
    for _ in 0..count {
        sink.budget.step()?;
        end += text_descriptor_size(record, end)?;
    }
    let mut offset = 40;
    for _ in 0..count {
        sink.budget.step()?;
        text(record, offset, end, wide, sink)?;
        offset += text_descriptor_size(record, offset)?;
    }
    Ok(())
}

fn small_text(record: &[u8], sink: &mut TextSink<'_>) -> Result<()> {
    let count = u32_at(record, 16)? as usize;
    let options = u32_at(record, 20)?;
    if options & GLYPH_INDEX_FLAG != 0 {
        sink.output.unsupported_text = true;
        return Ok(());
    }
    let narrow = options & SMALL_CHARS_FLAG != 0;
    let start = if options & NO_RECT_FLAG != 0 { 36 } else { 52 };
    let length = count
        .checked_mul(if narrow { 1 } else { 2 })
        .ok_or_else(|| invalid("EMF string size overflow"))?;
    sink.append(
        span(record, start, length)?,
        if narrow { Encoding::LowUnicode } else { Encoding::Utf16 },
    )
}
