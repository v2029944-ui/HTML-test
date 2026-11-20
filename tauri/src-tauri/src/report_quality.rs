//! Stream only the selected report and, on request, its shared source diagnostics.
use super::{err, read_model, Storage};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Statement};
use serde::de::{DeserializeSeed, Error, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::{fmt, io::BufReader};

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
    invalid_entries: u32,
    evidence: QualityEvidence,
}

#[derive(Debug, Default, PartialEq, Serialize)]
struct EvidenceCoverage {
    start: String,
    end: String,
}

#[derive(Debug, Default, Serialize)]
struct QualityEvidence {
    document_id: String,
    source_format: Option<String>,
    sheet_name: Option<String>,
    coverage: Option<EvidenceCoverage>,
    explicit_zero: Option<bool>,
    invalid_fields: u32,
    missing_day_entries: u32,
    deferred_day_entries: u32,
    unclassified_day_entries: u32,
    omitted_row_entries: u32,
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
    expected: Option<serde_json::Value>,
    actual: Option<serde_json::Value>,
    raw: Option<Vec<serde_json::Value>>,
    relevant: Option<bool>,
    scope: Option<String>,
    skipped: Option<bool>,
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

// Discard unsupported JSON values without allocating their subtrees. This is
// separate from document syntax errors, which still fail the read.
#[derive(Default)]
enum Field {
    #[default]
    Unsupported,
    Null,
    Text(String),
    Number(u64),
    Signed(i64),
    Decimal(f64),
    Comparison(serde_json::Value),
    Raw(Vec<serde_json::Value>),
    Boolean(bool),
    Cells(Vec<String>),
    Coverage(EvidenceCoverage),
}

impl Field {
    fn text(self) -> Option<String> {
        if let Self::Text(text) = self { Some(text) } else { None }
    }

    fn number(self) -> Option<u64> {
        if let Self::Number(number) = self { Some(number) } else { None }
    }

    fn metric(self) -> Option<serde_json::Value> {
        match self {
            Self::Number(n) => Some(n.into()),
            Self::Signed(n) => Some(n.into()),
            Self::Decimal(n) if n.is_finite() => Some(n.into()),
            Self::Null => Some(serde_json::Value::Null),
            _ => None,
        }
    }

    fn nullable_text(self) -> Result<Option<String>, ()> {
        match self {
            Self::Null => Ok(None),
            Self::Text(text) => Ok(Some(text)),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy)]
enum ValueShape { Issue, Warning, Scalar, Cells, Coverage, Comparison, Raw }

#[derive(Default)]
struct Entry {
    issue: Option<QualityIssue>,
    mirrored: bool,
    invalid_fields: u32,
}

impl<'de> DeserializeSeed<'de> for ValueShape {
    type Value = Field;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Field, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ValueShape {
    type Value = Field;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a saved diagnostic value")
    }

    fn visit_str<E: Error>(self, value: &str) -> Result<Field, E> {
        Ok(Field::Text(value.to_owned()))
    }

