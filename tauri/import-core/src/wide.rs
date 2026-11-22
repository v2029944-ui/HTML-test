//! Disk-backed shared-work daily import. Only <=1024 column pairs and bounded
//! sheet metadata are resident; financial rows and diagnostics stay in SQLite.
use crate::report_stage::{self, cell, normalize, number, page_limit};
use crate::{check_cancel, fail, ConversionSummary, LegacyOptions, Result, Stage, TripleMapping};
use chrono::{Datelike, Duration, Local, Months, NaiveDate};
use regex::Regex;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock,
    },
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WideMapping {
    pub work_col: u32,
    pub header_row: u32,
    pub first_day_col: u32,
    pub total_count_col: Option<u32>,
    pub total_amount_col: Option<u32>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WideSequentialPeriod {
    pub year: i32,
    /// None means a complete year; Some(1..=4) means a complete quarter.
    pub quarter: Option<u32>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WideOptions {
    /// Granularity of each pair, never an aggregation request.
    pub kind: String,
    /// Optional confirmed date for a single period lacking a header.
    pub start: Option<String>,
    pub year: Option<i32>,
    pub sheets: Vec<String>,
    pub mappings: BTreeMap<String, WideMapping>,
    /// Confirmed periods keyed by logical worksheet name; never inferred from totals.
    pub sequential_periods: BTreeMap<String, WideSequentialPeriod>,
    pub organizations: [String; 4],
}
impl Default for WideOptions {
    fn default() -> Self {
        Self {
            kind: "day".into(),
            start: None,
            year: None,
            sheets: vec![],
            mappings: BTreeMap::new(),
            sequential_periods: BTreeMap::new(),
            organizations: ["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Coverage {
    pub start: String,
    pub end: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct WideSheet {
    pub name: String,
    pub component: String,
    pub header_row: u32,
    pub work_col: u32,
    pub pairs: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReportProvenance {
    pub sheet_name: String,
    pub source_format: String,
    pub source_block: u32,
    pub explicit_zero: bool,
    pub source_control_date: Option<String>,
    pub coverage: Coverage,
}
#[derive(Debug)]
pub struct SourceIssue {
    pub sequence: u64,
    pub sheet: String,
    pub report_id: Option<String>,
    pub scope: String,
    pub kind: String,
    pub reason: String,
    pub date: Option<String>,
    pub source_row: Option<u32>,
    pub organization: Option<String>,
    pub count_cell: Option<String>,
    pub amount_cell: Option<String>,
    pub expected_count: Option<f64>,
    pub expected_amount: Option<f64>,
    pub actual_count: Option<f64>,
    pub actual_amount: Option<f64>,
    pub relevant: bool,
}
impl Serialize for SourceIssue {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        // Share the diagnostic boundary, including nullable source-control comparisons.
        #[derive(Serialize)]
        struct View<'a> {
            #[serde(flatten)]
            diagnostic: crate::ReportIssue,
            sheet: &'a str,
            report_id: &'a Option<String>,
            relevant: bool,
        }
        View {
            diagnostic: crate::ReportIssue {
                skipped: None,
                sequence: self.sequence,
                kind: self.kind.clone(),
                reason: self.reason.clone(),
                source_row: self.source_row,
                organization: self.organization.clone(),
                work_cell: None,
                count_cell: self.count_cell.clone(),
                amount_cell: self.amount_cell.clone(),
                expected_count: self.expected_count,
                expected_amount: self.expected_amount,
                actual_count: self.actual_count,
                actual_amount: self.actual_amount,
                scope: Some(self.scope.clone()),
                date: self.date.clone(),
                source_issue_sequence: Some(self.sequence),
            },
            sheet: &self.sheet,
            report_id: &self.report_id,
            relevant: self.relevant,
        }
        .serialize(serializer)
    }
}
#[derive(Clone, Copy)]
struct Pair {
    q: u32,
    a: u32,
}
struct Info {
    work: u32,
    header: u32,
    pairs: Vec<Pair>,
    totals: Option<Pair>,
}
#[derive(Clone, Copy, Debug)]
struct Parts {
    year: Option<i32>,
    suffix: Option<i32>,
    month: u32,
    day: u32,
}
struct Day {
    pair: Pair,
    parts: Parts,
    header_row: u32,
    header_col: u32,
    date: NaiveDate,
}
struct Patterns {
    quantity: Regex,
    amount: Regex,
    work: Regex,
    total: Regex,
    numeric: Regex,
    iso: Regex,
    text: Regex,
    quarter: Regex,
    quarter2: Regex,
    quarter_any: Regex,
    quarter2_any: Regex,
    roman_any: Regex,
    roman: Regex,
    annual: Regex,
    strip_prefix: Regex,
    strip_suffix: Regex,
}
fn js_regex(pattern: &str) -> Regex {
    let ws = r"\x09-\x0d\x20\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000\ufeff";
    let mut out = String::new();
    let mut chars = pattern.chars();
    let mut class = false;
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                if next == 's' || next == 'd' {
                    let part = if next == 's' { ws } else { "0-9" };
                    if !class {
                        out.push('[');
                    }
                    out.push_str(part);
                    if !class {
                        out.push(']');
                    }
                } else {
                    out.push(c);
                    out.push(next);
                }
            }
        } else {
            if c == '[' {
                class = true;
            } else if c == ']' {
                class = false;
            }
            out.push(c);
        }
    }
    Regex::new(&out).expect("constant date expression")
}
impl Patterns {
    fn new() -> Self {
        let r = js_regex;
        Self {
            quantity: r(r"количеств|кол[\s.-]*во|^кол\.?$|объ[её]м"),
            amount: r(r"сумм|стоимост|выручк"),
            work: r(r"наименован|работ|услуг"),
            total: r(r"(?i)итог|всего|общ(?:ее|ая|ий)|total"),
            numeric: r(
                r"^(\d{1,2})[./-](\d{1,2})(?:[./-](\d{4}|\d{2}))?(?:\s*\((?:пн|вт|ср|чт|пт|сб|вс)\))?$",
            ),
            iso: r(r"^(\d{4})-(\d{1,2})-(\d{1,2})(?:[t ]00:00(?::00)?)?$"),
            text: r(r"^(\d{1,2})\s+([а-яё]+)\.?\s*(\d{4})?$"),
            quarter: r(r"(?i)(?:^|[^a-z0-9])(?:q\s*|кв\.?\s*)([1-4])(?:[^a-z0-9]|$)"),
            quarter2: r(r"(?i)([1-4])\s*кв(?:артал|\.)?"),
            quarter_any: r(r"(?i)(?:^|[^a-z0-9])(?:q\s*|кв\.?\s*)(\d+)"),
            quarter2_any: r(r"(?i)(\d+)\s*кв(?:артал|\.)?"),
            roman_any: r(r"(?i)(?:^|\s)([ivxlcdm]+)\s*(?:квартал|кв\.?)"),
            roman: r(r"(?i)(?:^|\s)(iv|iii|ii|i)\s*(?:квартал|кв\.?)"),
            annual: r(r"(?i)годов|за\s+\d{4}\s*год|year|annual"),
            strip_prefix: r(r"^(?:количество|кол[.-]?во|сумма)\s*[:;,·—-]?\s*"),
            strip_suffix: r(r"[\s,:;·—-]*(?:количество|кол[.-]?во|сумма)$"),
        }
    }
}
const WIDE_SCHEMA:&str="CREATE TABLE IF NOT EXISTS wide_provenance(report_id TEXT PRIMARY KEY,sheet_name TEXT,source_block INTEGER,explicit_zero INTEGER,source_control_date TEXT,coverage_start TEXT,coverage_end TEXT) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS wide_source_issues(seq INTEGER PRIMARY KEY,sheet TEXT,report_id TEXT,kind TEXT,reason TEXT,date TEXT,source_row INTEGER,organization TEXT,count_cell TEXT,amount_cell TEXT,expected_count REAL,expected_amount REAL,actual_count REAL,actual_amount REAL,relevant INTEGER);
CREATE INDEX IF NOT EXISTS wide_source_sheet ON wide_source_issues(sheet,seq);
CREATE TABLE IF NOT EXISTS wide_days(sheet TEXT,date TEXT PRIMARY KEY,q INTEGER,a INTEGER,header_row INTEGER,header_col INTEGER) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS wide_days_sheet ON wide_days(sheet,q);
CREATE TABLE IF NOT EXISTS date_styles(style INTEGER PRIMARY KEY) WITHOUT ROWID;";
fn blank(s: &str) -> bool {
    s.trim_matches(report_stage::js_space).is_empty()
}
fn resolved() -> &'static str {
    "CASE WHEN c.kind='s' THEN s.value WHEN c.kind='b' THEN CASE c.value WHEN '1' THEN 'true' ELSE 'false' END ELSE c.value END"
}
impl Stage<'_> {
    fn hook(&self, cancel: Arc<AtomicBool>) {
        self.db
            .progress_handler(10000, Some(move || cancel.load(Ordering::Acquire)));
    }
    fn unhook(&self) {
        self.db.progress_handler(0, None::<fn() -> bool>);
    }
    fn wide_setup(&self, cancel: &AtomicBool) -> Result<()> {
        self.db.execute_batch(WIDE_SCHEMA)?;
        self.db.execute_batch("CREATE INDEX IF NOT EXISTS cells_by_column ON cells(sheet,col_num,row_num)")?;
        self.setup_date_styles(cancel)
    }
    pub(crate) fn setup_date_styles(&self, cancel: &AtomicBool) -> Result<()> {
        self.db.execute_batch("CREATE TABLE IF NOT EXISTS date_styles(style INTEGER PRIMARY KEY) WITHOUT ROWID; CREATE TABLE IF NOT EXISTS custom_formats(id INTEGER PRIMARY KEY,format TEXT); DELETE FROM date_styles; DELETE FROM custom_formats;")?;
        let mut formats=self.db.prepare("SELECT i.value,coalesce(f.value,'') FROM nodes n JOIN attributes i ON i.component=n.component AND i.seq=n.seq AND i.key='numFmtId' LEFT JOIN attributes f ON f.component=n.component AND f.seq=n.seq AND f.key='formatCode' WHERE n.component='xl/styles.xml' AND n.tag='numFmt' ORDER BY n.seq")?;
        let mut rows = formats.query([])?;
        while let Some(row) = rows.next()? {
            check_cancel(cancel)?;
            let id: String = row.get(0)?;
            let id = id
                .parse::<u32>()
                .map_err(|_| fail("invalid Excel number format id"))?;
            let format: String = row.get(1)?;
            self.db.execute("INSERT INTO custom_formats VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET format=excluded.format",params![id,format])?;
        }
        let mut styles=self.db.prepare("SELECT coalesce(a.value,'0') FROM nodes n LEFT JOIN attributes a ON a.component=n.component AND a.seq=n.seq AND a.key='numFmtId' WHERE n.component='xl/styles.xml' AND n.tag='xf' AND n.parent='cellXfs' ORDER BY n.seq")?;
        let mut rows = styles.query([])?;
        let mut index = 0u32;
        while let Some(row) = rows.next()? {
            check_cancel(cancel)?;
            let id: String = row.get(0)?;
            let id = id
                .parse::<u32>()
                .map_err(|_| fail("invalid Excel style format id"))?;
            let format: Option<String> = self
                .db
                .query_row("SELECT format FROM custom_formats WHERE id=?1", [id], |r| {
                    r.get(0)
                })
                .optional()?;
            let custom = format.is_some_and(|value| {
                let mut chars = value.chars();
                while let Some(ch) = chars.next() {
                    match ch {
                        '"' => { chars.by_ref().take_while(|c| *c != '"').count(); }
                        '[' => { chars.by_ref().take_while(|c| *c != ']').count(); }
                        '\\' | '_' | '*' => { chars.next(); }
                        'd' | 'D' | 'y' | 'Y' => return true,
                        _ => {}
                    }
                }
                false
            });
            if (14..=17).contains(&id) || id == 22 || custom {
                self.db
                    .execute("INSERT INTO date_styles VALUES(?1)", [index])?;
            }
            index += 1;
        }
        Ok(())
    }
    /// Validated display-name to worksheet component mapping from workbook relationships.
    pub fn sheet_names(&self) -> Result<Vec<(String, String)>> {
        let mut st=self.db.prepare("SELECT n.seq,a.value,r.value FROM nodes n JOIN attributes a ON a.component=n.component AND a.seq=n.seq AND a.key='name' JOIN attributes r ON r.component=n.component AND r.seq=n.seq AND (r.key='r:id' OR (r.namespace_uri='http://schemas.openxmlformats.org/officeDocument/2006/relationships' AND substr(r.key,-3)=':id')) WHERE n.component='xl/workbook.xml' AND n.tag='sheet' ORDER BY n.seq")?;
        let mut rs = st.query([])?;
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        while let Some(r) = rs.next()? {
            let name: String = r.get(1)?;
            let id: String = r.get(2)?;
            if out.len() >= 1024
                || name.encode_utf16().count() > 240
                || name.is_empty()
                || id.len() > 2048
                || !seen.insert(name.clone())
            {
                return Err(fail("workbook sheet metadata limit or duplicate name"));
            }
            let mut rel=self.db.prepare("SELECT t.value,m.value FROM nodes n JOIN attributes i ON i.component=n.component AND i.seq=n.seq AND i.key='Id' JOIN attributes t ON t.component=n.component AND t.seq=n.seq AND t.key='Target' LEFT JOIN attributes m ON m.component=n.component AND m.seq=n.seq AND m.key='TargetMode' WHERE n.component='xl/_rels/workbook.xml.rels' AND n.tag='Relationship' AND i.value=?1")?;
            let mut rr = rel.query([id])?;
            let Some(v) = rr.next()? else {
                return Err(fail("missing worksheet relationship"));
            };
            let target: String = v.get(0)?;
            let mode: Option<String> = v.get(1)?;
            if rr.next()?.is_some() {
                return Err(fail("duplicate worksheet relationship"));
            }
            if mode.as_deref() == Some("External") {
                return Err(fail("external worksheet relationship"));
            }
            let path = relationship_target(&target)?;
            let exists:bool=self.db.query_row("SELECT EXISTS(SELECT 1 FROM cells WHERE sheet=?1) OR EXISTS(SELECT 1 FROM metadata WHERE component=?1 AND name='worksheet')",[&path],|r|r.get(0))?;
            // Empty worksheets are legitimate; archive membership still establishes existence.
            if !exists && !self.stats.components.iter().any(|(n, _)| n == &path) {
                return Err(fail("missing worksheet component"));
            }
            out.push((name, path));
        }
        Ok(out)
    }
    fn value(&self, sheet: &str, row: u32, col: u32) -> Result<Option<(String, String, bool)>> {
        Ok(self.db.query_row(&format!("SELECT {},c.kind,EXISTS(SELECT 1 FROM date_styles d WHERE d.style=CAST(coalesce(nullif(c.style,''),'0') AS INTEGER)) FROM cells c LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER) WHERE c.sheet=?1 AND c.row_num=?2 AND c.col_num=?3",resolved()),params![sheet,row,col],|r|Ok((r.get::<_,Option<String>>(0)?.unwrap_or_default(),r.get(1)?,r.get(2)?))).optional()?)
    }
    fn header_value(
        &self,
        sheet: &str,
        row: u32,
        col: u32,
        local: bool,
    ) -> Result<Option<(String, String, bool)>> {
        let merge:Option<(u32,u32,u32)>=self.db.query_row("SELECT r,c,end_col FROM merges WHERE sheet=?1 AND r<=?2 AND end_row>=?2 AND c<=?3 AND end_col>=?3 ORDER BY seq LIMIT 1",params![sheet,row,col],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        if local && merge.is_some_and(|(_, c, e)| e - c > 1) {
            return Ok(None);
        }
        if let Some(v) = self.value(sheet, row, col)? {
            if !blank(&v.0) {
                return Ok(Some(v));
            }
        }
        if let Some((r, c, _)) = merge {
            self.value(sheet, r, c)
        } else {
            Ok(None)
        }
    }
    fn header_rows(&self, sheet: &str, header: u32, col: u32) -> Result<rusqlite::Statement<'_>> {
        let _ = (sheet, header, col);
        Ok(self.db.prepare("SELECT row_num FROM cells WHERE sheet=?1 AND col_num=?3 AND row_num<=?2 UNION SELECT min(end_row,?2) FROM merges WHERE sheet=?1 AND c<=?3 AND end_col>=?3 AND r<=?2 ORDER BY 1 DESC")?)
    }
    fn info(
        &self,
        sheet: &str,
        manual: Option<&WideMapping>,
        min_pairs: usize,
        orgs: &[String; 4],
        p: &Patterns,
        cancel: &AtomicBool,
    ) -> Result<Option<Info>> {
        let (height, width): (u32, u32) = self.db.query_row(
            "SELECT coalesce(max(row_num),0),coalesce(max(col_num),0) FROM cells WHERE sheet=?1",
            [sheet],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if let Some(m) = manual {
            if m.header_row == 0
                || m.header_row > height
                || [m.work_col, m.first_day_col]
                    .iter()
                    .any(|c| *c == 0 || *c > 2048)
                || m.total_count_col.is_some_and(|c| c == 0 || c > 2048)
                || m.total_amount_col.is_some_and(|c| c == 0 || c > 2048)
            {
                return Err(fail("Проверьте номера строк и столбцов."));
            }
            if m.total_amount_col.is_some() && m.total_count_col.is_none() {
                return Err(fail("Укажите столбец итогового количества вместе с итоговой суммой."));
            }
            let end = m.total_count_col.unwrap_or(width + 1);
            let pairs = (m.first_day_col..end.saturating_sub(1))
                .step_by(2)
                .map(|q| Pair { q, a: q + 1 })
                .collect::<Vec<_>>();
            let totals = m.total_count_col.map(|q| Pair {
                q,
                a: m.total_amount_col.unwrap_or(q + 1),
            });
            if end <= m.first_day_col || (end - m.first_day_col) % 2 != 0
                || pairs.is_empty()
                || pairs.len() > 1024
                || pairs.iter().any(|x| x.q == m.work_col || x.a == m.work_col)
                || totals.is_some_and(|x| {
                    x.a > 2048
                        || x.a == x.q
                        || x.q == m.work_col
                        || x.a == m.work_col
                        || pairs.iter().any(|y| y.q == x.a || y.a == x.a)
                })
            {
                return Err(fail("Проверьте пары столбцов количества и суммы."));
            }
            return Ok(Some(Info {
                work: m.work_col,
                header: m.header_row,
                pairs,
                totals,
            }));
        }
        let mut st=self.db.prepare(&format!("SELECT c.row_num,c.col_num,{} FROM cells c LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER) WHERE c.sheet=?1 ORDER BY c.row_num,c.col_num",resolved()))?;
        let mut rs = st.query([sheet])?;
        let (mut current, mut quantity) = (0, None);
        let mut pairs = Vec::new();
        while let Some(r) = rs.next()? {
            check_cancel(cancel)?;
            let row: u32 = r.get(0)?;
            let col: u32 = r.get(1)?;
            let val = normalize(&r.get::<_, Option<String>>(2)?.unwrap_or_default());
            if row != current {
                if pairs.len() >= min_pairs {
                    if let Some(i) = self.detect_work(
                        sheet,
                        current,
                        std::mem::take(&mut pairs),
                        orgs,
                        p,
                        cancel,
                    )? {
                        return Ok(Some(i));
                    }
                }
                pairs.clear();
                quantity = None;
                current = row;
            }
            if quantity == Some(col - 1) && p.amount.is_match(&val) {
                pairs.push(Pair { q: col - 1, a: col });
                quantity = None;
            } else {
                quantity = p.quantity.is_match(&val).then_some(col);
            }
        }
        if pairs.len() >= min_pairs {
            return self.detect_work(sheet, current, pairs, orgs, p, cancel);
        }
        Ok(None)
    }
    fn detect_work(
        &self,
        sheet: &str,
        header: u32,
        pairs: Vec<Pair>,
        orgs: &[String; 4],
        p: &Patterns,
        cancel: &AtomicBool,
    ) -> Result<Option<Info>> {
        let mut candidates: BTreeSet<u32> = (1..pairs[0].q).collect();
        for pair in &pairs {
            if pair.q > 1 {
                candidates.insert(pair.q - 1);
            }
        }
        let mut work = None;
        for &col in &candidates {
            check_cancel(cancel)?;
            let label = self
                .header_value(sheet, header, col, false)?
                .map(|v| normalize(&v.0))
                .unwrap_or_default();
            if p.work.is_match(&label) && !p.quantity.is_match(&label) && !p.amount.is_match(&label)
            {
                if work.is_some() {
                    return Ok(None);
                }
                work = Some(col);
            }
        }
        let mut org_column = None;
        // Scan each candidate using the column index, without retaining any rows.
        for &col in &candidates {
            let mut st=self.db.prepare(&format!("SELECT {} FROM cells c INDEXED BY cells_by_column LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER) WHERE c.sheet=?1 AND c.col_num=?2 AND c.row_num>?3",resolved()))?;
            let mut rows = st.query(params![sheet, col, header])?;
            while let Some(r) = rows.next()? {
                check_cancel(cancel)?;
                let v: String = r.get::<_, Option<String>>(0)?.unwrap_or_default();
                if orgs.iter().any(|o| normalize(o) == normalize(&v)) {
                    if org_column.is_some_and(|c| c != col) {
                        return Ok(None);
                    }
                    org_column = Some(col);
                    break;
                }
            }
        }
        let Some(work) = work.or(org_column.filter(|c| *c < pairs[0].q)) else {
            return Ok(None);
        };
        if work >= pairs[0].q {
            return Ok(None);
        }
        Ok(Some(Info {
            work,
            header,
            pairs,
            totals: None,
        }))
    }
    /// Classify only repeated structures with explicit work and metric headers.
    /// One triple is compatible with both layouts.
    pub fn worksheet_layout(&self, sheet: &str) -> Result<Option<&'static str>> {
        let p = Patterns::new();
        let mut stmt = self.db.prepare(&format!("SELECT c.row_num,c.col_num,{} FROM cells c LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER) WHERE c.sheet=?1 AND c.row_num<=64 ORDER BY c.row_num,c.col_num", resolved()))?;
        let mut rows = stmt.query([sheet])?;
        let mut work = BTreeMap::<u32, BTreeSet<u32>>::new();
        let mut pairs = BTreeMap::<u32, Vec<u32>>::new();
        let mut totals = BTreeMap::<u32, u32>::new();
        let mut quantity = None;
        while let Some(row) = rows.next()? {
            let r: u32 = row.get(0)?;
            let c: u32 = row.get(1)?;
            let value = normalize(&row.get::<_, Option<String>>(2)?.unwrap_or_default());
            if p.work.is_match(&value) && !p.amount.is_match(&value) && !p.quantity.is_match(&value) {
                work.entry(r).or_default().insert(c);
            }
            if p.total.is_match(&value) { totals.entry(c).or_insert(r); }
            if quantity == Some((r, c - 1)) && p.amount.is_match(&value) {
                pairs.entry(r).or_default().push(c - 1);
            }
            quantity = p.quantity.is_match(&value).then_some((r, c));
        }
        let mut hint = None;
        for (row, mut cols) in pairs {
            cols.retain(|q| ![*q, *q + 1].iter().any(|col| totals.get(col).is_some_and(|r| *r <= row)));
            if cols.len() < 2 { continue; }
            let Some((_, labels)) = work.range(..=row).next_back() else { continue };
            let next = if cols.iter().all(|c| labels.contains(&(c - 1))) {
                Some("legacy")
            } else if labels.len() == 1 && labels.first().is_some_and(|c| *c < cols[0])
                && cols.windows(2).all(|pair| pair[1] == pair[0] + 2) {
                Some("daily-wide")
            } else { None };
            if next.is_none() || hint.is_some_and(|prior| Some(prior) != next) { return Ok(None); }
            hint = next;
        }
        Ok(hint)
    }

    /// Old auto rules apply their kind and start to ordinary blocks. A shared
    /// calendar with adjacent daily headers retained its own dates and sheets.
    pub fn classic_daily_workbook(
        &self, book: &[(String, String)], orgs: &[String; 4], cancel: &AtomicBool,
    ) -> Result<bool> {
        if book.is_empty() || book.len() > 12 { return Ok(false); }
        let p = Patterns::new();
        let mut active = Vec::new();
        for (_, sheet) in book {
            check_cancel(cancel)?;
            if self.page(sheet, 0, 1)?.is_empty() { continue; }
            if self.worksheet_layout(sheet)? != Some("daily-wide") { return Ok(false); }
            active.push(sheet);
        }
        if active.is_empty() { return Ok(false); }
        self.wide_setup(cancel)?;
        let date1904 = self.uses_1904_epoch()?;
        for sheet in active {
            let Some(info) = self.info(sheet, None, 2, orgs, &p, cancel)? else { return Ok(false); };
            let mut previous: Option<NaiveDate> = None;
            let mut adjacent = false;
            // Two adjacent calendar starts rule out non-overlapping weeks or months.
            // Full conversion still verifies every pair, date and control total.
            for pair in info.pairs.iter().take(3) {
                let Some((parts, _, _)) = self.pair_date(sheet, &info, *pair, "detect-day", date1904, &p, cancel)? else { break };
                let Some(date) = NaiveDate::from_ymd_opt(parts.year.unwrap_or(2000), parts.month, parts.day) else { return Ok(false); };
                if previous.is_some_and(|d| d.succ_opt() == Some(date)) { adjacent = true; break; }
                previous = Some(date);
            }
            if !adjacent { return Ok(false); }
        }
        Ok(true)
    }

    fn uses_1904_epoch(&self) -> Result<bool> {
        Ok(self.db.query_row("SELECT EXISTS(SELECT 1 FROM nodes n JOIN attributes a ON a.component=n.component AND a.seq=n.seq AND a.key='date1904' WHERE n.component='xl/workbook.xml' AND n.tag='workbookPr' AND a.value IN ('1','true'))",[],|r|r.get(0))?)
    }

    pub fn detect_wide(
        &self,
        cfg: &WideOptions,
        cancel: Arc<AtomicBool>,
    ) -> Result<Vec<WideSheet>> {
        validate(cfg)?;
        check_cancel(&cancel)?;
        self.hook(cancel.clone());
        let result = (|| {
            let p = Patterns::new();
            self.wide_setup(&cancel)?;
            let mut found = Vec::new();
            for (name, path) in self.sheet_names()? {
                if !cfg.sheets.is_empty() && !cfg.sheets.contains(&name) {
                    continue;
                }
                check_cancel(&cancel)?;
                if let Some(i) = self.info(
                    &path,
                    cfg.mappings.get(&name),
                    2,
                    &cfg.organizations,
                    &p,
                    &cancel,
                )? {
                    if found.len() >= 12 {
                        return Err(fail("more than 12 daily sheets; select an explicit subset"));
                    }
                    found.push(WideSheet {
                        name,
                        component: path,
                        header_row: i.header,
                        work_col: i.work,
                        pairs: i.pairs.len(),
                    });
                }
            }
            if cfg
                .sequential_periods
                .keys()
                .any(|name| !found.iter().any(|s| &s.name == name))
            {
                return Err(fail("sequential period refers to an unused worksheet"));
            }
            Ok(found)
        })();
        self.unhook();
        result
    }
    fn column_date(
        &self,
        sheet: &str,
        info: &Info,
        col: u32,
        period_kind: &str,
        date1904: bool,
        p: &Patterns,
        cancel: &AtomicBool,
    ) -> Result<Option<(Parts, u32, u32)>> {
        let mut st = self.header_rows(sheet, info.header, col)?;
        let mut rows = st.query(params![sheet, info.header, col])?;
        while let Some(r) = rows.next()? {
            check_cancel(cancel)?;
            let row: u32 = r.get(0)?;
            if let Some((v, kind, styled)) = self.header_value(sheet, row, col, true)? {
                if let Some(parts) = period_header_date(&v, &kind, styled, date1904, period_kind, p)? {
                    return Ok(Some((parts, row, col)));
                }
            }
        }
        let mut st = self.header_rows(sheet, info.header.saturating_sub(1), col)?;
        let mut rows = st.query(params![sheet, info.header.saturating_sub(1), col])?;
        while let Some(r) = rows.next()? {
            check_cancel(cancel)?;
            let row: u32 = r.get(0)?;
            let day = self
                .header_value(sheet, row, col, true)?
                .map(|v| v.0)
                .unwrap_or_default();
            if day.len() > 2 || day.is_empty() || !day.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let mut higher = self.header_rows(sheet, row.saturating_sub(1), col)?;
            let mut hr = higher.query(params![sheet, row.saturating_sub(1), col])?;
            while let Some(h) = hr.next()? {
                check_cancel(cancel)?;
                let n: u32 = h.get(0)?;
                if let Some(v) = self.header_value(sheet, n, col, false)? {
                    if let Some(parts) =
                        date_parts(&format!("{day} {}", v.0), "inlineStr", false, false, p)?
                    {
                        return Ok(Some((parts, row, col)));
                    }
                }
            }
        }
        Ok(None)
    }
    fn pair_date(
        &self,
        sheet: &str,
        i: &Info,
        pair: Pair,
        period_kind: &str,
        date1904: bool,
        p: &Patterns,
        cancel: &AtomicBool,
    ) -> Result<Option<(Parts, u32, u32)>> {
        let a = self.column_date(sheet, i, pair.q, period_kind, date1904, p, cancel)?;
        let b = self.column_date(sheet, i, pair.a, period_kind, date1904, p, cancel)?;
        match (a, b) {
            (None, x) | (x, None) => Ok(x),
            (Some((mut a, r, c)), Some((b, _, _))) => {
                if a.day != b.day
                    || a.month != b.month
                    || matches!((a.year,b.year),(Some(x),Some(y))if x!=y)
                    || matches!((a.year,b.suffix),(Some(x),Some(y))if x%100!=y)
                    || matches!((b.year,a.suffix),(Some(x),Some(y))if x%100!=y)
                    || matches!((a.suffix,b.suffix),(Some(x),Some(y))if x!=y)
                {
                    return Err(fail("quantity and amount header dates conflict"));
                }
                if a.year.is_none() {
                    a.year = b.year;
                }
                a.suffix = a.suffix.or(b.suffix);
                Ok(Some((a, r, c)))
            }
        }
    }
    /// Uses the local calendar date, matching browser reportToday().
    pub fn convert_wide(
        &mut self,
        cfg: &WideOptions,
        cancel: Arc<AtomicBool>,
        progress: impl FnMut(ConversionSummary),
    ) -> Result<ConversionSummary> {
        self.convert_wide_as_of(cfg, cancel, Local::now().date_naive(), progress)
    }
    /// Deterministic clock injection for tests/embedding; no CLI date-mode switch.
    pub fn convert_wide_as_of(
        &mut self,
        cfg: &WideOptions,
        cancel: Arc<AtomicBool>,
        today: NaiveDate,
        mut progress: impl FnMut(ConversionSummary),
    ) -> Result<ConversionSummary> {
        validate(cfg)?;
        check_cancel(&cancel)?;
        self.hook(cancel.clone());
        let result = (|| {
            let p = Patterns::new();
            self.wide_setup(&cancel)?;
            self.db.execute_batch(report_stage::SCHEMA)?;
            self.db.execute_batch("BEGIN IMMEDIATE;DELETE FROM converted_reports;DELETE FROM converted_rows;DELETE FROM converted_issues;DELETE FROM converted_totals;DELETE FROM converted_organizations;DELETE FROM wide_provenance;DELETE FROM wide_source_issues;DELETE FROM wide_days;DELETE FROM manifest WHERE name='conversion_complete';")?;
            let book = self.sheet_names()?;
            let names = if cfg.sheets.is_empty() {
                let mut names = Vec::new();
                for (name, path) in &book {
                    check_cancel(&cancel)?;
                    if self
                        .info(
                            path,
                            cfg.mappings.get(name),
                            if book.len() == 1 || cfg.start.is_some() { 1 } else { 2 },
                            &cfg.organizations,
                            &p,
                            &cancel,
                        )?
                        .is_some()
                    {
                        if names.len() >= 12 {
                            return Err(fail("select at most 12 daily sheets"));
                        }
                        names.push(name.clone());
                    }
                }
                names
            } else {
                cfg.sheets.clone()
            };
            if let Some(name) = cfg.mappings.keys().find(|name| !names.contains(name)) {
                return Err(fail(&format!("Лист «{name}» из ручной настройки отсутствует среди листов для загрузки. Проверьте книгу и правило.")));
            }
            if names.is_empty() {
                return Err(fail("Не найден лист с единым списком работ и парами столбцов. Проверьте структуру или укажите столбцы вручную."));
            }
            if names.len() > 1 && cfg.start.is_some() {
                return Err(fail("Ручная дата относится к одному листу. Выберите лист или используйте даты в Excel."));
            }
            if cfg
                .sequential_periods
                .keys()
                .any(|name| !names.contains(name))
            {
                return Err(fail("sequential period refers to an unused worksheet"));
            }
            let date1904 = self.uses_1904_epoch()?;
            let mut summary = ConversionSummary::default();
            let mut occupied = BTreeMap::<NaiveDate, NaiveDate>::new();
            let confirmed_start = cfg.start.as_deref().map(parse_start).transpose()?;
            for name in names {
                check_cancel(&cancel)?;
                let sequential = cfg.sequential_periods.get(&name);
                let sequential_bounds = sequential.map(period_bounds).transpose()?;
                let path = &book
                    .iter()
                    .find(|x| x.0 == name)
                    .ok_or_else(|| fail("selected worksheet missing"))?
                    .1;
                let info = self
                    .info(
                        path,
                        cfg.mappings.get(&name),
                        1,
                        &cfg.organizations,
                        &p,
                        &cancel,
                    )?
                    .ok_or_else(|| fail("Не найдены пары столбцов для выбранного листа."))?;
                let mut days = Vec::new();
                let mut total = None;
                let mut used_start = false;
                let mut years = BTreeSet::new();
                for pair in &info.pairs {
                    check_cancel(&cancel)?;
                    let (mut is_total, mut metric_total) = (false, false);
                    let mut st = self.header_rows(path, info.header, pair.q)?;
                    let mut rs = st.query(params![path, info.header, pair.q])?;
                    while let Some(r) = rs.next()? {
                        check_cancel(&cancel)?;
                        let row: u32 = r.get(0)?;
                        if self
                            .header_value(path, row, pair.q, true)?
                            .is_some_and(|v| p.total.is_match(&v.0))
                        {
                            if row < info.header {
                                is_total = true
                            } else {
                                metric_total = true
                            }
                        }
                    }
                    let date = if is_total {
                        None
                    } else {
                        self.pair_date(path, &info, *pair, &cfg.kind, date1904, &p, &cancel)?
                    };
                    if is_total || date.is_none() && metric_total {
                        if total.is_some() && info.totals.is_none() {
                            return Err(fail("multiple total pairs; provide manual totals"));
                        }
                        total = Some(*pair);
                        continue;
                    }
                    let (parts, row, col) = if let Some((begin, _)) = sequential_bounds {
                        let expected = begin + Duration::days(days.len() as i64);
                        if let Some((parts, _, _)) = date {
                            if parts.year.is_some_and(|year| year != expected.year())
                                || parts
                                    .suffix
                                    .is_some_and(|year| year != expected.year() % 100)
                                || parts.month != expected.month()
                                || parts.day != expected.day()
                            {
                                return Err(fail(
                                    "header date conflicts with sequential daily position",
                                ));
                            }
                        }
                        (
                            Parts {
                                year: Some(expected.year()),
                                suffix: None,
                                month: expected.month(),
                                day: expected.day(),
                            },
                            date.map_or(info.header, |d| d.1),
                            date.map_or(pair.q, |d| d.2),
                        )
                    } else if date.is_none() && confirmed_start.is_some() && days.is_empty() {
                        let first = confirmed_start.unwrap();
                        used_start = true;
                        (Parts { year: Some(first.year()), suffix: None, month: first.month(), day: first.day() },
                         info.header, pair.q)
                    } else {
                        date.ok_or_else(|| {
                            fail(&format!(
                                "Над ячейками {} / {} нет даты периода. Для единственного периода укажите начало вручную.",
                                cell(pair.q, info.header),
                                cell(pair.a, info.header)
                            ))
                        })?
                    };
                    if let Some(year) = parts.year {
                        years.insert(year);
                    }
                    days.push(Day {
                        pair: *pair,
                        parts,
                        header_row: row,
                        header_col: col,
                        date: NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
                    });
                }
                if used_start && days.len() != 1 {
                    return Err(fail("Укажите в заголовках даты каждой пары столбцов. Ручная дата подходит для одного периода."));
                }
                if days.is_empty() {
                    return Err(fail("no daily date columns"));
                }
                if sequential_bounds
                    .is_some_and(|(begin, end)| days.len() as i64 != (end - begin).num_days() + 1)
                {
                    return Err(fail(
                        "daily pair count does not match the explicit sequential period",
                    ));
                }
                if cfg
                    .year
                    .is_some_and(|year| years.iter().any(|y| *y != year))
                {
                    return Err(fail("chosen year conflicts with complete header dates"));
                }
                let infer = years.is_empty();
                let mut quarters = [None; 3];
                let mut sequential_quarters = BTreeSet::new();
                let mut invalid_quarter = false;
                let mut annual = false;
                // Legacy coverage examines sheet name plus all string header
                // cells joined by spaces. Collapse JS whitespace and retain a
                // 64-character suffix: every coverage expression spans fewer
                // than 32 non-whitespace characters. A cell remains <=64KiB;
                // the complete header is never concatenated in memory.
                let mut coverage_tail = String::new();
                let mut coverage_cropped = false;
                let mut inspect = |value: &str| {
                    if infer {
                        infer_years(value, &mut years);
                    } else if sequential.is_some() {
                        find_years(value, &mut years, 9999);
                    }
                    // Cropping must not invent ^/left-word boundaries. Matches
                    // starting at the cropped edge cannot reach a new cell:
                    // the compressed tail exceeds every coverage regex span.
                    // Complete matches there were inspected in the last cell.
                    let mut coverage = if coverage_cropped {
                        "x".to_owned()
                    } else {
                        String::new()
                    };
                    coverage.push_str(&coverage_tail);
                    for c in value.chars().chain(std::iter::once(' ')) {
                        if report_stage::js_space(c) {
                            if !coverage.ends_with(' ') {
                                coverage.push(' ');
                            }
                        } else {
                            coverage.push(c);
                        }
                    }
                    let value = coverage.as_str();
                    if sequential.is_some() {
                        for (index, pattern) in
                            [&p.quarter_any, &p.quarter2_any].into_iter().enumerate()
                        {
                            for capture in pattern.captures_iter(value) {
                                if index == 0
                                    && value
                                        .as_bytes()
                                        .get(capture.get(0).unwrap().end())
                                        .is_some_and(u8::is_ascii_alphanumeric)
                                {
                                    continue;
                                }
                                match capture[1].parse::<u32>() {
                                    Ok(q @ 1..=4) => {
                                        sequential_quarters.insert(q);
                                    }
                                    _ => invalid_quarter = true,
                                }
                            }
                        }
                        for capture in p.roman_any.captures_iter(value) {
                            match capture[1].to_lowercase().as_str() {
                                "i" => {
                                    sequential_quarters.insert(1);
                                }
                                "ii" => {
                                    sequential_quarters.insert(2);
                                }
                                "iii" => {
                                    sequential_quarters.insert(3);
                                }
                                "iv" => {
                                    sequential_quarters.insert(4);
                                }
                                _ => invalid_quarter = true,
                            }
                        }
                    }
                    if quarters[0].is_none() {
                        quarters[0] = p
                            .quarter
                            .captures(value)
                            .map(|c| c[1].parse::<u32>().unwrap());
                    }
                    if quarters[1].is_none() {
                        quarters[1] = p
                            .quarter2
                            .captures(value)
                            .map(|c| c[1].parse::<u32>().unwrap());
                    }
                    if quarters[2].is_none() {
                        quarters[2] =
                            p.roman
                                .captures(value)
                                .map(|c| match c[1].to_lowercase().as_str() {
                                    "i" => 1,
                                    "ii" => 2,
                                    "iii" => 3,
                                    _ => 4,
                                });
                    }
                    annual |= p.annual.is_match(value);
                    let start = coverage.char_indices().rev().nth(63).map_or(0, |x| x.0);
                    coverage_cropped = start > 0;
                    coverage_tail = coverage[start..].to_owned();
                };
                inspect(&name);
                let mut st=self.db.prepare(&format!("SELECT c.row_num,c.kind,{} FROM cells c LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER) WHERE c.sheet=?1 AND c.row_num<=?2 ORDER BY c.row_num,c.col_num",resolved()))?;
                let mut rs = st.query(params![path, info.header])?;
                let mut numeric_years = BTreeSet::new();
                while let Some(r) = rs.next()? {
                    check_cancel(&cancel)?;
                    let kind: String = r.get(1)?;
                    let v: String = r.get::<_, Option<String>>(2)?.unwrap_or_default();
                    if ["s", "str", "inlineStr"].contains(&kind.as_str()) {
                        inspect(&v);
                    } else if (infer || sequential.is_some()) && r.get::<_, u32>(0)? < info.header {
                        if let Some(y) = number(Some(&v))
                            .filter(|n| n.fract() == 0. && *n >= 1900. && *n <= 9999.)
                        {
                            numeric_years.insert(y as i32);
                        }
                    }
                }
                drop(inspect);
                years.extend(numeric_years);
                if let Some(period) = sequential {
                    if years.iter().any(|year| *year != period.year) {
                        return Err(fail(
                            "sequential period year conflicts with worksheet headers",
                        ));
                    }
                    if invalid_quarter
                        || sequential_quarters.len() > 1
                        || sequential_quarters
                            .iter()
                            .any(|q| Some(*q) != period.quarter)
                    {
                        return Err(fail(
                            "sequential period quarter conflicts with worksheet headers",
                        ));
                    }
                }
                let year = sequential
                    .map(|p| p.year)
                    .or(cfg.year)
                    .or_else(|| confirmed_start.map(|d| d.year()))
                    .or_else(|| {
                        let explicit = cfg.kind != "day" && days.iter().all(|d| d.parts.year.is_some());
                        (years.len() == 1 || explicit).then(|| *years.first().unwrap())
                    })
                    .ok_or_else(|| fail("year is ambiguous; provide a confirmed year"))?;
                for (index, day) in days.iter_mut().enumerate() {
                    if day.parts.suffix.is_some_and(|s| year % 100 != s) {
                        return Err(fail("year conflicts with abbreviated date"));
                    }
                    let y = day.parts.year.unwrap_or(year);
                    if !(1900..=9999).contains(&y) {
                        return Err(fail("date year outside 1900..9999"));
                    }
                    day.date = NaiveDate::from_ymd_opt(y, day.parts.month, day.parts.day)
                        .ok_or_else(|| fail("invalid day in header"))?;
                    if index == 0 && confirmed_start.is_some_and(|d| d != day.date) {
                        return Err(fail("Дата первого периода не совпадает с датой в заголовке Excel."));
                    }
                    let end = shared_period_end(day.date, &cfg.kind)?;
                    if occupied.range(..=end).next_back().is_some_and(|(_, prior_end)| *prior_end >= day.date) {
                        return Err(fail("duplicate day or overlapping periods: периоды пересекаются. Проверьте период и даты в Excel."));
                    }
                    occupied.insert(day.date, end);
                    self.db
                        .execute(
                            "INSERT INTO wide_days VALUES(?1,?2,?3,?4,?5,?6)",
                            params![
                                name,
                                day.date.to_string(),
                                day.pair.q,
                                day.pair.a,
                                day.header_row,
                                day.header_col
                            ],
                        )
                        .map_err(|_| fail("duplicate day within or across selected sheets"))?;
                }
                let (begin, end) = if cfg.kind != "day" {
                    (days.iter().map(|d| d.date).min().unwrap(),
                     shared_period_end(days.iter().map(|d| d.date).max().unwrap(), &cfg.kind)?)
                } else if let Some(bounds) = sequential_bounds {
                    bounds
                } else if let Some(q) = quarters.into_iter().flatten().next() {
                    let month = (q - 1) * 3 + 1;
                    let begin = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
                    let end = if q == 4 {
                        NaiveDate::from_ymd_opt(year, 12, 31).unwrap()
                    } else {
                        NaiveDate::from_ymd_opt(year, month + 3, 1).unwrap() - Duration::days(1)
                    };
                    (begin, end)
                } else if annual || days.len() >= 364 {
                    (
                        NaiveDate::from_ymd_opt(year, 1, 1).unwrap(),
                        NaiveDate::from_ymd_opt(year, 12, 31).unwrap(),
                    )
                } else {
                    (
                        days.iter().map(|d| d.date).min().unwrap(),
                        days.iter().map(|d| d.date).max().unwrap(),
                    )
                };
                let mut first: Option<(String, String)> = None;
                for (index, day) in days.iter().enumerate() {
                    check_cancel(&cancel)?;
                    let mut st=self.db.prepare(&format!("SELECT {} FROM cells c INDEXED BY cells_by_column LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER) WHERE c.sheet=?1 AND c.col_num IN (?2,?3) AND c.row_num>?4",resolved()))?;
                    let mut rs = st.query(params![path, day.pair.q, day.pair.a, info.header])?;
                    let mut any = false;
                    while let Some(r) = rs.next()? {
                        check_cancel(&cancel)?;
                        if !blank(&r.get::<_, Option<String>>(0)?.unwrap_or_default()) {
                            any = true;
                            break;
                        }
                    }
                    if !any {
                        self.source_issue(
                            &name,
                            if cfg.kind == "day" { "missing-day" } else { "missing-period" },
                            if cfg.kind == "day" { "Daily columns are blank; no zero report invented" } else { "В столбцах периода нет значений" },
                            Some(day.date),
                            None,
                            None,
                            Some(cell(day.header_col, day.header_row)),
                            None,
                            (None, None),
                            (None, None),
                            day.date <= today,
                        )?;
                        continue;
                    }
                    let id = format!("wide-{}", summary.reports + 1);
                    let date = day.date.to_string();
                    let mapping = TripleMapping {
                        work_col: info.work,
                        count_col: day.pair.q,
                        amount_col: day.pair.a,
                        header_row: info.header,
                    };
                    let period_end = shared_period_end(day.date, &cfg.kind)?;
                    self.db.execute("INSERT INTO converted_reports VALUES(?1,?2,?3,?4,?5,?6,0,0,0,0,?7,?8,?9,?10)",
                        params![summary.reports+1,id,path,cfg.kind,date,period_end.to_string(),info.header,info.work,day.pair.q,day.pair.a])?;
                    let legacy = LegacyOptions {
                        period_dates: None,
                        sheet: path.clone(),
                        kind: cfg.kind.clone(),
                        start: date.clone(),
                        organizations: cfg.organizations.clone(),
                        mappings: None,
                    };
                    let (s, zero) = report_stage::convert_day(
                        &self.db,
                        &legacy,
                        &mapping,
                        &id,
                        &cancel,
                        |s| {
                            progress(ConversionSummary {
                                reports: summary.reports,
                                rows: summary.rows + s.rows,
                                issues: summary.issues + s.issues,
                            })
                        },
                    )?;
                    self.db.execute(
                        "INSERT INTO wide_provenance VALUES(?1,?2,?3,?4,NULL,?5,?6)",
                        params![
                            id,
                            name,
                            index + 1,
                            zero,
                            begin.to_string(),
                            end.to_string()
                        ],
                    )?;
                    if first.is_none() {
                        first = Some((id, date));
                    }
                    summary.reports += 1;
                    summary.rows += s.rows;
                    summary.issues += s.issues;
                    progress(summary);
                }
                if let Some(totals) = info.totals.or(total) {
                    self.source_totals(&name, path, &info, &days, totals, cfg, &cancel)?;
                }
                let mut date = begin;
                while cfg.kind == "day" && date <= end && date <= today {
                    check_cancel(&cancel)?;
                    if !days.iter().any(|d| d.date == date) {
                        self.source_issue(
                            &name,
                            "missing-day",
                            "Day is absent from source header",
                            Some(date),
                            Some(info.header),
                            None,
                            None,
                            None,
                            (None, None),
                            (None, None),
                            true,
                        )?;
                    }
                    let Some(next) = date.succ_opt() else {
                        break;
                    };
                    date = next;
                }
                let relevant: u64 = self.db.query_row(
                    "SELECT count(*) FROM wide_source_issues WHERE sheet=?1 AND relevant=1",
                    [&name],
                    |r| r.get(0),
                )?;
                if relevant > 0 {
                    let Some((id, date)) = first else {
                        return Err(fail("selected worksheet has diagnostics but no daily reports; explicitly deselect it"));
                    };
                    self.db.execute(
                        "UPDATE wide_source_issues SET report_id=?2 WHERE sheet=?1 AND relevant=1",
                        params![name, id],
                    )?;
                    self.db.execute(
                        "UPDATE wide_provenance SET source_control_date=?2 WHERE sheet_name=?1",
                        params![name, date],
                    )?;
                    let offset: u64 = self.db.query_row(
                        "SELECT coalesce(max(seq),0) FROM converted_issues WHERE report_id=?1",
                        [&id],
                        |r| r.get(0),
                    )?;
                    self.db.execute("INSERT INTO converted_issues SELECT ?1,?2+row_number() OVER(ORDER BY seq),kind,reason,source_row,organization,NULL,count_cell,amount_cell,expected_count,expected_amount,actual_count,actual_amount,'source-period',date,seq,NULL FROM wide_source_issues WHERE sheet=?3 AND relevant=1 ORDER BY seq",params![id,offset,name])?;
                    self.db.execute("UPDATE converted_reports SET has_mismatch=1 WHERE id=?1 AND EXISTS(SELECT 1 FROM wide_source_issues WHERE sheet=?2 AND relevant=1 AND kind='mismatch')",params![id,name])?;
                    summary.issues += relevant;
                }
            }
            if summary.reports == 0 {
                return Err(fail("В выбранных листах нет заполненных периодов."));
            }
            check_cancel(&cancel)?;
            self.db.execute(
                "INSERT INTO manifest VALUES('conversion_complete','daily-wide')",
                [],
            )?;
            self.db.execute_batch("COMMIT")?;
            Ok(summary)
        })();
        self.unhook();
        if result.is_err() && !self.db.is_autocommit() {
            let _ = self.db.execute_batch("ROLLBACK");
        }
        result
    }
    #[allow(clippy::too_many_arguments)]
    fn source_issue(
        &self,
        sheet: &str,
        kind: &str,
        reason: &str,
        date: Option<NaiveDate>,
        row: Option<u32>,
        org: Option<&str>,
        qcell: Option<String>,
        acell: Option<String>,
        expected: (Option<f64>, Option<f64>),
        actual: (Option<f64>, Option<f64>),
        relevant: bool,
    ) -> Result<()> {
        self.db.execute("INSERT INTO wide_source_issues VALUES(NULL,?1,NULL,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",params![sheet,kind,reason,date.map(|d|d.to_string()),row,org,qcell,acell,expected.0,expected.1,actual.0,actual.1,relevant])?;
        Ok(())
    }
    fn source_totals(
        &self,
        name: &str,
        path: &str,
        info: &Info,
        days: &[Day],
        totals: Pair,
        cfg: &WideOptions,
        cancel: &AtomicBool,
    ) -> Result<()> {
        let mut day_cols = [0u8; 2049];
        for d in days {
            day_cols[d.pair.q as usize] = 1;
            day_cols[d.pair.a as usize] = 2;
        }
        let mut st=self.db.prepare(&format!("SELECT c.row_num,c.col_num,{} FROM cells c LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER) WHERE c.sheet=?1 AND c.row_num>?2 ORDER BY c.row_num,c.col_num",resolved()))?;
        let mut rs = st.query(params![path, info.header])?;
        let mut row = 0;
        let mut work = String::new();
        let (mut q, mut a) = (String::new(), String::new());
        let mut sum = (0., 0.);
        let finish = |row, work: &str, q: &str, a: &str, sum: (f64, f64)| -> Result<()> {
            if row == 0 || blank(work) || (blank(q) && blank(a)) {
                return Ok(());
            }
            let expected = (number(Some(q)), number(Some(a)));
            if !sum.0.is_finite() || !sum.1.is_finite() {
                return Err(fail("nonfinite source-period total"));
            }
            if expected.0.is_none()
                || expected.1.is_none()
                || (sum.0 - expected.0.unwrap()).abs() > 0.001
                || (sum.1 - expected.1.unwrap()).abs() > 0.03
            {
                let org = cfg
                    .organizations
                    .iter()
                    .find(|o| normalize(o) == normalize(work))
                    .map(String::as_str);
                self.source_issue(
                    name,
                    if expected.0.is_none() || expected.1.is_none() {
                        "invalid"
                    } else {
                        "mismatch"
                    },
                    if cfg.kind == "day" { "Daily values do not match source-period controls" } else { "Значения по периодам не совпадают с контрольными итогами Excel" },
                    None,
                    Some(row),
                    org,
                    Some(cell(totals.q, row)),
                    Some(cell(totals.a, row)),
                    expected,
                    (Some(sum.0), Some(sum.1)),
                    true,
                )?;
            }
            Ok(())
        };
        while let Some(r) = rs.next()? {
            check_cancel(cancel)?;
            let n: u32 = r.get(0)?;
            if row != n {
                finish(row, &work, &q, &a, sum)?;
                work.clear();
                q.clear();
                a.clear();
                sum = (0., 0.);
                row = n;
            }
            let col: u32 = r.get(1)?;
            let value: String = r.get::<_, Option<String>>(2)?.unwrap_or_default();
            if col == info.work {
                work = value;
            } else if col == totals.q {
                q = value;
            } else if col == totals.a {
                a = value;
            } else if day_cols[col as usize] == 1 {
                sum.0 += number(Some(&value)).unwrap_or(0.);
            } else if day_cols[col as usize] == 2 {
                sum.1 += number(Some(&value)).unwrap_or(0.);
            }
        }
        finish(row, &work, &q, &a, sum)
    }
    pub fn report_provenance(&self, id: &str) -> Result<Option<ReportProvenance>> {
        self.conversion_ready()?;
        let exists: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='wide_provenance')",
            [],
            |r| r.get(0),
        )?;
        if !exists {
            return Ok(None);
        }
        Ok(self.db.query_row("SELECT p.sheet_name,p.source_block,p.explicit_zero,p.source_control_date,p.coverage_start,p.coverage_end,CASE WHEN r.kind='day' THEN 'daily-wide' ELSE 'period-pairs' END FROM wide_provenance p JOIN converted_reports r ON r.id=p.report_id WHERE p.report_id=?1",[id],|r|Ok(ReportProvenance{sheet_name:r.get(0)?,source_format:r.get(6)?,source_block:r.get(1)?,explicit_zero:r.get(2)?,source_control_date:r.get(3)?,coverage:Coverage{start:r.get(4)?,end:r.get(5)?}})).optional()?)
    }
    pub fn source_issue_page(&self, after: u64, limit: usize) -> Result<Vec<SourceIssue>> {
        page_limit(limit)?;
        self.conversion_ready()?;
        let wide:bool=self.db.query_row("SELECT EXISTS(SELECT 1 FROM manifest WHERE name='conversion_complete' AND value='daily-wide')",[],|r|r.get(0))?;
        if !wide {
            return Ok(Vec::new());
        }
        let mut st=self.db.prepare("SELECT seq,sheet,report_id,kind,reason,date,source_row,organization,count_cell,amount_cell,expected_count,expected_amount,actual_count,actual_amount,relevant FROM wide_source_issues WHERE seq>?1 ORDER BY seq LIMIT ?2")?;
        let rows = st.query_map(params![after, limit], read_source_issue)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
    pub fn source_issues(
        &self,
        sheet_name: &str,
        after: u64,
        limit: usize,
    ) -> Result<Vec<SourceIssue>> {
        page_limit(limit)?;
        self.conversion_ready()?;
        let exists:bool=self.db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='wide_source_issues') AND EXISTS(SELECT 1 FROM manifest WHERE name='conversion_complete' AND value='daily-wide')",[],|r|r.get(0))?;
        if !exists {
            return Ok(Vec::new());
        }
        let mut st=self.db.prepare("SELECT seq,sheet,report_id,kind,reason,date,source_row,organization,count_cell,amount_cell,expected_count,expected_amount,actual_count,actual_amount,relevant FROM wide_source_issues WHERE sheet=?1 AND seq>?2 ORDER BY seq LIMIT ?3")?;
        let rows = st.query_map(params![sheet_name, after, limit], read_source_issue)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}
