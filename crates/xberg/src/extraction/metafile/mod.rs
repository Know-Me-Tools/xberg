//! Bounded native text recovery from Windows metafile records, without rasterization.
//! Strings retain record order; glyph indices and ambiguous font code pages remain explicit gaps.

use crate::extractors::security::{SecurityBudget, SecurityLimits};
use crate::{Result, XbergError};

mod emf;
mod wmf;

#[derive(Clone, Copy)]
pub(crate) enum MetafileKind {
    Emf,
    Wmf,
}

#[derive(Debug, Default)]
pub(crate) struct MetafileText {
    pub text: String,
    pub unsupported_text: bool,
}

#[derive(Clone, Copy)]
enum Encoding {
    Ascii,
    Utf16,
    LowUnicode,
}

pub(crate) fn kind(data: &[u8], format: &str) -> Option<MetafileKind> {
    if data.get(..4) == Some(&[1, 0, 0, 0]) && data.get(40..44) == Some(b" EMF") {
        Some(MetafileKind::Emf)
    } else if data.starts_with(&[0xd7, 0xcd, 0xc6, 0x9a]) || matches!(data.get(..6), Some([1 | 2, 0, 9, 0, 0, 1 | 3])) {
        Some(MetafileKind::Wmf)
    } else if format.eq_ignore_ascii_case("emf") {
        Some(MetafileKind::Emf)
    } else if format.eq_ignore_ascii_case("wmf") {
        Some(MetafileKind::Wmf)
    } else {
        None
    }
}

pub(crate) fn extract(
    data: &[u8],
    kind: MetafileKind,
    limits: &SecurityLimits,
    budget: &mut SecurityBudget,
) -> Result<MetafileText> {
    if data.len() > limits.max_archive_size {
        return Err(invalid("metafile exceeds configured input size"));
    }
    let mut sink = TextSink {
        output: MetafileText::default(),
        limits,
        budget,
    };
    match kind {
        MetafileKind::Emf => emf::extract(data, &mut sink)?,
        MetafileKind::Wmf => wmf::extract(data, &mut sink)?,
    }
    Ok(sink.output)
}

fn invalid(reason: impl std::fmt::Display) -> XbergError {
    XbergError::parsing(format!("Invalid metafile text records: {reason}"))
}

fn span(data: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| invalid("record offset overflow"))?;
    data.get(offset..end)
        .ok_or_else(|| invalid("truncated record or string"))
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16> {
    span(data, offset, 2)?
        .try_into()
        .map(u16::from_le_bytes)
        .map_err(invalid)
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    span(data, offset, 4)?
        .try_into()
        .map(u32::from_le_bytes)
        .map_err(invalid)
}

fn decoded_length(data: &[u8], encoding: Encoding) -> Result<usize> {
    match encoding {
        Encoding::Ascii => Ok(data.len()),
        Encoding::LowUnicode => Ok(data.iter().map(|&byte| char::from(byte).len_utf8()).sum()),
        Encoding::Utf16 => char::decode_utf16(
            data.as_chunks::<2>()
                .0
                .iter()
                .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]])),
        )
        .try_fold(0usize, |length, character| {
            length
                .checked_add(character.map_err(invalid)?.len_utf8())
                .ok_or_else(|| invalid("text size overflow"))
        }),
    }
}

struct TextSink<'a> {
    output: MetafileText,
    limits: &'a SecurityLimits,
    budget: &'a mut SecurityBudget,
}

impl TextSink<'_> {
    fn append(&mut self, data: &[u8], encoding: Encoding) -> Result<()> {
        let units = if matches!(encoding, Encoding::Utf16) {
            data.len() / 2
        } else {
            data.len()
        };
        if units > self.limits.max_entity_length {
            return Err(invalid("text record exceeds configured entity length"));
        }
        if matches!(encoding, Encoding::Ascii) && !data.is_ascii() {
            // ~keep ANSI records require the playback font's code page; never invent it from locale or UTF-8.
            self.output.unsupported_text = true;
            return Ok(());
        }
        let length = decoded_length(data, encoding)?;
        if length > self.limits.max_entity_length {
            return Err(invalid("decoded text record exceeds configured entity length"));
        }
        if length == 0 {
            return Ok(());
        }
        let separator = usize::from(!self.output.text.is_empty());
        // ~keep Charge decoded UTF-8 bytes and separators before growing the output string.
        self.budget.account_text(
            length
                .checked_add(separator)
                .ok_or_else(|| invalid("text size overflow"))?,
        )?;
        if separator != 0 {
            self.output.text.push('\n');
        }
        match encoding {
            Encoding::Ascii => self.output.text.push_str(std::str::from_utf8(data).map_err(invalid)?),
            Encoding::LowUnicode => self.output.text.extend(data.iter().map(|&byte| char::from(byte))),
            Encoding::Utf16 => {
                for character in char::decode_utf16(
                    data.as_chunks::<2>()
                        .0
                        .iter()
                        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]])),
                ) {
                    self.output.text.push(character.map_err(invalid)?);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
