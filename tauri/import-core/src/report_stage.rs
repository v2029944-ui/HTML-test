//! Sequential conversion of ordinary triples into disk-backed reports.
//! Financial rows and diagnostics are never collected into a report-sized Vec.
use crate::{check_cancel, fail, Result, Stage};
use chrono::{Datelike, Duration, Months, NaiveDate};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::{collections::HashSet, sync::atomic::AtomicBool};
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TripleMapping {
    pub header_row: u32,
    pub work_col: u32,
    pub count_col: u32,
    pub amount_col: u32,
}
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyOptions {
    pub sheet: String,
    pub kind: String,
    pub start: String,
    pub organizations: [String; 4],
    pub mappings: Option<Vec<TripleMapping>>,
    /// Full set-ordered list of explicitly chosen dates. Omission retains the
    /// sequential dates derived from start; partial lists are rejected.
    #[serde(default)]
    pub period_dates: Option<Vec<String>>,
}
#[derive(Clone, Copy, Default, Debug)]
pub struct ConversionSummary {
    pub reports: u64,
    pub rows: u64,
    pub issues: u64,
}
#[derive(Debug, Serialize)]
pub struct ReportInfo {
    pub ordinal: u64,
    pub sheet: String,
    pub mapping: TripleMapping,
    pub id: String,
    pub start: String,
    pub end: String,
    pub kind: String,
    pub row_count: u64,
    pub count: f64,
    pub amount: f64,
    pub has_mismatch: bool,
}
#[derive(Debug, Serialize)]
pub struct ReportRow {
    pub row_index: u64,
    pub organization: String,
    pub work: String,
    pub count: f64,
    pub amount: f64,
    pub source_row: u32,
    pub work_cell: String,
    pub count_cell: String,
    pub amount_cell: String,
}
#[derive(Debug)]
pub struct ReportIssue {
    pub skipped: Option<bool>,
    pub scope: Option<String>,
    pub date: Option<String>,
    pub source_issue_sequence: Option<u64>,
    pub sequence: u64,
    pub kind: String,
    pub reason: String,
    pub source_row: Option<u32>,
    pub organization: Option<String>,
    pub work_cell: Option<String>,
    pub count_cell: Option<String>,
    pub amount_cell: Option<String>,
    pub expected_count: Option<f64>,
    pub expected_amount: Option<f64>,
    pub actual_count: Option<f64>,
    pub actual_amount: Option<f64>,
}
/// Compatibility aliases are additive: retain every native provenance field.
impl Serialize for ReportIssue {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        #[derive(Serialize)]
        struct Numbers {
            count: Option<f64>,
            amount: Option<f64>,
        }
        let cells: Vec<_> = [
            self.work_cell.as_deref(),
            self.count_cell.as_deref(),
            self.amount_cell.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect();
        let comparison = self
            .actual_count
            .zip(self.actual_amount)
            .filter(|(c, a)| c.is_finite() && a.is_finite())
            .filter(|_| {
                [self.expected_count, self.expected_amount]
                    .into_iter()
                    .flatten()
                    .all(f64::is_finite)
            });
        let (expected, actual) = match comparison {
            Some((count, amount)) => (
                Some(Numbers {
                    count: self.expected_count,
                    amount: self.expected_amount,
                }),
                Some(Numbers {
                    count: Some(count),
                    amount: Some(amount),
                }),
            ),
            None => (None, None),
        };
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("type", &self.kind)?;
        map.serialize_entry("row", &self.source_row)?;
        map.serialize_entry("cells", &cells)?;
        map.serialize_entry("expected", &expected)?;
        map.serialize_entry("actual", &actual)?;
        if let Some(scope) = &self.scope {
            map.serialize_entry("scope", scope)?;
        }
        if let Some(date) = &self.date {
            map.serialize_entry("date", date)?;
        }
        if let Some(sequence) = self.source_issue_sequence {
            map.serialize_entry("source_issue_sequence", &sequence)?;
        }
        if self.skipped == Some(true) {
            map.serialize_entry("skipped", &true)?;
        }
        map.serialize_entry("sequence", &self.sequence)?;
        map.serialize_entry("kind", &self.kind)?;
        map.serialize_entry("reason", &self.reason)?;
        map.serialize_entry("source_row", &self.source_row)?;
        map.serialize_entry("organization", &self.organization)?;
        map.serialize_entry("work_cell", &self.work_cell)?;
        map.serialize_entry("count_cell", &self.count_cell)?;
        map.serialize_entry("amount_cell", &self.amount_cell)?;
        map.serialize_entry("expected_count", &self.expected_count)?;
        map.serialize_entry("expected_amount", &self.expected_amount)?;
        map.serialize_entry("actual_count", &self.actual_count)?;
        map.serialize_entry("actual_amount", &self.actual_amount)?;
        map.end()
    }
}
#[derive(Debug)]
pub struct ReportTotal {
    pub sequence: u64,
    pub kind: String,
    pub organization: Option<String>,
    pub source_row: u32,
    pub count: Option<f64>,
    pub amount: Option<f64>,
    pub work_cell: String,
    pub count_cell: String,
    pub amount_cell: String,
}
impl Serialize for ReportTotal {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(12))?;
        map.serialize_entry("type", &self.kind)?;
        map.serialize_entry("row", &self.source_row)?;
        map.serialize_entry("name", &self.organization)?;
        map.serialize_entry("sequence", &self.sequence)?;
        map.serialize_entry("kind", &self.kind)?;
        map.serialize_entry("source_row", &self.source_row)?;
        map.serialize_entry("organization", &self.organization)?;
        map.serialize_entry("count", &self.count)?;
        map.serialize_entry("amount", &self.amount)?;
        map.serialize_entry("work_cell", &self.work_cell)?;
        map.serialize_entry("count_cell", &self.count_cell)?;
        map.serialize_entry("amount_cell", &self.amount_cell)?;
        map.end()
    }
}
#[derive(Debug, Serialize)]
pub struct ReportOrganization {
    pub organization: String,
    pub count: f64,
    pub amount: f64,
}
pub(crate) const SCHEMA:&str="CREATE TABLE IF NOT EXISTS converted_reports(ordinal INTEGER PRIMARY KEY,id TEXT UNIQUE,sheet TEXT,kind TEXT,start TEXT,end TEXT,row_count INTEGER,count REAL,amount REAL,has_mismatch INTEGER,header_row INTEGER,work_col INTEGER,count_col INTEGER,amount_col INTEGER);
 CREATE TABLE IF NOT EXISTS converted_rows(report_id TEXT,row_index INTEGER,organization TEXT,work TEXT,count REAL,amount REAL,source_row INTEGER,work_cell TEXT,count_cell TEXT,amount_cell TEXT,PRIMARY KEY(report_id,row_index)) WITHOUT ROWID;
 CREATE TABLE IF NOT EXISTS converted_issues(report_id TEXT,seq INTEGER,kind TEXT,reason TEXT,source_row INTEGER,organization TEXT,work_cell TEXT,count_cell TEXT,amount_cell TEXT,expected_count REAL,expected_amount REAL,actual_count REAL,actual_amount REAL,scope TEXT,date TEXT,source_issue_seq INTEGER,skipped INTEGER CHECK(skipped IS NULL OR skipped IN(0,1)),PRIMARY KEY(report_id,seq)) WITHOUT ROWID;
 CREATE TABLE IF NOT EXISTS converted_totals(report_id TEXT,seq INTEGER,kind TEXT,organization TEXT,source_row INTEGER,count REAL,amount REAL,PRIMARY KEY(report_id,seq)) WITHOUT ROWID;
 CREATE TABLE IF NOT EXISTS converted_organizations(report_id TEXT,seq INTEGER,organization TEXT,count REAL,amount REAL,PRIMARY KEY(report_id,seq)) WITHOUT ROWID;";