    fn visit_u64<E: Error>(self, value: u64) -> Result<Field, E> { Ok(Field::Number(value)) }
    fn visit_i64<E: Error>(self, value: i64) -> Result<Field, E> {
        Ok(u64::try_from(value).map(Field::Number).unwrap_or(Field::Signed(value)))
    }
    fn visit_bool<E: Error>(self, value: bool) -> Result<Field, E> { Ok(Field::Boolean(value)) }
    fn visit_unit<E: Error>(self) -> Result<Field, E> { Ok(Field::Null) }
    fn visit_f64<E: Error>(self, n: f64) -> Result<Field, E> { Ok(Field::Decimal(n)) }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Field, A::Error> {
        if matches!(self, Self::Comparison) {
            let mut values = serde_json::Map::new();
            let mut valid = true;
            while let Some(key) = map.next_key::<String>()? {
                if matches!(key.as_str(), "count" | "amount") {
                    match map.next_value_seed(Self::Scalar)?.metric() {
                        Some(n) if !values.contains_key(&key) => { values.insert(key,n); }
                        _ => valid = false,
                    }
                } else {
                    map.next_value::<IgnoredAny>()?;
                }
            }
            return Ok(if valid && !values.is_empty() {
                Field::Comparison(serde_json::Value::Object(values))
            } else { Field::Unsupported });
        }
        if matches!(self, Self::Coverage) {
            let (mut start, mut end) = (None, None);
            let (mut seen_start, mut seen_end) = (false, false);
            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "start" => {
                        if seen_start { return Err(A::Error::duplicate_field("start")); }
                        seen_start = true;
                        start = map.next_value_seed(Self::Scalar)?.text();
                    }
                    "end" => {
                        if seen_end { return Err(A::Error::duplicate_field("end")); }
                        seen_end = true;
                        end = map.next_value_seed(Self::Scalar)?.text();
                    }
                    _ => { map.next_value::<IgnoredAny>()?; }
                }
            }
            return Ok(match (start, end) {
                (Some(start), Some(end)) if iso_date(&start) && iso_date(&end) && start <= end =>
                    Field::Coverage(EvidenceCoverage { start, end }),
                _ => Field::Unsupported,
            });
        }
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(Field::Unsupported)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Field, A::Error> {
        if matches!(self, Self::Raw) {
            let mut values = Vec::new();
            // Importers store one value per Excel input cell. Keep malformed
            // historical arrays bounded without shifting the remaining cells.
            while let Some(field) = seq.next_element_seed(Self::Scalar)? {
                if values.len() == 8 { continue; }
                match field {
                    Field::Text(s) => values.push(clip(s,256).into()),
                    Field::Boolean(b) => values.push(b.into()),
                    scalar => if let Some(value)=scalar.metric() {values.push(value)} else {
                        while seq.next_element::<IgnoredAny>()?.is_some() {}
                        return Ok(Field::Unsupported);
                    }
                }
            }
            return Ok(Field::Raw(values));
        }
        if !matches!(self, Self::Cells) {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            return Ok(Field::Unsupported);
        }
        let mut cells = Vec::new();
        while cells.len() < 8 {
            match seq.next_element_seed(Self::Scalar)? {
                Some(Field::Text(cell)) => cells.push(clip(cell, 128)),
                Some(_) => {}
                None => return Ok(Field::Cells(cells)),
            }
        }
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Field::Cells(cells))
    }
}

struct EntryRead;

impl<'de> DeserializeSeed<'de> for EntryRead {
    type Value = Entry;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Entry, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for EntryRead {
    type Value = Entry;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a saved diagnostic")
    }

