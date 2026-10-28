//! DOC character scripts from authoritative CHPX runs and piece properties. ~keep

use super::{DOC_WARNING_SOURCE, normalized_doc_character, papx};
use crate::error::{Result, XbergError};
use crate::extractors::security::SecurityBudget;
use crate::types::{AnnotationKind, ProcessingWarning, TextAnnotation};

const CHPX_PAIR: usize = 12;
const PAGE_BYTES: usize = 512;
const PAGE_MASK: u32 = 0x003f_ffff;
const SPRM_C_ISS: u16 = 0x2a48;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Script {
    #[default]
    Normal,
    Superscript,
    Subscript,
}

pub(super) fn compact_prm_script(prm: u16) -> Option<Script> {
    const COMPLEX_FLAG: u16 = 1;
    const ISPRM_MASK: u16 = 0x7f;
    const ISPRM_C_ISS: u16 = 0x68;
    if prm & COMPLEX_FLAG != 0 || (prm >> 1) & ISPRM_MASK != ISPRM_C_ISS {
        return None;
    }
    match prm >> 8 {
        0 => Some(Script::Normal),
        1 => Some(Script::Superscript),
        2 => Some(Script::Subscript),
        _ => None,
    }
}

#[derive(Clone, Copy)]
pub(super) enum ScriptProperty {
    Unchanged,
    Override(Script),
    Malformed,
}

struct ScriptRun {
    start: u32,
    end: u32,
    script: Script,
}

#[derive(Default)]
pub(super) struct ScriptIndex(Vec<ScriptRun>);

impl ScriptIndex {
    pub(super) fn read(
        word: &[u8],
        table: &[u8],
        pairs: usize,
        budget: &mut SecurityBudget,
        warnings: &mut Vec<ProcessingWarning>,
    ) -> Result<Self> {
        let declared = pairs.checked_sub(2).and_then(|offset| word.get(offset..pairs));
        if !declared.is_some_and(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]) as usize > CHPX_PAIR) {
            return Ok(Self::default());
        }
        let Some((offset, length)) = papx::read_fc_lcb(word, pairs, CHPX_PAIR) else {
            return Ok(Self::default());
        };
        if length == 0 {
            return Ok(Self::default());
        }
        let bte = offset.checked_add(length).and_then(|end| table.get(offset..end));
        if let Some(bte) = bte
            && let Some(index) = Self::from_bte(word, bte, budget)?
        {
            return Ok(index);
        }
        crate::core::diagnostics::push_warning(
            warnings,
            DOC_WARNING_SOURCE,
            "Malformed DOC character properties were ignored; paragraph text was preserved",
        );
        Ok(Self::default())
    }

    fn from_bte(word: &[u8], bte: &[u8], budget: &mut SecurityBudget) -> Result<Option<Self>> {
        if bte.len() < 12 || !(bte.len() - 4).is_multiple_of(8) {
            return Ok(None);
        }
        let count = (bte.len() - 4) / 8;
        let mut index = Self::default();
        for row in 0..count {
            step(budget)?;
            let (start, end) = (dword(bte, row * 4), dword(bte, (row + 1) * 4));
            if start >= end {
                return Ok(None);
            }
            let page = dword(bte, 4 * (count + 1 + row)) & PAGE_MASK;
            let page = (page as usize).checked_mul(PAGE_BYTES);
            let fkp = page.and_then(|offset| offset.checked_add(PAGE_BYTES).and_then(|end| word.get(offset..end)));
            let Some(fkp) = fkp else { return Ok(None) };
            if !index.read_page(fkp, start, end, budget)? {
                return Ok(None);
            }
        }
        Ok(Some(index))
    }

    fn read_page(&mut self, fkp: &[u8], first: u32, last: u32, budget: &mut SecurityBudget) -> Result<bool> {
        let count = usize::from(fkp[PAGE_BYTES - 1]);
        let header = 4 * (count + 1) + count;
        if header > PAGE_BYTES - 1 {
            return Ok(false);
        }
        for row in 0..count {
            step(budget)?;
            let (start, end) = (dword(fkp, row * 4), dword(fkp, (row + 1) * 4));
            if start >= end {
                return Ok(false);
            }
            let offset = usize::from(fkp[4 * (count + 1) + row]) * 2;
            let script = if offset == 0 {
                Script::Normal
            } else {
                if offset < header || offset >= PAGE_BYTES - 1 {
                    return Ok(false);
                }
                let length = usize::from(fkp[offset]);
                let Some(properties) = fkp
                    .get(offset + 1..offset + 1 + length)
                    .filter(|_| offset + 1 + length < PAGE_BYTES)
                else {
                    return Ok(false);
                };
                let Some(script) = script_property(properties, budget)? else {
                    return Ok(false);
                };
                script
            };
            let (start, end) = (start.max(first), end.min(last));
            if start < end && script != Script::Normal {
                step(budget)?;
                self.0.push(ScriptRun { start, end, script });
            }
        }
        Ok(true)
    }

    pub(super) fn at(&self, fc_end: u32) -> Script {
        let next = self.0.partition_point(|run| run.start < fc_end);
        next.checked_sub(1)
            .and_then(|row| self.0.get(row))
            .filter(|run| fc_end <= run.end)
            .map_or(Script::Normal, |run| run.script)
    }
}