pub(crate) fn js_space(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'
            | '\u{000a}'
            | '\u{000b}'
            | '\u{000c}'
            | '\u{000d}'
            | '\u{0020}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    ) || ('\u{2000}'..='\u{200a}').contains(&c)
}
pub(crate) fn normalize(s: &str) -> String {
    s.trim_end_matches(['.', ',', ';', ':'])
        .split(js_space)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
pub(crate) fn number(s: Option<&str>) -> Option<f64> {
    let s = s?;
    if s.trim_matches(js_space).is_empty() {
        return None;
    }
    let s = s
        .chars()
        .filter(|c| !js_space(*c))
        .collect::<String>()
        .replacen(',', ".", 1);
    let n = if s.starts_with("0x")
        || s.starts_with("0X")
        || s.starts_with("0b")
        || s.starts_with("0B")
        || s.starts_with("0o")
        || s.starts_with("0O")
    {
        let base = match s.as_bytes()[1].to_ascii_lowercase() {
            b'x' => 16,
            b'b' => 2,
            _ => 8,
        };
        if s.len() == 2 {
            return None;
        }
        let mut n = 0f64;
        for c in s[2..].chars() {
            n = n * base as f64 + c.to_digit(base)? as f64;
        }
        n
    } else {
        s.parse().ok()?
    };
    n.is_finite().then_some(n)
}
fn round(n: f64, places: i32) -> f64 {
    let p = 10f64.powi(places);
    // Preserve the legacy EPSILON addition and scaling. Math.round ties go
    // toward +infinity; adding 0.5 first would alter large odd integers.
    let scaled = (n + f64::EPSILON) * p;
    if !scaled.is_finite() || scaled.fract() == 0.0 {
        return scaled / p;
    }
    let floor = scaled.floor();
    let rounded = if scaled - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    };
    if rounded == 0.0 {
        0.0f64.copysign(scaled) / p
    } else {
        rounded / p
    }
}
pub(crate) fn cell(col: u32, row: u32) -> String {
    let mut c = col;
    let mut bytes = Vec::with_capacity(3);
    while c > 0 {
        c -= 1;
        bytes.push(b'A' + (c % 26) as u8);
        c /= 26;
    }
    bytes.reverse();
    format!("{}{row}", String::from_utf8(bytes).unwrap())
}
pub(crate) fn page_limit(n: usize) -> Result<()> {
    if (1..=100).contains(&n) {
        Ok(())
    } else {
        Err(fail("report page limit is 1..100"))
    }
}
fn legacy_periods(
    cfg: &LegacyOptions,
    start: NaiveDate,
    count: usize,
) -> Result<Vec<(NaiveDate, NaiveDate)>> {
    if count > 684
        || cfg
            .period_dates
            .as_ref()
            .is_some_and(|dates| dates.len() != count)
    {
        return Err(fail(
            "Число дат должно совпадать с числом наборов столбцов.",
        ));
    }
    let mut periods: Vec<(NaiveDate, NaiveDate)> = Vec::with_capacity(count);
    for i in 0..count {
        let first = if let Some(dates) = &cfg.period_dates {
            let raw = &dates[i];
            let parsed = NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| {
                fail(&format!(
                    "Набор {}: укажите корректную дату периода.",
                    i + 1
                ))
            })?;
            if raw.len() != 10 || parsed.to_string() != *raw {
                return Err(fail(&format!(
                    "Набор {}: укажите дату в формате YYYY-MM-DD.",
                    i + 1
                )));
            }
            parsed
        } else {
            match cfg.kind.as_str() {
                "month" => start.checked_add_months(Months::new(i as u32)),
                "week" => start.checked_add_signed(Duration::days(i as i64 * 7)),
                _ => start.checked_add_signed(Duration::days(i as i64)),
            }
            .ok_or_else(|| fail("Дата периода вне диапазона."))?
        };
        if cfg.kind == "month" && first.day() != 1 {
            return Err(fail(&format!(
                "Набор {}: месячный период должен начинаться первого числа.",
                i + 1
            )));
        }
        let end = match cfg.kind.as_str() {
            "week" => first.checked_add_signed(Duration::days(6)),
            "month" => first
                .checked_add_months(Months::new(1))
                .and_then(|date| date.checked_sub_signed(Duration::days(1))),
            _ => Some(first),
        }
        .ok_or_else(|| fail("Дата периода вне диапазона."))?;
        if first.year() > 9999 || end.year() > 9999 {
            return Err(fail("Дата периода вне диапазона."));
        }
        if periods.iter().any(|(a, b)| *a <= end && *b >= first) {
            return Err(fail(&format!(
                "Даты наборов пересекаются. Проверьте период {}.",
                i + 1
            )));
        }
        periods.push((first, end));
    }
    Ok(periods)
}