    fn visit_str<E: Error>(self, _: &str) -> Result<Entry, E> { Ok(Entry::default()) }
    fn visit_u64<E: Error>(self, _: u64) -> Result<Entry, E> { Ok(Entry::default()) }
    fn visit_i64<E: Error>(self, _: i64) -> Result<Entry, E> { Ok(Entry::default()) }
    fn visit_bool<E: Error>(self, _: bool) -> Result<Entry, E> { Ok(Entry::default()) }
    fn visit_unit<E: Error>(self) -> Result<Entry, E> { Ok(Entry::default()) }
    fn visit_f64<E: Error>(self, _: f64) -> Result<Entry, E> { Ok(Entry::default()) }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Entry, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Entry::default())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Entry, A::Error> {
        let mut issue = QualityIssue::default();
        let mut reason = None;
        let mut legacy_kind = None;
        let mut legacy_row = None;
        let mut mirrored = false;
        let mut seen = 0u8;
        let mut invalid = 0u8;
        while let Some(key) = map.next_key::<String>()? {
            let scalar = ValueShape::Scalar;
            let flag = match key.as_str() {
                "kind" => 1, "type" => 2, "date" => 4, "relevant" => 8,
                "scope" => 16, "skipped" => 32, _ => 0,
            };
            if flag != 0 {
                if seen & flag != 0 { invalid |= flag; }
                seen |= flag;
            }
            match key.as_str() {
                "reason" => reason = map.next_value_seed(scalar)?.text(),
                "kind" => {
                    match map.next_value_seed(scalar)?.nullable_text() {
                        Ok(value) => {
                            issue.kind = label(value);
                            if issue.kind.as_ref().is_some_and(|s| s.is_empty()) { invalid |= flag; }
                        }
                        Err(()) => invalid |= flag,
                    }
                }
                "type" => {
                    match map.next_value_seed(scalar)?.nullable_text() {
                        Ok(value) => {
                            legacy_kind = label(value);
                            if legacy_kind.as_ref().is_some_and(|s| s.is_empty()) { invalid |= flag; }
                        }
                        Err(()) => invalid |= flag,
                    }
                }
                "sheet" => issue.sheet = label(map.next_value_seed(scalar)?.text()),
                "organization" => issue.organization = label(map.next_value_seed(scalar)?.text()),
                "date" => {
                    match map.next_value_seed(scalar)?.nullable_text() {
                        Ok(value) => {
                            if value.as_deref().is_some_and(|s| !iso_date(s)) { invalid |= flag; }
                            issue.date = label(value);
                        }
                        Err(()) => invalid |= flag,
                    }
                }
                "source_row" | "sourceRow" => {
                    issue.source_row = map.next_value_seed(scalar)?.number().and_then(|n| u32::try_from(n).ok());
                }
                "row" => legacy_row = map.next_value_seed(scalar)?.number().and_then(|n| u32::try_from(n).ok()),
                "work_cell" | "workCell" => issue.work_cell = label(map.next_value_seed(scalar)?.text()),
                "count_cell" | "countCell" => issue.count_cell = label(map.next_value_seed(scalar)?.text()),
                "amount_cell" | "amountCell" => issue.amount_cell = label(map.next_value_seed(scalar)?.text()),
                "expected" => if let Field::Comparison(v)=map.next_value_seed(ValueShape::Comparison)? {issue.expected=Some(v)},
                "actual" => if let Field::Comparison(v)=map.next_value_seed(ValueShape::Comparison)? {issue.actual=Some(v)},
                "raw" => if let Field::Raw(v)=map.next_value_seed(ValueShape::Raw)? {issue.raw=Some(v)},
                "cells" => {
                    if let Field::Cells(cells) = map.next_value_seed(ValueShape::Cells)? { issue.cells = cells; }
                }
                "relevant" => {
                    match map.next_value_seed(scalar)? {
                        Field::Boolean(value) => issue.relevant = Some(value),
                        Field::Null => issue.relevant = None,
                        _ => invalid |= flag,
                    }
                }
                "scope" => {
                    match map.next_value_seed(scalar)?.nullable_text() {
                        Ok(value) => {
                            issue.scope = label(value);
                            if issue.scope.as_ref().is_some_and(|s| s.is_empty()) { invalid |= flag; }
                        }
                        Err(()) => invalid |= flag,
                    }
                }
                "skipped" => {
                    match map.next_value_seed(scalar)? {
                        Field::Boolean(value) => issue.skipped = Some(value),
                        Field::Null => issue.skipped = None,
                        _ => invalid |= flag,
                    }
                }
                "source_issue_sequence" => mirrored = map.next_value_seed(scalar)?.number().is_some(),
                _ => { map.next_value::<IgnoredAny>()?; }
            }
        }
        if matches!((&issue.kind, &legacy_kind), (Some(a), Some(b)) if a != b) {
            invalid |= 1;
        }
        // Equal native kind/type aliases and nullable options are valid. Only
        // invalidate facts that depend on each malformed classification field.
        if invalid & 3 != 0 { issue.kind = None; legacy_kind = None; }
        if invalid & 4 != 0 { issue.date = None; }
        if invalid & 16 != 0 { issue.scope = None; }
        if invalid & (1 | 2 | 4 | 8 | 16) != 0 { issue.relevant = None; }
        if invalid & (1 | 2 | 16 | 32) != 0 { issue.skipped = None; }
        Ok(Entry {
            mirrored,
            invalid_fields: invalid.count_ones(),
            issue: reason.map(|reason| {
                issue.reason = reason;
                issue.kind = issue.kind.or(legacy_kind);
                issue.source_row = issue.source_row.or(legacy_row);
                issue
            }),
        })
    }
}

#[derive(Default)]
struct Window {
    total: u32,
    invalid_entries: u32,
    issues: Vec<QualityIssue>,
    evidence: QualityEvidence,
}

