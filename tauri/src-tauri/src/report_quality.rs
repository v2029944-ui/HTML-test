//! Stream only the selected report and, on request, its shared source diagnostics.
use super::{err, read_model, Storage};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::de::{DeserializeSeed, Error, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::{fmt, io::BufReader, marker::PhantomData};

const MAX_PAGE: usize = 50;

#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityScope {
    Report,
    Source,
}

#[derive(Debug, Serialize)]
pub struct ReportQualityPage {
    issues: Vec<QualityIssue>,
    next_offset: Option<u32>,
    source_anchor: Option<String>,
    warning_fallback: bool,
}

#[derive(Debug, Default, Serialize)]
struct QualityIssue {
    kind: Option<String>,
    reason: String,
    sheet: Option<String>,
    organization: Option<String>,
    date: Option<String>,
    source_row: Option<u32>,
    work_cell: Option<String>,
    count_cell: Option<String>,
    amount_cell: Option<String>,
    cells: Vec<String>,
    relevant: Option<bool>,
}

#[derive(Deserialize)]
struct SavedIssue {
    reason: String,
    kind: Option<String>,
    #[serde(rename = "type")]
    legacy_kind: Option<String>,
    sheet: Option<String>,
    organization: Option<String>,
    date: Option<String>,
    #[serde(alias = "sourceRow")]
    source_row: Option<u32>,
    row: Option<u32>,
    #[serde(alias = "workCell")]
    work_cell: Option<String>,
    #[serde(alias = "countCell")]
    count_cell: Option<String>,
    #[serde(alias = "amountCell")]
    amount_cell: Option<String>,
    #[serde(default, deserialize_with = "saved_cells")]
    cells: Vec<String>,
    relevant: Option<bool>,
    source_issue_sequence: Option<u64>,
}

fn clip(mut text: String, max_bytes: usize) -> String {
    if text.len() > max_bytes {
        let mut end = max_bytes - '…'.len_utf8();
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    text
}

fn label(text: Option<String>) -> Option<String> {
    text.map(|text| clip(text, 256))
}

fn saved_cells<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    struct Cells;
    impl<'de> Visitor<'de> for Cells {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a list of Excel cell references")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut cells = Vec::new();
            for _ in 0..8 {
                match seq.next_element::<String>()? {
                    Some(cell) => cells.push(clip(cell, 128)),
                    None => return Ok(cells),
                }
            }
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(cells)
        }
    }
    d.deserialize_seq(Cells)
}

trait PageItem {
    fn mirrored(&self) -> bool {
        false
    }
    fn into_issue(self) -> QualityIssue;
}

impl PageItem for SavedIssue {
    fn mirrored(&self) -> bool {
        self.source_issue_sequence.is_some()
    }

    fn into_issue(self) -> QualityIssue {
        QualityIssue {
            kind: label(self.kind.or(self.legacy_kind)),
            reason: clip(self.reason, 4096),
            sheet: label(self.sheet),
            organization: label(self.organization),
            date: label(self.date),
            source_row: self.source_row.or(self.row),
            work_cell: label(self.work_cell),
            count_cell: label(self.count_cell),
            amount_cell: label(self.amount_cell),
            cells: self.cells,
            relevant: self.relevant,
        }
    }
}

impl PageItem for String {
    fn into_issue(self) -> QualityIssue {
        QualityIssue { kind: Some("warning".into()), reason: clip(self, 4096), ..Default::default() }
    }
}

#[derive(Default)]
struct Window {
    any: bool,
    more: bool,
    issues: Vec<QualityIssue>,
}

#[derive(Clone, Copy)]
struct Page {
    offset: u32,
    limit: usize,
    omit_mirrors: bool,
}

struct WindowRead<T>(Page, PhantomData<T>);

impl<'de, T: Deserialize<'de> + PageItem> DeserializeSeed<'de> for WindowRead<T> {
    type Value = Window;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Window, D::Error> {
        d.deserialize_seq(self)
    }
}

impl<'de, T: Deserialize<'de> + PageItem> Visitor<'de> for WindowRead<T> {
    type Value = Window;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an array of saved diagnostics")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Window, A::Error> {
        let page = self.0;
        let mut result = Window::default();
        let mut skip = page.offset;
        while let Some(item) = seq.next_element::<T>()? {
            result.any = true;
            if page.omit_mirrors && item.mirrored() {
                continue;
            }
            if skip > 0 {
                skip -= 1;
                continue;
            }
            if result.issues.len() == page.limit {
                result.more = true;
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                break;
            }
            result.issues.push(item.into_issue());
        }
        Ok(result)
    }
}

#[derive(Default)]
struct Document {
    issues: Option<Window>,
    warnings: Option<Window>,
    source_issues: Option<Window>,
    sheet: Option<String>,
    anchor: Option<String>,
}

struct DocumentRead {
    scope: QualityScope,
    page: Page,
}

