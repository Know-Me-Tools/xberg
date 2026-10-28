use super::*;

pub(super) fn extract(data: &[u8], sink: &mut TextSink<'_>) -> Result<()> {
    let header = if data.starts_with(&[0xd7, 0xcd, 0xc6, 0x9a]) {
        22
    } else {
        0
    };
    let data = span(data, header, data.len().saturating_sub(header))?;
    if !matches!(u16_at(data, 0)?, 1 | 2) || u16_at(data, 2)? != 9 || !matches!(u16_at(data, 4)?, 0x100 | 0x300) {
        return Err(invalid("invalid WMF header"));
    }
    let length = (u32_at(data, 6)? as usize)
        .checked_mul(2)
        .ok_or_else(|| invalid("WMF file size overflow"))?;
    let data = span(data, 0, length)?;
    let mut fonts = Fonts::new(u16_at(data, 10)? as usize);
    let mut position = 18;
    while position < data.len() {
        sink.budget.step()?;
        let record = next_record(data, position)?;
        position += record.len();
        let function = u16_at(record, 4)?;
        if function == 0 {
            if position != data.len() {
                return Err(invalid("data after WMF end record"));
            }
            return Ok(());
        }
        fonts.wmf_record(record, function, sink.budget)?;
        match function {
            0x0521 | 0x0a32 if !fonts.ascii => sink.output.unsupported_text = true,
            0x0521 => text(record, sink)?,
            0x0a32 => extended_text(record, sink)?,
            _ => {}
        }
    }
    Err(invalid("missing WMF end record"))
}

fn next_record(data: &[u8], position: usize) -> Result<&[u8]> {
    let size = (u32_at(data, position)? as usize)
        .checked_mul(2)
        .ok_or_else(|| invalid("WMF record size overflow"))?;
    if size < 6 {
        return Err(invalid("invalid WMF record size"));
    }
    span(data, position, size)
}

#[derive(Clone, Copy)]
enum Object {
    Other,
    Font(bool),
}

// ~keep GDI creates objects before selection; only the selected font determines ANSI interpretation.
pub(super) struct Fonts {
    objects: Vec<Option<Object>>,
    pub(super) ascii: bool,
    saved: Vec<bool>,
}

impl Fonts {
    pub(super) fn new(handles: usize) -> Self {
        Self {
            objects: vec![None; handles],
            ascii: true,
            saved: Vec::new(),
        }
    }

    fn create(&mut self, handle: usize, object: Object) -> Result<()> {
        let slot = self
            .objects
            .get_mut(handle)
            .ok_or_else(|| invalid("invalid font object handle"))?;
        if slot.is_some() {
            return Err(invalid("object handle already allocated"));
        }
        *slot = Some(object);
        Ok(())
    }

    fn select(&mut self, handle: u32, stock_bit: u32) -> Result<()> {
        if handle & stock_bit != 0 {
            match handle & !stock_bit {
                11 | 12 => self.ascii = true,
                10 | 13 | 14 | 16 | 17 => self.ascii = false,
                _ => {}
            }
        } else {
            match self.objects.get(handle as usize).and_then(|object| *object) {
                Some(Object::Font(ascii)) => self.ascii = ascii,
                Some(Object::Other) => {}
                None => return Err(invalid("selection of missing metafile object")),
            }
        }
        Ok(())
    }

    fn delete(&mut self, handle: usize) -> Result<()> {
        let slot = self
            .objects
            .get_mut(handle)
            .ok_or_else(|| invalid("invalid deleted object handle"))?;
        if slot.take().is_none() {
            return Err(invalid("deletion of missing metafile object"));
        }
        Ok(())
    }

    fn restore(&mut self, level: i32) -> Result<()> {
        let index = if level < 0 {
            self.saved.len().checked_sub(level.unsigned_abs() as usize)
        } else {
            usize::try_from(level).ok().and_then(|level| level.checked_sub(1))
        }
        .ok_or_else(|| invalid("invalid saved metafile context"))?;
        self.ascii = *self
            .saved
            .get(index)
            .ok_or_else(|| invalid("missing saved metafile context"))?;
        self.saved.truncate(index);
        Ok(())
    }

    fn wmf_record(&mut self, record: &[u8], function: u16, budget: &mut SecurityBudget) -> Result<()> {
        match function {
            0x02fb | 0x02fa | 0x02fc | 0x00f7 | 0x01f9 | 0x06ff | 0x0142 => {
                let object = if function == 0x02fb {
                    span(record, 6, 18)?;
                    // ~keep MS-WMF Font: charset follows five i16 metrics and three one-byte style flags.
                    Object::Font(record[19] == 0)
                } else {
                    Object::Other
                };
                let mut available = None;
                for (index, object) in self.objects.iter().enumerate() {
                    budget.step()?;
                    if object.is_none() {
                        available = Some(index);
                        break;
                    }
                }
                let handle = available.ok_or_else(|| invalid("WMF object table exhausted"))?;
                self.create(handle, object)?;
            }
            0x012d => self.select(u16_at(record, 6)? as u32, 0x8000)?,
            0x01f0 => self.delete(u16_at(record, 6)? as usize)?,
            0x001e => self.saved.push(self.ascii),
            0x0127 => self.restore(u16_at(record, 6)? as i16 as i32)?,
            _ => {}
        }
        Ok(())
    }

    pub(super) fn emf_record(&mut self, record: &[u8], function: u32) -> Result<()> {
        match function {
            82 => {
                // ~keep MS-EMF EXTCreateFont: 12-byte record prefix, then LOGFONT charset at byte 23.
                span(record, 12, 92)?;
                self.create(u32_at(record, 8)? as usize, Object::Font(record[35] == 0))?;
            }
            38 | 39 | 49 | 93 | 94 | 95 | 99 | 122 => {
                self.create(u32_at(record, 8)? as usize, Object::Other)?;
            }
            37 => self.select(u32_at(record, 8)?, 0x8000_0000)?,
            40 | 101 => self.delete(u32_at(record, 8)? as usize)?,
            33 => self.saved.push(self.ascii),
            34 => self.restore(u32_at(record, 8)? as i32)?,
            _ => {}
        }
        Ok(())
    }
}

fn text_length(record: &[u8], offset: usize) -> Result<usize> {
    let count = u16_at(record, offset)? as i16;
    usize::try_from(count).map_err(|_| invalid("negative WMF string length"))
}

fn text(record: &[u8], sink: &mut TextSink<'_>) -> Result<()> {
    let count = text_length(record, 6)?;
    let padded = count + count % 2;
    span(
        record,
        8,
        padded
            .checked_add(4)
            .ok_or_else(|| invalid("WMF string size overflow"))?,
    )?;
    sink.append(span(record, 8, count)?, Encoding::Ascii)
}

fn extended_text(record: &[u8], sink: &mut TextSink<'_>) -> Result<()> {
    let count = text_length(record, 10)?;
    let options = u16_at(record, 12)?;
    if options & 0x10 != 0 {
        sink.output.unsupported_text = true;
        return Ok(());
    }
    let start = if options & 6 != 0 { 22 } else { 14 };
    span(record, start, count + count % 2)?;
    sink.append(span(record, start, count)?, Encoding::Ascii)
}