fn read_source_issue(r: &rusqlite::Row<'_>) -> rusqlite::Result<SourceIssue> {
    Ok(SourceIssue {
        sequence: r.get(0)?,
        sheet: r.get(1)?,
        report_id: r.get(2)?,
        scope: "source-period".into(),
        kind: r.get(3)?,
        reason: r.get(4)?,
        date: r.get(5)?,
        source_row: r.get(6)?,
        organization: r.get(7)?,
        count_cell: r.get(8)?,
        amount_cell: r.get(9)?,
        expected_count: r.get(10)?,
        expected_amount: r.get(11)?,
        actual_count: r.get(12)?,
        actual_amount: r.get(13)?,
        relevant: r.get(14)?,
    })
}

fn parse_start(value: &str) -> Result<NaiveDate> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| fail("Укажите корректную дату первого периода."))?;
    if !(1900..=9999).contains(&date.year()) || date.to_string() != value {
        return Err(fail("Укажите корректную дату первого периода."));
    }
    Ok(date)
}

pub(crate) fn shared_period_end(start: NaiveDate, kind: &str) -> Result<NaiveDate> {
    let end = match kind {
        "day" => Some(start),
        "week" => start.checked_add_signed(Duration::days(6)),
        "month" if start.day() == 1 => start.checked_add_months(Months::new(1)).and_then(|d| d.pred_opt()),
        "month" => return Err(fail("Месячный период должен начинаться первого числа.")),
        _ => return Err(fail("Выберите день, неделю или месяц.")),
    };
    end.filter(|d| d.year() <= 9999).ok_or_else(|| fail("Дата периода вне диапазона."))
}

