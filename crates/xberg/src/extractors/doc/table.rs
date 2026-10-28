use super::*;

/// The rows of the table being read, until a paragraph outside it ends it.
#[derive(Default)]
pub(super) struct PendingTable {
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    cell: String,
    columns: usize,
    reserved_cells: usize,
    scripts: Vec<crate::rendering::table_scripts::CellScriptAnnotation>,
}

impl PendingTable {
    /// Add one table paragraph. A cell's paragraphs are joined by line breaks.
    pub(super) fn add(
        &mut self,
        paragraph: &DocParagraph,
        mark: DocTableMark,
        budget: &mut SecurityBudget,
    ) -> Result<()> {
        let text = paragraph.content.trim();
        if !text.is_empty() {
            if !self.cell.is_empty() {
                self.cell.push('\n');
            }
            self.add_scripts(paragraph, text);
            self.cell.push_str(text);
        }
        if mark.cell_end {
            self.end_cell(budget)?;
        }
        if mark.row_end {
            self.end_row(budget)?;
        }
        Ok(())
    }

    fn add_scripts(&mut self, paragraph: &DocParagraph, text: &str) {
        let trimmed_start = paragraph.content.len() - paragraph.content.trim_start().len();
        let trimmed_end = trimmed_start + text.len();
        for annotation in &paragraph.annotations {
            if !matches!(
                annotation.kind,
                crate::types::AnnotationKind::Superscript | crate::types::AnnotationKind::Subscript
            ) {
                continue;
            }
            let start = (annotation.start as usize).max(trimmed_start);
            let end = (annotation.end as usize).min(trimmed_end);
            if start >= end || paragraph.content.get(start..end).is_none() {
                continue;
            }
            let Some(start) = self
                .cell
                .len()
                .checked_add(start - trimmed_start)
                .and_then(|value| u32::try_from(value).ok())
            else {
                continue;
            };
            let Some(end) = self
                .cell
                .len()
                .checked_add(end - trimmed_start)
                .and_then(|value| u32::try_from(value).ok())
            else {
                continue;
            };
            let mut annotation = annotation.clone();
            annotation.start = start;
            annotation.end = end;
            self.scripts
                .push(crate::rendering::table_scripts::CellScriptAnnotation {
                    row: self.rows.len(),
                    col: self.row.len(),
                    annotation,
                });
        }
    }

    fn end_cell(&mut self, budget: &mut SecurityBudget) -> Result<()> {
        let grid = self
            .row
            .len()
            .checked_add(1)
            .map(|columns| columns.max(self.columns))
            .and_then(|columns| {
                self.rows
                    .len()
                    .checked_add(1)?
                    .checked_mul(columns)
                    .map(|cells| (columns, cells))
            });
        let (columns, cells) = grid.ok_or_else(|| crate::XbergError::security("DOC table cell count overflow"))?;
        // Markdown pads every row to the widest one, including rows already read. ~keep
        // Reserve that whole rectangle before allocating another cell. ~keep
        budget.add_cells(cells - self.reserved_cells)?;
        self.columns = columns;
        self.reserved_cells = cells;
        self.row.push(std::mem::take(&mut self.cell));
        Ok(())
    }

    fn end_row(&mut self, budget: &mut SecurityBudget) -> Result<()> {
        if !self.cell.is_empty() {
            self.end_cell(budget)?;
        }
        if !self.row.is_empty() {
            self.rows.push(std::mem::take(&mut self.row));
        }
        Ok(())
    }

    /// Emit the table read so far, unless it holds no text, and start over.
    pub(super) fn flush(&mut self, doc: &mut InternalDocument, budget: &mut SecurityBudget) -> Result<()> {
        self.end_row(budget)?;
        let cells = std::mem::take(&mut self.rows);
        let scripts = std::mem::take(&mut self.scripts);
        self.columns = 0;
        self.reserved_cells = 0;
        if cells.iter().flatten().all(|cell| cell.trim().is_empty()) {
            return Ok(());
        }
        let markdown = crate::rendering::common::render_table_markdown(&cells);
        let table_index = doc.push_table(Table {
            cells,
            markdown,
            ..Default::default()
        });
        let mut element = InternalElement::text(ElementKind::Table { table_index }, "", 0);
        if !scripts.is_empty() {
            let encoded =
                serde_json::to_string(&scripts).expect("table script annotations contain only serializable values");
            element.attributes = Some(AHashMap::from_iter([(
                crate::rendering::table_scripts::TABLE_CELL_SCRIPTS_ATTRIBUTE.to_string(),
                encoded,
            )]));
        }
        doc.push_element(element);
        Ok(())
    }
}