impl Stage<'_> {
    pub fn convert_legacy(
        &mut self,
        cfg: &LegacyOptions,
        cancel: &AtomicBool,
        mut progress: impl FnMut(ConversionSummary),
    ) -> Result<ConversionSummary> {
        check_cancel(cancel)?;
        if cfg.start.len() != 10
            || cfg.sheet.len() > 2048
            || cfg
                .organizations
                .iter()
                .any(|s| s.trim_matches(js_space).is_empty() || s.encode_utf16().count() > 240)
            || cfg.mappings.as_ref().is_some_and(|m| m.len() > 684)
            || cfg
                .period_dates
                .as_ref()
                .is_some_and(|dates| dates.len() > 684 || dates.iter().any(|date| date.len() != 10))
        {
            return Err(fail("Некорректные или слишком большие параметры импорта."));
        }
        let start = NaiveDate::parse_from_str(&cfg.start, "%Y-%m-%d")
            .map_err(|_| fail("Укажите достоверную дату первого периода."))?;
        if cfg.start.len() != 10
            || !["day", "week", "month"].contains(&cfg.kind.as_str())
            || cfg.kind == "month" && start.day() != 1
        {
            return Err(fail("Некорректный тип или начало периода."));
        }
        let orgs = cfg
            .organizations
            .iter()
            .map(|s| normalize(s))
            .collect::<Vec<_>>();
        if cfg
            .organizations
            .iter()
            .any(|s| s.trim_matches(js_space).is_empty() || s.encode_utf16().count() > 240)
            || orgs.iter().collect::<HashSet<_>>().len() != 4
        {
            return Err(fail("Требуются четыре разных подразделения."));
        }
        let mappings = if let Some(m) = &cfg.mappings {
            m.clone()
        } else {
            detect(&self.db, &cfg.sheet)?
        };
        if mappings.is_empty() || mappings.len() > 684 {
            return Err(fail(
                "Не найдены тройки столбцов «Работа / Количество / Сумма».",
            ));
        }
        for m in &mappings {
            if m.header_row == 0
                || m.header_row > self.limits.rows
                || [m.work_col, m.count_col, m.amount_col]
                    .iter()
                    .any(|c| *c == 0 || *c > self.limits.columns)
                || m.work_col == m.count_col
                || m.work_col == m.amount_col
                || m.count_col == m.amount_col
            {
                return Err(fail("Некорректное сопоставление столбцов."));
            }
        }
        let periods = legacy_periods(cfg, start, mappings.len())?;
        self.db.execute_batch(SCHEMA)?;
        self.db.execute_batch("BEGIN IMMEDIATE;DELETE FROM converted_reports;DELETE FROM converted_rows;DELETE FROM converted_issues;DELETE FROM converted_totals;DELETE FROM converted_organizations;DELETE FROM manifest WHERE name='conversion_complete';")?;
        let result = (|| {
            let mut summary = ConversionSummary::default();
            for (i, mapping) in mappings.iter().enumerate() {
                check_cancel(cancel)?;
                let (first, end) = periods[i];
                let id = format!("legacy-{}", i + 1);
                self.db.execute(
                    "INSERT INTO converted_reports VALUES(?1,?2,?3,?4,?5,?6,0,0,0,0,?7,?8,?9,?10)",
                    params![
                        i + 1,
                        id,
                        cfg.sheet,
                        cfg.kind,
                        first.to_string(),
                        end.to_string(),
                        mapping.header_row,
                        mapping.work_col,
                        mapping.count_col,
                        mapping.amount_col
                    ],
                )?;
                let mut context = Context {
                    db: &self.db,
                    cfg,
                    m: mapping,
                    id: &id,
                    active: None,
                    by_org: [(0., 0.); 4],
                    present: [0; 4],
                    present_count: 0,
                    rows: 0,
                    issues: 0,
                    totals: 0,
                    sum: (0., 0.),
                    mismatch: false,
                    overall: false,
                    daily: false,
                };
                let mut stmt=self.db.prepare("SELECT c.row_num,c.col_num,CASE WHEN c.kind='s' THEN s.value WHEN c.kind='b' THEN CASE c.value WHEN '1' THEN 'true' ELSE 'false' END ELSE c.value END FROM cells c LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER) WHERE c.sheet=?1 AND c.row_num>?2 AND c.col_num IN (?3,?4,?5) ORDER BY c.row_num,c.col_num")?;
                let mut rows = stmt.query(params![
                    cfg.sheet,
                    mapping.header_row,
                    mapping.work_col,
                    mapping.count_col,
                    mapping.amount_col
                ])?;
                let mut row_num = 0;
                let mut values: [Option<String>; 3] = [None, None, None];
                let mut done = false;
                while let Some(row) = rows.next()? {
                    check_cancel(cancel)?;
                    let current: u32 = row.get(0)?;
                    if row_num != 0 && current != row_num {
                        if context.process(row_num, &values)? {
                            done = true;
                            break;
                        }
                        values = [None, None, None];
                        if row_num % 2048 == 0 {
                            progress(ConversionSummary {
                                reports: summary.reports,
                                rows: summary.rows + context.rows,
                                issues: summary.issues + context.issues,
                            });
                        }
                    }
                    row_num = current;
                    let col: u32 = row.get(1)?;
                    let slot = if col == mapping.work_col {
                        0
                    } else if col == mapping.count_col {
                        1
                    } else {
                        2
                    };
                    values[slot] = row.get(2)?;
                }
                if !done && row_num != 0 {
                    context.process(row_num, &values)?;
                }
                if !context.overall {
                    context.check_block()?;
                }
                if context.rows == 0 {
                    context.issue(
                        "empty",
                        "Нет распознанных работ",
                        None,
                        None,
                        (None, None, None),
                        None,
                        None,
                        false,
                    )?;
                }
                for (j, (q, a)) in context.by_org.iter().enumerate() {
                    if context.present[j] != 0 {
                        context.db.execute(
                            "INSERT INTO converted_organizations VALUES(?1,?2,?3,?4,?5)",
                            params![id, context.present[j], cfg.organizations[j], q, a],
                        )?;
                    }
                }
                if !round(context.sum.0, 6).is_finite() || !round(context.sum.1, 2).is_finite() {
                    return Err(fail("Итог не помещается в числовой диапазон."));
                }
                context.db.execute("UPDATE converted_reports SET row_count=?2,count=?3,amount=?4,has_mismatch=?5 WHERE id=?1",params![id,context.rows,round(context.sum.0,6),round(context.sum.1,2),context.mismatch])?;
                summary.reports += 1;
                summary.rows += context.rows;
                summary.issues += context.issues;
                progress(summary);
            }
            check_cancel(cancel)?;
            self.db.execute(
                "INSERT INTO manifest VALUES('conversion_complete','legacy')",
                [],
            )?;
            self.db.execute_batch("COMMIT")?;
            Ok(summary)
        })();
        if result.is_err() {
            let _ = self.db.execute_batch("ROLLBACK");
        }
        result
    }
    pub(crate) fn conversion_ready(&self) -> Result<()> {
        let ready: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM manifest WHERE name='conversion_complete'",
            [],
            |r| r.get(0),
        )?;
        if ready == 1 {
            Ok(())
        } else {
            Err(fail("Report conversion is not complete"))
        }
    }
    pub fn report_page(&self, after: u64, limit: usize) -> Result<Vec<ReportInfo>> {
        page_limit(limit)?;
        self.conversion_ready()?;
        let mut s=self.db.prepare("SELECT id,start,end,kind,row_count,count,amount,has_mismatch,ordinal,sheet,header_row,work_col,count_col,amount_col FROM converted_reports WHERE ordinal>?1 ORDER BY ordinal LIMIT ?2")?;
        let r = s.query_map(params![after, limit], |r| {
            Ok(ReportInfo {
                ordinal: r.get(8)?,
                sheet: r.get(9)?,
                mapping: TripleMapping {
                    header_row: r.get(10)?,
                    work_col: r.get(11)?,
                    count_col: r.get(12)?,
                    amount_col: r.get(13)?,
                },
                id: r.get(0)?,
                start: r.get(1)?,
                end: r.get(2)?,
                kind: r.get(3)?,
                row_count: r.get(4)?,
                count: r.get(5)?,
                amount: r.get(6)?,
                has_mismatch: r.get(7)?,
            })
        })?;
        Ok(r.collect::<std::result::Result<_, _>>()?)
    }
    /// Exclusive one-based total sequence; at most 100 controls per page.
    pub fn report_totals(&self, id: &str, after: u64, limit: usize) -> Result<Vec<ReportTotal>> {
        page_limit(limit)?;
        self.conversion_ready()?;
        let mut stmt = self.db.prepare("SELECT t.seq,t.kind,t.organization,t.source_row,t.count,t.amount,r.work_col,r.count_col,r.amount_col FROM converted_totals t JOIN converted_reports r ON r.id=t.report_id WHERE t.report_id=?1 AND t.seq>?2 ORDER BY t.seq LIMIT ?3")?;
        let rows = stmt.query_map(params![id, after, limit], |r| {
            let source_row = r.get(3)?;
            Ok(ReportTotal {
                sequence: r.get(0)?,
                kind: r.get(1)?,
                organization: r.get(2)?,
                source_row,
                count: r.get(4)?,
                amount: r.get(5)?,
                work_cell: cell(r.get(6)?, source_row),
                count_cell: cell(r.get(7)?, source_row),
                amount_cell: cell(r.get(8)?, source_row),
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
    /// At most four configured organizations, in source first-seen order.
    pub fn report_organizations(&self, id: &str) -> Result<Vec<ReportOrganization>> {
        self.conversion_ready()?;
        let mut stmt = self.db.prepare("SELECT organization,count,amount FROM converted_organizations WHERE report_id=?1 ORDER BY seq LIMIT 4")?;
        let rows = stmt.query_map([id], |r| {
            Ok(ReportOrganization {
                organization: r.get(0)?,
                count: r.get(1)?,
                amount: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
    pub fn report_rows(&self, id: &str, after: u64, limit: usize) -> Result<Vec<ReportRow>> {
        page_limit(limit)?;
        self.conversion_ready()?;
        let mut s=self.db.prepare("SELECT row_index,organization,work,count,amount,source_row,work_cell,count_cell,amount_cell FROM converted_rows WHERE report_id=?1 AND row_index>?2 ORDER BY row_index LIMIT ?3")?;
        let r = s.query_map(params![id, after, limit], |r| {
            Ok(ReportRow {
                row_index: r.get(0)?,
                organization: r.get(1)?,
                work: r.get(2)?,
                count: r.get(3)?,
                amount: r.get(4)?,
                source_row: r.get(5)?,
                work_cell: r.get(6)?,
                count_cell: r.get(7)?,
                amount_cell: r.get(8)?,
            })
        })?;
        Ok(r.collect::<std::result::Result<_, _>>()?)
    }
    pub fn report_issues(&self, id: &str, after: u64, limit: usize) -> Result<Vec<ReportIssue>> {
        page_limit(limit)?;
        self.conversion_ready()?;
        let mut s=self.db.prepare("SELECT seq,kind,reason,source_row,organization,work_cell,count_cell,amount_cell,expected_count,expected_amount,actual_count,actual_amount,scope,date,source_issue_seq,skipped FROM converted_issues WHERE report_id=?1 AND seq>?2 ORDER BY seq LIMIT ?3")?;
        let r = s.query_map(params![id, after, limit], |r| {
            Ok(ReportIssue {
                sequence: r.get(0)?,
                kind: r.get(1)?,
                reason: r.get(2)?,
                source_row: r.get(3)?,
                organization: r.get(4)?,
                work_cell: r.get(5)?,
                count_cell: r.get(6)?,
                amount_cell: r.get(7)?,
                expected_count: r.get(8)?,
                expected_amount: r.get(9)?,
                actual_count: r.get(10)?,
                actual_amount: r.get(11)?,
                scope: r.get(12)?,
                date: r.get(13)?,
                source_issue_sequence: r.get(14)?,
                skipped: r.get(15)?,
            })
        })?;
        Ok(r.collect::<std::result::Result<_, _>>()?)
    }
}
fn detect(db: &Connection, sheet: &str) -> Result<Vec<TripleMapping>> {
    let mut stmt=db.prepare("SELECT c.row_num,c.col_num,CASE WHEN c.kind='s' THEN s.value ELSE c.value END FROM cells c LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER) WHERE c.sheet=?1 AND c.row_num<=10 ORDER BY c.row_num,c.col_num")?;
    let mut rows = stmt.query([sheet])?;
    let mut q = None;
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    while let Some(row) = rows.next()? {
        let r: u32 = row.get(0)?;
        let c: u32 = row.get(1)?;
        let text: Option<String> = row.get(2)?;
        let text = normalize(text.as_deref().unwrap_or(""));
        if q == Some((r, c - 1)) && text.contains("сумм") && c > 2 && seen.insert(c - 2) {
            found.push(TripleMapping {
                header_row: r,
                work_col: c - 2,
                count_col: c - 1,
                amount_col: c,
            });
        }
        q = if text.contains("количеств") {
            Some((r, c))
        } else {
            None
        };
    }
    found.sort_by_key(|m| m.work_col);
    Ok(found)
}
#[derive(Clone, Copy)]
struct Active {
    org: usize,
    count: f64,
    amount: f64,
    row: u32,
    unchecked: bool,
}
struct Context<'a> {
    db: &'a Connection,
    cfg: &'a LegacyOptions,
    m: &'a TripleMapping,
    id: &'a str,
    active: Option<Active>,
    by_org: [(f64, f64); 4],
    present: [u64; 4],
    present_count: u64,
    rows: u64,
    issues: u64,
    totals: u64,
    sum: (f64, f64),
    mismatch: bool,
    overall: bool,
    daily: bool,
}
impl Context<'_> {
    fn issue(
        &mut self,
        kind: &str,
        reason: &str,
        row: Option<u32>,
        org: Option<usize>,
        cells: (Option<String>, Option<String>, Option<String>),
        expected: Option<(f64, f64)>,
        actual: Option<(f64, f64)>,
        skipped: bool,
    ) -> Result<()> {
        self.issues += 1;
        self.db.execute(
            "INSERT INTO converted_issues VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,NULL,NULL,NULL,?14)",
            params![
                self.id,
                self.issues,
                kind,
                reason,
                row,
                org.map(|i| &self.cfg.organizations[i]),
                cells.0,
                cells.1,
                cells.2,
                expected.map(|x| x.0),
                expected.map(|x| x.1),
                actual.map(|x| x.0),
                actual.map(|x| x.1),
                skipped.then_some(true)
            ],
        )?;
        Ok(())
    }
    fn check_block(&mut self) -> Result<()> {
        if let Some(a) = self.active {
            if a.unchecked {
                return Ok(());
            }
            let actual = self.by_org[a.org];
            if (actual.0 - a.count).abs() > 0.001 || (actual.1 - a.amount).abs() > 0.03 {
                self.mismatch = true;
                self.issue(
                    "mismatch",
                    "Не сошлись контрольные значения подразделения",
                    Some(a.row),
                    Some(a.org),
                    (
                        None,
                        Some(cell(self.m.count_col, a.row)),
                        Some(cell(self.m.amount_col, a.row)),
                    ),
                    Some((a.count, a.amount)),
                    Some(actual),
                    false,
                )?;
            }
        }
        Ok(())
    }
    fn mark_present(&mut self, org: usize) {
        if self.present[org] == 0 {
            self.present_count += 1;
            self.present[org] = self.present_count;
        }
    }
    fn add(&mut self, row: u32, work: &str, q: f64, a: f64) -> Result<()> {
        let org = self
            .active
            .ok_or_else(|| fail("No active organization"))?
            .org;
        self.rows += 1;
        self.sum.0 += q;
        self.sum.1 += a;
        self.by_org[org].0 += q;
        self.by_org[org].1 += a;
        self.mark_present(org);
        if [
            self.sum.0,
            self.sum.1,
            self.by_org[org].0,
            self.by_org[org].1,
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return Err(fail("Нефинитный итог отчёта."));
        }
        self.db.execute(
            "INSERT INTO converted_rows VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                self.id,
                self.rows,
                self.cfg.organizations[org],
                work,
                q,
                a,
                row,
                cell(self.m.work_col, row),
                cell(self.m.count_col, row),
                cell(self.m.amount_col, row)
            ],
        )?;
        Ok(())
    }
    fn process(&mut self, row: u32, values: &[Option<String>; 3]) -> Result<bool> {
        let work = values[0].as_deref().unwrap_or("").trim_matches(js_space);
        let q = number(values[1].as_deref());
        let a = number(values[2].as_deref());
        let active = self.active.map(|x| x.org);
        let bad_cells = (
            None,
            if q.is_none() {
                Some(cell(self.m.count_col, row))
            } else {
                None
            },
            if a.is_none() {
                Some(cell(self.m.amount_col, row))
            } else {
                None
            },
        );
        let raw_present = |slot: usize| {
            values[slot]
                .as_deref()
                .is_some_and(|s| !s.trim_matches(js_space).is_empty())
        };
        let blank_pair = !raw_present(1) && !raw_present(2);
        if work.is_empty() {
            if let (Some(q), Some(a), Some(_)) = (q, a, active) {
                self.add(row, &format!("[Без названия — строка {row}]"), q, a)?;
                self.issue(
                    "empty-name",
                    "Название работы пустое; строка включена с временным названием",
                    Some(row),
                    active,
                    (Some(cell(self.m.work_col, row)), None, None),
                    None,
                    None,
                    false,
                )?;
            } else if q.is_some() || a.is_some() || raw_present(1) || raw_present(2) {
                let cells = if q.is_some() || a.is_some() {
                    (Some(cell(self.m.work_col, row)), bad_cells.1, bad_cells.2)
                } else {
                    (
                        Some(cell(self.m.work_col, row)),
                        raw_present(1).then(|| cell(self.m.count_col, row)),
                        raw_present(2).then(|| cell(self.m.amount_col, row)),
                    )
                };
                self.issue(
                    "invalid",
                    "Строка без названия содержит неполные или некорректные значения",
                    Some(row),
                    active,
                    cells,
                    None,
                    None,
                    true,
                )?;
            }
            return Ok(false);
        }
        let lower = work.to_lowercase();
        let total = ["итого", "всего"].iter().any(|p| {
            lower.strip_prefix(p).is_some_and(|tail| {
                tail.is_empty()
                    || tail.starts_with(':')
                    || tail.chars().next().is_some_and(js_space)
            })
        });
        if total {
            self.check_block()?;
            if self.daily && blank_pair {
                return Ok(self.rows > 0);
            }
            if self.rows > 0 || self.daily {
                self.overall = true;
                self.totals += 1;
                self.db.execute(
                    "INSERT INTO converted_totals VALUES(?1,?2,'all',NULL,?3,?4,?5)",
                    params![self.id, self.totals, row, q, a],
                )?;
                if q.is_none() || a.is_none() {
                    self.issue(
                        "invalid",
                        "Общий итог содержит некорректное количество или сумму",
                        Some(row),
                        None,
                        bad_cells,
                        None,
                        None,
                        false,
                    )?;
                } else if (self.sum.0 - q.unwrap()).abs() > 0.001
                    || (self.sum.1 - a.unwrap()).abs() > 0.03
                {
                    self.mismatch = true;
                    self.issue(
                        "mismatch",
                        "Общий итог не совпадает с детализацией (сверьте строки)",
                        Some(row),
                        None,
                        (
                            None,
                            Some(cell(self.m.count_col, row)),
                            Some(cell(self.m.amount_col, row)),
                        ),
                        Some((q.unwrap(), a.unwrap())),
                        Some(self.sum),
                        false,
                    )?;
                }
                return Ok(true);
            }
            return Ok(false);
        }
        if let Some(org) = self
            .cfg
            .organizations
            .iter()
            .position(|n| normalize(n) == normalize(work))
        {
            self.check_block()?;
            if self.daily && blank_pair {
                self.active = Some(Active {
                    org,
                    count: 0.,
                    amount: 0.,
                    row,
                    unchecked: true,
                });
                return Ok(false);
            }
            if let (Some(q), Some(a)) = (q, a) {
                self.active = Some(Active {
                    org,
                    count: q,
                    amount: a,
                    row,
                    unchecked: false,
                });
                self.mark_present(org);
                self.totals += 1;
                self.db.execute(
                    "INSERT INTO converted_totals VALUES(?1,?2,'org',?3,?4,?5,?6)",
                    params![self.id, self.totals, self.cfg.organizations[org], row, q, a],
                )?;
            } else {
                self.issue(
                    "invalid",
                    "Итог подразделения не содержит оба числа",
                    Some(row),
                    Some(org),
                    bad_cells,
                    None,
                    None,
                    false,
                )?;
                self.active = None;
            }
            return Ok(false);
        }
        if self.daily && blank_pair {
            return Ok(false);
        }
        if let (Some(q), Some(a)) = (q, a) {
            if active.is_some() {
                self.add(row, work, q, a)?;
            } else {
                self.issue(
                    "skipped",
                    "Строка без распознанного подразделения",
                    Some(row),
                    None,
                    (Some(cell(self.m.work_col, row)), None, None),
                    None,
                    None,
                    true,
                )?;
            }
        } else {
            self.issue(
                "invalid",
                "Пропущена строка: нет количества или суммы",
                Some(row),
                active,
                bad_cells,
                None,
                None,
                true,
            )?;
        }
        Ok(false)
    }
}
#[cfg(test)]
mod numeric_tests {
    use super::*;
    #[test]
    fn rounding_matches_math_round_after_legacy_epsilon_scaling() {
        for (input, places, expected) in [
            (4503599627.370497, 6, 4503599627.370497),
            (45035996273704.97, 2, 45035996273704.97),
            (-45035996273704.97, 2, -45035996273704.97),
            (0.4999999999999997, 0, 0.0),
            (-0.5000000000000003, 0, -1.0),
            (-1.5, 0, -1.0),
            (-2.5, 0, -2.0),
            (-0.5, 0, -0.0),
            (-0.1, 0, -0.0),
            (1.005, 2, 1.01),
            (1.2345678, 6, 1.234568),
        ] {
            assert_eq!(
                round(input, places).to_bits(),
                (expected as f64).to_bits(),
                "{input} / {places}"
            );
        }
        assert!(!round(1e308, 6).is_finite());
    }
    #[test]
    fn numeric_cells_match_ecmascript_whitespace_and_finite_rules() {
        assert_eq!(number(Some("1\u{00a0}234,50")), Some(1234.5));
        assert_eq!(number(Some("1\u{feff}2")), Some(12.0));
        assert_eq!(number(Some("1\u{0085}2")), None);
        assert_eq!(number(Some("false")), None);
        assert_eq!(number(Some("0")), Some(0.));
        assert_eq!(number(Some("0x10")), Some(16.));
        assert_eq!(number(Some("1,2,3")), None);
        assert_eq!(number(Some("1e999")), None);
    }
}

pub(crate) fn convert_day(
    db: &Connection,
    cfg: &LegacyOptions,
    mapping: &TripleMapping,
    id: &str,
    cancel: &AtomicBool,
    mut progress: impl FnMut(ConversionSummary),
) -> Result<(ConversionSummary, bool)> {
    let mut context = Context {
        db,
        cfg,
        m: mapping,
        id,
        active: None,
        by_org: [(0., 0.); 4],
        present: [0; 4],
        present_count: 0,
        rows: 0,
        issues: 0,
        totals: 0,
        sum: (0., 0.),
        mismatch: false,
        overall: false,
        daily: true,
    };
    let mut stmt=db.prepare("SELECT c.row_num,c.col_num,CASE WHEN c.kind='s' THEN s.value WHEN c.kind='b' THEN CASE c.value WHEN '1' THEN 'true' ELSE 'false' END ELSE c.value END FROM cells c INDEXED BY cells_by_column LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER) WHERE c.sheet=?1 AND c.row_num>?2 AND c.col_num IN (?3,?4,?5) ORDER BY c.row_num,c.col_num")?;
    let mut rows = stmt.query(params![
        cfg.sheet,
        mapping.header_row,
        mapping.work_col,
        mapping.count_col,
        mapping.amount_col
    ])?;
    let mut row_num = 0;
    let mut values: [Option<String>; 3] = [None, None, None];
    let mut done = false;
    while let Some(row) = rows.next()? {
        check_cancel(cancel)?;
        let current: u32 = row.get(0)?;
        if row_num != 0 && current != row_num {
            if context.process(row_num, &values)? {
                done = true;
                break;
            }
            values = [None, None, None];
            if row_num % 2048 == 0 {
                progress(ConversionSummary {
                    reports: 0,
                    rows: context.rows,
                    issues: context.issues,
                });
            }
        }
        row_num = current;
        let col: u32 = row.get(1)?;
        let slot = if col == mapping.work_col {
            0
        } else if col == mapping.count_col {
            1
        } else {
            2
        };
        values[slot] = row.get(2)?;
    }
    if !done && row_num != 0 {
        context.process(row_num, &values)?;
    }
    if !context.overall {
        context.check_block()?;
    }
    let explicit_zero: bool = context.rows == 0 && db.query_row("SELECT EXISTS(SELECT 1 FROM converted_totals WHERE report_id=?1 AND kind='org') AND NOT EXISTS(SELECT 1 FROM converted_totals WHERE report_id=?1 AND (count IS NULL OR amount IS NULL OR count!=0 OR amount!=0))",[id],|r|r.get(0))?;
    if context.rows == 0 && !explicit_zero {
        context.issue(
            "empty",
            "Нет распознанных работ",
            None,
            None,
            (None, None, None),
            None,
            None,
            false,
        )?;
    }
    for (j, (q, a)) in context.by_org.iter().enumerate() {
        if context.present[j] != 0 {
            context.db.execute(
                "INSERT INTO converted_organizations VALUES(?1,?2,?3,?4,?5)",
                params![id, context.present[j], cfg.organizations[j], q, a],
            )?;
        }
    }
    if !round(context.sum.0, 6).is_finite() || !round(context.sum.1, 2).is_finite() {
        return Err(fail("Итог не помещается в числовой диапазон."));
    }
    context.db.execute(
        "UPDATE converted_reports SET row_count=?2,count=?3,amount=?4,has_mismatch=?5 WHERE id=?1",
        params![
            id,
            context.rows,
            round(context.sum.0, 6),
            round(context.sum.1, 2),
            context.mismatch
        ],
    )?;

    Ok((
        ConversionSummary {
            reports: 1,
            rows: context.rows,
            issues: context.issues,
        },
        explicit_zero,
    ))
}