impl<'de> DeserializeSeed<'de> for DocumentRead {
    type Value = Document;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Document, D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for DocumentRead {
    type Value = Document;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a saved report object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Document, A::Error> {
        let mut doc = Document::default();
        let mut legacy_sheet = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "sourceSheet" => doc.sheet = label(map.next_value()?),
                "sheetName" => legacy_sheet = label(map.next_value()?),
                "sourceDiagnosticsReport" => doc.anchor = map.next_value()?,
                "issues" if self.scope == QualityScope::Report => {
                    if doc.issues.is_some() { return Err(A::Error::duplicate_field("issues")); }
                    doc.issues = Some(map.next_value_seed(WindowRead(self.page, PhantomData::<SavedIssue>))?);
                }
                "warnings" if self.scope == QualityScope::Report => {
                    if doc.warnings.is_some() { return Err(A::Error::duplicate_field("warnings")); }
                    doc.warnings = Some(map.next_value_seed(WindowRead(self.page, PhantomData::<String>))?);
                }
                "sourceIssues" => {
                    if doc.source_issues.is_some() { return Err(A::Error::duplicate_field("sourceIssues")); }
                    doc.source_issues = Some(if self.scope == QualityScope::Source {
                        map.next_value_seed(WindowRead(self.page, PhantomData::<SavedIssue>))?
                    } else {
                        map.next_value::<IgnoredAny>()?;
                        Window::default()
                    });
                }
                _ => { map.next_value::<IgnoredAny>()?; }
            }
        }
        doc.sheet = doc.sheet.or(legacy_sheet);
        Ok(doc)
    }
}

fn read_document(conn: &Connection, id: &str, scope: QualityScope, page: Page) -> Result<Document, String> {
    let rowid = conn.query_row("SELECT rowid FROM reports WHERE id=?1", [id], |r| r.get(0))
        .optional().map_err(err)?.ok_or("Сохранённый отчёт не найден.")?;
    let blob = conn.blob_open("main", "reports", "json", rowid, true).map_err(err)?;
    let bounded = read_model::TokenBounded::new(blob).limit_depth();
    let mut deserializer = serde_json::Deserializer::from_reader(BufReader::with_capacity(8192, bounded));
    let doc = DocumentRead { scope, page }.deserialize(&mut deserializer).map_err(err)?;
    deserializer.end().map_err(err)?;
    Ok(doc)
}

impl Storage {
    pub fn report_quality_page(
        &self, expected_revision: i64, id: &str, scope: QualityScope, offset: u32, limit: usize,
    ) -> Result<ReportQualityPage, String> {
        if id.is_empty() || id.len() > 65_536 || !(1..=MAX_PAGE).contains(&limit) {
            return Err("Недопустимый идентификатор отчёта или размер страницы.".into());
        }
        let conn = Connection::open_with_flags(self.db_path(), OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;
        let tx = conn.unchecked_transaction().map_err(err)?;
        read_model::check_revision(&tx, expected_revision)?;
        // The normalized link follows anchor replacements. The serialized
        // pointer is only a fallback for reports without normalized links.
        let live_anchor: Option<(String, bool)> = tx.query_row(
            "SELECT CASE WHEN length(CAST(l.anchor_id AS BLOB)) BETWEEN 1 AND 65536
                         THEN l.anchor_id ELSE '' END,
                    EXISTS(SELECT 1 FROM reports AS anchor WHERE anchor.id=l.anchor_id)
             FROM report_source_diagnostic_links AS l WHERE l.report_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional().map_err(err)?;
        if live_anchor.as_ref().is_some_and(|(id, _)| id.is_empty()) {
            return Err("Недопустимая ссылка на общие замечания источника.".into());
        }
        let mut page = Page { offset, limit, omit_mirrors: false };
        let (doc, source_anchor) = match (scope, live_anchor) {
            (QualityScope::Source, Some((anchor, _))) => {
                let doc = read_document(&tx, &anchor, scope, page)?;
                (doc, Some(anchor))
            }
            (QualityScope::Report, live) => {
                // If the stored anchor is missing, preserve any report copies.
                page.omit_mirrors = live.as_ref().is_some_and(|(_, exists)| *exists);
                let doc = read_document(&tx, id, scope, page)?;
                let anchor = live.map(|(anchor, _)| anchor).or_else(|| document_anchor(&doc, id));
                (doc, anchor)
            }
            (QualityScope::Source, None) => {
                let doc = read_document(&tx, id, scope, page)?;
                let anchor = document_anchor(&doc, id).ok_or("Общие замечания источника не сохранены.")?;
                let doc = if anchor == id { doc } else { read_document(&tx, &anchor, scope, page)? };
                (doc, Some(anchor))
            }
        };
        let warning_fallback = scope == QualityScope::Report && !doc.issues.as_ref().is_some_and(|w| w.any)
            && doc.warnings.as_ref().is_some_and(|w| w.any);
        let mut window = match scope {
            QualityScope::Report if warning_fallback => doc.warnings.unwrap_or_default(),
            QualityScope::Report => doc.issues.unwrap_or_default(),
            QualityScope::Source => doc.source_issues.ok_or("Общие замечания источника не найдены в сохранённом отчёте.")?,
        };
        for issue in &mut window.issues {
            if issue.sheet.is_none() {
                issue.sheet.clone_from(&doc.sheet);
            }
        }
        tx.commit().map_err(err)?;
        Ok(ReportQualityPage {
            issues: window.issues,
            next_offset: window.more.then(|| offset + limit as u32),
            source_anchor,
            warning_fallback,
        })
    }
}

fn document_anchor(doc: &Document, id: &str) -> Option<String> {
    doc.anchor.clone().filter(|anchor| !anchor.is_empty())
        .or_else(|| doc.source_issues.as_ref().map(|_| id.to_owned()))
}

#[cfg(test)]
#[path = "report_quality_tests.rs"]
mod tests;