fn iso_date(value: &str) -> bool {
    value.len() == 10 && value.bytes().enumerate().all(|(i, c)|
        if i == 4 || i == 7 { c == b'-' } else { c.is_ascii_digit() })
        && super::valid_date(value)
}

impl QualityEvidence {
    fn observe(&mut self, issue: &QualityIssue, invalid_fields: u32) {
        self.invalid_fields += invalid_fields;
        if issue.kind.as_deref() == Some("missing-day") {
            if issue.scope.as_deref() == Some("source-period")
                && issue.date.as_deref().is_some_and(iso_date)
            {
                match issue.relevant {
                    Some(true) => self.missing_day_entries += 1,
                    Some(false) => self.deferred_day_entries += 1,
                    None => self.unclassified_day_entries += 1,
                }
            } else { self.unclassified_day_entries += 1; }
        }
        if matches!(issue.kind.as_deref(), Some("invalid" | "skipped")) && issue.skipped == Some(true)
            && matches!(issue.scope.as_deref(), None | Some("report"))
        { self.omitted_row_entries += 1; }
    }
}

#[derive(Clone, Copy)]
struct Page {
    offset: u32,
    limit: usize,
    omit_mirrors: bool,
}

// SQLite's empty filename opens a private temporary database. Its bounded page
// cache keeps the exact membership test on disk even for large reports.
struct WarningIndex {
    db: Connection,
}

impl WarningIndex {
    fn new() -> Result<Self, String> {
        let db = Connection::open("").map_err(err)?;
        db.execute_batch(
            "PRAGMA cache_size=-512;
             PRAGMA temp_store=FILE;
             PRAGMA journal_mode=OFF;
             CREATE TABLE known(reason TEXT PRIMARY KEY) WITHOUT ROWID;
             CREATE TABLE warnings(ordinal INTEGER PRIMARY KEY, reason TEXT NOT NULL UNIQUE);
             BEGIN;",
        ).map_err(err)?;
        Ok(Self { db })
    }

    fn append(&self, window: &mut Window, page: Page) -> Result<bool, String> {
        let offset = page.offset.saturating_sub(window.total);
        let available = page.limit - window.issues.len();
        let mut query = self.db.prepare(
            "SELECT reason FROM warnings AS w
             WHERE NOT EXISTS(SELECT 1 FROM known WHERE reason=w.reason)
             ORDER BY ordinal LIMIT ?1 OFFSET ?2"
        ).map_err(err)?;
        let reasons = query.query_map(params![available as u32 + 1, offset], |row| row.get(0)).map_err(err)?;
        for reason in reasons {
            let reason = reason.map_err(err)?;
            if window.issues.len() == page.limit { return Ok(true); }
            window.issues.push(QualityIssue {
                kind: Some("warning".into()),
                reason: clip(reason, 4096),
                ..Default::default()
            });
        }
        Ok(false)
    }
}

struct WindowRead<'a, 'conn> {
    shape: ValueShape,
    page: Page,
    index: Option<&'a mut Statement<'conn>>,
}

impl<'de> DeserializeSeed<'de> for WindowRead<'_, '_> {
    type Value = Window;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Window, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for WindowRead<'_, '_> {
    type Value = Window;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an array of saved diagnostics")
    }

    fn visit_str<E: Error>(self, _: &str) -> Result<Window, E> { self.visit_unit() }
    fn visit_u64<E: Error>(self, _: u64) -> Result<Window, E> { self.visit_unit() }
    fn visit_i64<E: Error>(self, _: i64) -> Result<Window, E> { self.visit_unit() }
    fn visit_bool<E: Error>(self, _: bool) -> Result<Window, E> { self.visit_unit() }
    fn visit_f64<E: Error>(self, _: f64) -> Result<Window, E> { self.visit_unit() }
    fn visit_unit<E: Error>(self) -> Result<Window, E> {
        Ok(Window { invalid_entries: 1, ..Default::default() })
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Window, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        self.visit_unit()
    }