fn validate(cfg: &WideOptions) -> Result<()> {
    if !matches!(cfg.kind.as_str(), "day" | "week" | "month")
        || (!cfg.sequential_periods.is_empty() && (cfg.kind != "day" || cfg.start.is_some())) {
        return Err(fail("Последовательные колонки за квартал или год доступны для дневных отчётов."));
    }
    if let Some(raw) = &cfg.start {
        shared_period_end(parse_start(raw)?, &cfg.kind)?;
    }
    if cfg.year.is_some_and(|y| !(1900..=9999).contains(&y))
        || cfg.sheets.len() > 12
        || cfg.mappings.len() > 12
        || cfg.sequential_periods.len() > 12
        || cfg.sequential_periods.values().any(|p| {
            !(1900..=9999).contains(&p.year)
                || p.quarter.is_some_and(|q| !(1..=4).contains(&q))
                || cfg.year.is_some_and(|year| year != p.year)
        })
        || cfg.sheets.iter().collect::<BTreeSet<_>>().len() != cfg.sheets.len()
        || cfg
            .sheets
            .iter()
            .chain(cfg.mappings.keys())
            .chain(cfg.sequential_periods.keys())
            .any(|n| n.is_empty() || n.encode_utf16().count() > 240)
        || cfg
            .organizations
            .iter()
            .any(|n| blank(n) || n.encode_utf16().count() > 240)
        || cfg
            .organizations
            .iter()
            .map(|n| normalize(n))
            .collect::<BTreeSet<_>>()
            .len()
            != 4
    {
        return Err(fail("invalid or oversized daily options"));
    }
    Ok(())
}
fn period_bounds(period: &WideSequentialPeriod) -> Result<(NaiveDate, NaiveDate)> {
    let month = period.quarter.map_or(1, |q| (q - 1) * 3 + 1);
    let begin = NaiveDate::from_ymd_opt(period.year, month, 1)
        .ok_or_else(|| fail("invalid sequential period"))?;
    let end = if period.quarter.is_none() || period.quarter == Some(4) {
        NaiveDate::from_ymd_opt(period.year, 12, 31).unwrap()
    } else {
        NaiveDate::from_ymd_opt(period.year, month + 3, 1).unwrap() - Duration::days(1)
    };
    Ok((begin, end))
}
fn infer_years(value: &str, years: &mut BTreeSet<i32>) {
    find_years(value, years, 2199);
}
fn find_years(value: &str, years: &mut BTreeSet<i32>, maximum: i32) {
    let bytes = value.as_bytes();
    for i in 0..bytes.len().saturating_sub(3) {
        let b = &bytes[i..i + 4];
        if b.iter().all(u8::is_ascii_digit)
            && (i == 0 || !bytes[i - 1].is_ascii_digit())
            && (i + 4 == bytes.len() || !bytes[i + 4].is_ascii_digit())
        {
            let year = (b[0] - b'0') as i32 * 1000
                + (b[1] - b'0') as i32 * 100
                + (b[2] - b'0') as i32 * 10
                + (b[3] - b'0') as i32;
            if (1900..=maximum).contains(&year) {
                years.insert(year);
            }
        }
    }
}
fn relationship_target(target: &str) -> Result<String> {
    if target.len() > 2048
        || target.is_empty()
        || target.contains(['\\', ':', '?', '#', '\0'])
        || target.starts_with("//")
    {
        return Err(fail("invalid worksheet relationship target"));
    }
    let mut parts = if target.starts_with('/') {
        Vec::new()
    } else {
        vec!["xl"]
    };
    for part in target.trim_start_matches('/').split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(fail("worksheet relationship escapes archive"));
                }
            }
            x => parts.push(x),
        }
    }
    if parts.is_empty() {
        return Err(fail("empty worksheet relationship target"));
    }
    Ok(parts.join("/"))
}
pub(crate) fn excel_serial_date(n: f64, date1904: bool) -> Result<NaiveDate> {
    let n = n.floor() as i64;
    if !date1904 && n == 60 {
        return Err(fail("nonexistent Excel date 1900-02-29"));
    }
    let base = if date1904 {
        NaiveDate::from_ymd_opt(1904, 1, 1)
    } else if n < 60 {
        NaiveDate::from_ymd_opt(1899, 12, 31)
    } else {
        NaiveDate::from_ymd_opt(1899, 12, 30)
    }
    .unwrap();
    base
        .checked_add_signed(Duration::days(n))
        .ok_or_else(|| fail("Excel date out of range"))
}
pub(crate) fn typed_excel_date(value: &str) -> Option<NaiveDate> {
    static ISO: LazyLock<Regex> = LazyLock::new(|| Regex::new(
        r"^([0-9]{4}-[0-9]{2}-[0-9]{2})(?:T(?:[01][0-9]|2[0-3]):[0-5][0-9](?::[0-5][0-9](?:\.[0-9]+)?)?(?:Z|[+-](?:[01][0-9]|2[0-3]):[0-5][0-9])?)?$"
    ).unwrap());
    // Preserve the calendar date written in the workbook without converting the offset to UTC.
    NaiveDate::parse_from_str(&ISO.captures(value)?[1], "%Y-%m-%d").ok()
}
fn period_header_date(
    value: &str, cell_kind: &str, styled: bool, date1904: bool,
    period_kind: &str, patterns: &Patterns,
) -> Result<Option<Parts>> {
    if let Some(parts) = date_parts(value, cell_kind, styled, date1904, patterns)? {
        return Ok(Some(parts));
    }
    if period_kind == "detect-day" || !matches!(cell_kind, "s" | "str" | "inlineStr") { return Ok(None); }
    let text = value.trim();
    if text.len() == 7 && text.as_bytes()[4] == b'-' && text.bytes().enumerate().all(|(i,c)| i==4 || c.is_ascii_digit()) {
            if period_kind != "month" { return Err(fail("В заголовке указан месяц. Проверьте период отчёта.")); }
            let first = parse_start(&format!("{text}-01"))?;
            return Ok(Some(Parts { year: Some(first.year()), suffix: None, month: first.month(), day: 1 }));
    }
    if period_kind == "month" {
        if let Some(parts) = date_parts(&format!("1 {text}"), "inlineStr", false, false, patterns)? {
            return Ok(Some(parts));
        }
    }
    static RANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(
        r"(?i)^(?:(?:отч[её]тный\s+)?период\s*[:=]?\s*)?(?:с\s+)?(\d{4}-\d{2}-\d{2}|\d{1,2}[./]\d{1,2}[./]\d{4})\s*(?:по|[–—-])\s*(\d{4}-\d{2}-\d{2}|\d{1,2}[./]\d{1,2}[./]\d{4})$"
    ).unwrap());
    if let Some(range) = RANGE.captures(text) {
        let parse = |raw: &str| -> Result<NaiveDate> {
            let fmt = if raw.contains('.') { "%d.%m.%Y" } else if raw.contains('/') { "%d/%m/%Y" } else { "%Y-%m-%d" };
            NaiveDate::parse_from_str(raw, fmt).map_err(|_| fail("В заголовке Excel указана некорректная дата."))
        };
        let first = parse(&range[1])?;
        if shared_period_end(first, period_kind)? != parse(&range[2])? {
            return Err(fail("Диапазон дат в заголовке Excel не соответствует выбранному периоду."));
        }
        return Ok(Some(Parts { year: Some(first.year()), suffix: None, month: first.month(), day: first.day() }));
    }
    Ok(None)
}