fn dword(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated DOC property table"),
    )
}

fn step(budget: &mut SecurityBudget) -> Result<()> {
    budget.step().map_err(|error| XbergError::validation(error.to_string()))
}

fn script_property(properties: &[u8], budget: &mut SecurityBudget) -> Result<Option<Script>> {
    Ok(match piece_script_property(properties, budget)? {
        ScriptProperty::Unchanged => Some(Script::Normal),
        ScriptProperty::Override(script) => Some(script),
        ScriptProperty::Malformed => None,
    })
}

pub(super) fn piece_script_property(mut properties: &[u8], budget: &mut SecurityBudget) -> Result<ScriptProperty> {
    let mut script = None;
    while !properties.is_empty() {
        step(budget)?;
        let Some(opcode) = properties.get(..2) else {
            return Ok(ScriptProperty::Malformed);
        };
        let opcode = u16::from_le_bytes([opcode[0], opcode[1]]);
        properties = &properties[2..];
        let Some(length) = papx::sprm_operand_length(opcode, properties) else {
            return Ok(ScriptProperty::Malformed);
        };
        let Some(operand) = properties.get(..length) else {
            return Ok(ScriptProperty::Malformed);
        };
        if opcode == SPRM_C_ISS {
            script = Some(match operand[0] {
                1 => Script::Superscript,
                2 => Script::Subscript,
                _ => Script::Normal,
            });
        }
        properties = &properties[length..];
    }
    Ok(script.map_or(ScriptProperty::Unchanged, ScriptProperty::Override))
}

pub(super) fn normalize(
    raw: &str,
    scripts: &[Script],
    budget: &mut SecurityBudget,
) -> Result<(String, Vec<TextAnnotation>)> {
    if raw.chars().count() != scripts.len() {
        return Err(XbergError::parsing("DOC character-property alignment is invalid"));
    }
    let mut text = String::new();
    let mut annotations = Vec::new();
    let mut newlines = 0;
    for (character, script) in raw.chars().zip(scripts) {
        step(budget)?;
        let Some(character) = normalized_doc_character(character) else {
            continue;
        };
        newlines = if character == '\n' { newlines + 1 } else { 0 };
        if newlines > 2 {
            continue;
        }
        budget
            .account_text(character.len_utf8())
            .map_err(|error| XbergError::validation(error.to_string()))?;
        let start =
            u32::try_from(text.len()).map_err(|_| XbergError::validation("DOC annotation offset exceeds u32"))?;
        text.push(character);
        let end = u32::try_from(text.len()).map_err(|_| XbergError::validation("DOC annotation offset exceeds u32"))?;
        append_annotation(&mut annotations, *script, start, end, budget)?;
    }
    let leading = (text.len() - text.trim_start().len()) as u32;
    let length = text.trim().len() as u32;
    for annotation in &mut annotations {
        annotation.start = annotation.start.saturating_sub(leading).min(length);
        annotation.end = annotation.end.saturating_sub(leading).min(length);
    }
    annotations.retain(|annotation| annotation.start < annotation.end);
    Ok((text.trim().to_owned(), annotations))
}

fn append_annotation(
    annotations: &mut Vec<TextAnnotation>,
    script: Script,
    start: u32,
    end: u32,
    budget: &mut SecurityBudget,
) -> Result<()> {
    let kind = match script {
        Script::Normal => return Ok(()),
        Script::Superscript => AnnotationKind::Superscript,
        Script::Subscript => AnnotationKind::Subscript,
    };
    if let Some(last) = annotations.last_mut()
        && last.end == start
        && last.kind == kind
    {
        last.end = end;
    } else {
        step(budget)?;
        annotations.push(TextAnnotation { start, end, kind });
    }
    Ok(())
}