    fn visit_seq<A: SeqAccess<'de>>(mut self, mut seq: A) -> Result<Window, A::Error> {
        let mut result = Window::default();
        if matches!(self.shape, ValueShape::Warning) {
            let index = self.index.as_mut().expect("warning index");
            while let Some(field) = seq.next_element_seed(ValueShape::Scalar)? {
                if let Some(reason) = field.text() {
                    index.execute([reason]).map_err(A::Error::custom)?;
                } else {
                    result.invalid_entries += 1;
                }
            }
            return Ok(result);
        }
        while let Some(entry) = seq.next_element_seed(EntryRead)? {
            let Some(mut issue) = entry.issue else {
                result.invalid_entries += 1;
                continue;
            };
            if let Some(index) = self.index.as_mut() {
                index.execute([&issue.reason]).map_err(A::Error::custom)?;
            }
            if self.page.omit_mirrors && entry.mirrored { continue; }
            // Observe every active entry before applying the display window.
            result.evidence.observe(&issue, entry.invalid_fields);
            if result.total >= self.page.offset && result.issues.len() < self.page.limit {
                issue.reason = clip(issue.reason, 4096);
                result.issues.push(issue);
            }
            result.total += 1;
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
    evidence: QualityEvidence,
}

struct DocumentRead<'a> {
    scope: QualityScope,
    page: Page,
    index: Option<&'a WarningIndex>,
}