fn date_parts(
    value: &str,
    kind: &str,
    styled: bool,
    date1904: bool,
    p: &Patterns,
) -> Result<Option<Parts>> {
    if kind == "d" {
        return Ok(typed_excel_date(value).map(|d| Parts {
            year: Some(d.year()), suffix: None, month: d.month(), day: d.day(),
        }));
    }
    if !["s", "str", "inlineStr"].contains(&kind) {
        if let Ok(n) = value.parse::<f64>() {
            if n >= if date1904 { 0. } else { 1. } && n < 2958466. && (styled || n >= 20000.) {
                let d = excel_serial_date(n,date1904)?;
                return Ok(Some(Parts {
                    year: Some(d.year()),
                    suffix: None,
                    month: d.month(),
                    day: d.day(),
                }));
            }
        }
    }
    let text = value.trim_matches(report_stage::js_space).to_lowercase();
    let text = p.strip_prefix.replace(&text, "");
    let text = p.strip_suffix.replace(&text, "");
    let suffix = text.strip_suffix("г.").or_else(|| text.strip_suffix('г'));
    let text = suffix
        .filter(|s| s.chars().last().is_some_and(report_stage::js_space))
        .map(|s| s.trim_end_matches(report_stage::js_space))
        .unwrap_or(&text);
    if let Some(c) = p.iso.captures(text) {
        return Ok(Some(Parts {
            year: Some(c[1].parse()?),
            suffix: None,
            month: c[2].parse()?,
            day: c[3].parse()?,
        }));
    }
    if let Some(c) = p.numeric.captures(text) {
        let y = c.get(3).map(|s| s.as_str());
        return Ok(Some(Parts {
            year: y.filter(|s| s.len() == 4).map(str::parse).transpose()?,
            suffix: y.filter(|s| s.len() == 2).map(str::parse).transpose()?,
            month: c[2].parse()?,
            day: c[1].parse()?,
        }));
    }
    if let Some(c) = p.text.captures(text) {
        let month = [
            "январ",
            "феврал",
            "март",
            "апрел",
            "ма",
            "июн",
            "июл",
            "август",
            "сентябр",
            "октябр",
            "ноябр",
            "декабр",
        ]
        .iter()
        .position(|m| {
            if *m == "ма" {
                c[2].starts_with("май") || c[2].starts_with("мая")
            } else {
                c[2].starts_with(m)
            }
        });
        if let Some(m) = month {
            return Ok(Some(Parts {
                year: c.get(3).map(|y| y.as_str().parse()).transpose()?,
                suffix: None,
                month: m as u32 + 1,
                day: c[1].parse()?,
            }));
        }
    }
    Ok(None)
}
#[cfg(test)]
mod date_tests {
    use super::*;
    #[test]
    fn date_text_matches_browser_string_types_and_whitespace() {
        let p = Patterns::new();
        assert_eq!(
            date_parts("2024-02-29", "d", false, false, &p)
                .unwrap()
                .unwrap()
                .day,
            29
        );
        assert_eq!(
            date_parts("1\u{feff}февраля 2024", "inlineStr", false, false, &p)
                .unwrap()
                .unwrap()
                .month,
            2
        );
        assert!(
            date_parts("1\u{0085}февраля 2024", "inlineStr", false, false, &p)
                .unwrap()
                .is_none()
        );
        assert!(date_parts("1 малина 2024", "inlineStr", false, false, &p)
            .unwrap()
            .is_none());
        assert!(date_parts("٢٠٢٤-01-01", "inlineStr", false, false, &p)
            .unwrap()
            .is_none());
    }
    #[test]
    fn typed_header_keeps_the_calendar_date_and_ignores_invalid_times() {
        let patterns = Patterns::new();
        let d = date_parts("2024-02-29T23:45:12.125-05:00", "d", false, false, &patterns)
            .unwrap().unwrap();
        assert_eq!((d.year, d.month, d.day), (Some(2024), 2, 29));
        assert!(date_parts("2024-02-29T25:00:00", "d", false, false, &patterns).unwrap().is_none());
    }
    #[test]
    fn date_serial_epoch_is_explicit_and_fake_leap_day_rejected() {
        let p = Patterns::new();
        let d = date_parts("0", "n", true, true, &p).unwrap().unwrap();
        assert_eq!((d.year, d.month, d.day), (Some(1904), 1, 1));
        let d = date_parts("1", "n", true, true, &p).unwrap().unwrap();
        assert_eq!((d.month, d.day), (1, 2));
        assert!(date_parts("60", "n", true, false, &p).is_err());
    }
}