impl<'de> DeserializeSeed<'de> for DocumentRead<'_> {
    type Value = Document;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Document, D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for DocumentRead<'_> {
    type Value = Document;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a saved report object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Document, A::Error> {
        let mut doc = Document::default();
        let mut legacy_sheet = None;
        let mut metadata_seen = 0u8;
        while let Some(key) = map.next_key::<String>()? {
            let flag = match key.as_str() {
                "sourceFormat" => 1, "sheetName" => 2,
                "sourceCoverage" => 4, "explicitZero" => 8, _ => 0,
            };
            if flag != 0 {
                if metadata_seen & flag != 0 { return Err(A::Error::custom(format!("duplicate source metadata field: {key}"))); }
                metadata_seen |= flag;
            }
            match key.as_str() {
                "sourceSheet" => doc.sheet = label(map.next_value_seed(ValueShape::Scalar)?.text()),
                "sheetName" => {
                    match map.next_value_seed(ValueShape::Scalar)?.nullable_text() {
                        Ok(value) => {
                            legacy_sheet = label(value);
                            doc.evidence.sheet_name = legacy_sheet.clone().filter(|s| !s.is_empty());
                            if legacy_sheet.as_ref().is_some_and(|s| s.is_empty()) { doc.evidence.invalid_fields += 1; }
                        }
                        Err(()) => doc.evidence.invalid_fields += 1,
                    }
                }
                "sourceFormat" => {
                    match map.next_value_seed(ValueShape::Scalar)?.nullable_text() {
                        Ok(value) => {
                            if value.as_ref().is_some_and(|s| s.is_empty()) { doc.evidence.invalid_fields += 1; }
                            doc.evidence.source_format = label(value.filter(|s| !s.is_empty()));
                        }
                        Err(()) => doc.evidence.invalid_fields += 1,
                    }
                }
                "sourceCoverage" => {
                    match map.next_value_seed(ValueShape::Coverage)? {
                        Field::Coverage(coverage) => doc.evidence.coverage = Some(coverage),
                        Field::Null => {},
                        _ => doc.evidence.invalid_fields += 1,
                    }
                }
                "explicitZero" => {
                    match map.next_value_seed(ValueShape::Scalar)? {
                        Field::Boolean(value) => doc.evidence.explicit_zero = Some(value),
                        Field::Null => {},
                        _ => doc.evidence.invalid_fields += 1,
                    }
                }
                "sourceDiagnosticsReport" => doc.anchor = map.next_value_seed(ValueShape::Scalar)?.text(),
                "issues" if self.scope == QualityScope::Report => {
                    if doc.issues.is_some() { return Err(A::Error::duplicate_field("issues")); }
                    let index = self.index.expect("report warning index");
                    let mut put = index.db.prepare("INSERT OR IGNORE INTO known VALUES(?1)").map_err(A::Error::custom)?;
                    doc.issues = Some(map.next_value_seed(WindowRead {
                        shape: ValueShape::Issue, page: self.page, index: Some(&mut put),
                    })?);
                }
                "warnings" if self.scope == QualityScope::Report => {
                    if doc.warnings.is_some() { return Err(A::Error::duplicate_field("warnings")); }
                    let index = self.index.expect("report warning index");
                    let mut put = index.db.prepare("INSERT OR IGNORE INTO warnings(reason) VALUES(?1)").map_err(A::Error::custom)?;
                    doc.warnings = Some(map.next_value_seed(WindowRead {
                        shape: ValueShape::Warning, page: self.page, index: Some(&mut put),
                    })?);
                }
                "sourceIssues" => {
                    if doc.source_issues.is_some() { return Err(A::Error::duplicate_field("sourceIssues")); }
                    doc.source_issues = Some(if self.scope == QualityScope::Source {
                        map.next_value_seed(WindowRead {
                            shape: ValueShape::Issue, page: self.page, index: None,
                        })?
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

fn read_document(conn: &Connection, id: &str, scope: QualityScope, page: Page, index: Option<&WarningIndex>) -> Result<Document, String> {
    let rowid = conn.query_row("SELECT rowid FROM reports WHERE id=?1", [id], |r| r.get(0))
        .optional().map_err(err)?.ok_or("Сохранённый отчёт не найден.")?;
    let blob = conn.blob_open("main", "reports", "json", rowid, true).map_err(err)?;
    let bounded = read_model::TokenBounded::new(blob).limit_depth();
    let mut deserializer = serde_json::Deserializer::from_reader(BufReader::with_capacity(8192, bounded));
    let doc = DocumentRead { scope, page, index }.deserialize(&mut deserializer).map_err(err)?;
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
        let index = (scope == QualityScope::Report).then(WarningIndex::new).transpose()?;
        let (doc, source_anchor) = match (scope, live_anchor) {
            (QualityScope::Source, Some((anchor, _))) => {
                let doc = read_document(&tx, &anchor, scope, page, None)?;
                (doc, Some(anchor))
            }
            (QualityScope::Report, live) => {
                // If the stored anchor is missing, preserve any report copies.
                page.omit_mirrors = live.as_ref().is_some_and(|(_, exists)| *exists);
                let doc = read_document(&tx, id, scope, page, index.as_ref())?;
                let anchor = live.map(|(anchor, _)| anchor).or_else(|| document_anchor(&doc, id));
                (doc, anchor)
            }
            (QualityScope::Source, None) => {
                let doc = read_document(&tx, id, scope, page, index.as_ref())?;
                let anchor = document_anchor(&doc, id).ok_or("Общие замечания источника не сохранены.")?;
                let doc = if anchor == id { doc } else { read_document(&tx, &anchor, scope, page, None)? };
                (doc, Some(anchor))
            }
        };
        let mut window = match scope {
            QualityScope::Report => doc.issues.unwrap_or_default(),
            QualityScope::Source => doc.source_issues.ok_or("Общие замечания источника не найдены в сохранённом отчёте.")?,
        };
        let after = offset.saturating_add(limit as u32);
        let mut more = window.total > after;
        let mut warning_fallback = false;
        if let (Some(index), Some(warnings)) = (index, doc.warnings) {
            window.invalid_entries += warnings.invalid_entries;
            if !more {
                more = index.append(&mut window, page)?;
                warning_fallback = window.total == 0 && !window.issues.is_empty();
            }
        }
        for issue in &mut window.issues {
            if issue.sheet.is_none() {
                issue.sheet.clone_from(&doc.sheet);
            }
        }
        let mut evidence = doc.evidence;
        evidence.document_id = if scope == QualityScope::Report { id.to_owned() } else { source_anchor.clone().expect("source document anchor") };
        evidence.invalid_fields += window.evidence.invalid_fields;
        evidence.missing_day_entries = window.evidence.missing_day_entries;
        evidence.deferred_day_entries = window.evidence.deferred_day_entries;
        evidence.unclassified_day_entries = window.evidence.unclassified_day_entries;
        evidence.omitted_row_entries = window.evidence.omitted_row_entries;
        tx.commit().map_err(err)?;
        Ok(ReportQualityPage {
            issues: window.issues,
            next_offset: more.then_some(after),
            source_anchor,
            warning_fallback,
            invalid_entries: window.invalid_entries,
            evidence,
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
