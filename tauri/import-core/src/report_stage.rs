//! Sequential conversion of ordinary triples into disk-backed reports.
//! Financial rows and diagnostics are never collected into a report-sized Vec.
use crate::{check_cancel, fail, Result, Stage};
use chrono::{Datelike, Duration, Months, NaiveDate};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::{collections::HashSet, sync::atomic::AtomicBool};
#[derive(Clone, Debug)]
pub struct TripleMapping {
    pub header_row: u32,
    pub work_col: u32,
    pub count_col: u32,
    pub amount_col: u32,
}
#[derive(Clone, Debug)]
pub struct LegacyOptions {
    pub sheet: String,
    pub kind: String,
    pub start: String,
    pub organizations: [String; 4],
    pub mappings: Option<Vec<TripleMapping>>,
}
#[derive(Clone, Copy, Default, Debug)]
pub struct ConversionSummary {
    pub reports: u64,
    pub rows: u64,
    pub issues: u64,
}
#[derive(Debug, Serialize)]
pub struct ReportInfo {
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
#[derive(Debug, Serialize)]
pub struct ReportIssue {
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
const SCHEMA:&str="CREATE TABLE IF NOT EXISTS converted_reports(ordinal INTEGER PRIMARY KEY,id TEXT UNIQUE,sheet TEXT,kind TEXT,start TEXT,end TEXT,row_count INTEGER,count REAL,amount REAL,has_mismatch INTEGER);
 CREATE TABLE IF NOT EXISTS converted_rows(report_id TEXT,row_index INTEGER,organization TEXT,work TEXT,count REAL,amount REAL,source_row INTEGER,work_cell TEXT,count_cell TEXT,amount_cell TEXT,PRIMARY KEY(report_id,row_index)) WITHOUT ROWID;
 CREATE TABLE IF NOT EXISTS converted_issues(report_id TEXT,seq INTEGER,kind TEXT,reason TEXT,source_row INTEGER,organization TEXT,work_cell TEXT,count_cell TEXT,amount_cell TEXT,expected_count REAL,expected_amount REAL,actual_count REAL,actual_amount REAL,PRIMARY KEY(report_id,seq)) WITHOUT ROWID;
 CREATE TABLE IF NOT EXISTS converted_totals(report_id TEXT,seq INTEGER,kind TEXT,organization TEXT,source_row INTEGER,count REAL,amount REAL,PRIMARY KEY(report_id,seq)) WITHOUT ROWID;
 CREATE TABLE IF NOT EXISTS converted_organizations(report_id TEXT,organization TEXT,count REAL,amount REAL,PRIMARY KEY(report_id,organization)) WITHOUT ROWID;";
fn js_space(c: char) -> bool {
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
fn normalize(s: &str) -> String {
    s.trim_end_matches(['.', ',', ';', ':'])
        .split(js_space)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn number(s: Option<&str>) -> Option<f64> {
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
    (((n + f64::EPSILON) * p) + 0.5).floor() / p
}
fn cell(col: u32, row: u32) -> String {
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
fn page_limit(n: usize) -> Result<()> {
    if (1..=100).contains(&n) {
        Ok(())
    } else {
        Err(fail("report page limit is 1..100"))
    }
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
                || m.header_row > 60000
                || [m.work_col, m.count_col, m.amount_col]
                    .iter()
                    .any(|c| *c == 0 || *c > 2048)
                || m.work_col == m.count_col
                || m.work_col == m.amount_col
                || m.count_col == m.amount_col
            {
                return Err(fail("Некорректное сопоставление столбцов."));
            }
        }
        self.db.execute_batch(SCHEMA)?;
        self.db.execute_batch("BEGIN IMMEDIATE;DELETE FROM converted_reports;DELETE FROM converted_rows;DELETE FROM converted_issues;DELETE FROM converted_totals;DELETE FROM converted_organizations;DELETE FROM manifest WHERE name='conversion_complete';")?;
        let result = (|| {
            let mut summary = ConversionSummary::default();
            for (i, mapping) in mappings.iter().enumerate() {
                check_cancel(cancel)?;
                let first = match cfg.kind.as_str() {
                    "month" => start.checked_add_months(Months::new(i as u32)),
                    "week" => start.checked_add_signed(Duration::days(i as i64 * 7)),
                    _ => start.checked_add_signed(Duration::days(i as i64)),
                }
                .ok_or_else(|| fail("Дата периода вне диапазона."))?;
                let end = match cfg.kind.as_str() {
                    "week" => first.checked_add_signed(Duration::days(6)),
                    "month" => first
                        .checked_add_months(Months::new(1))
                        .and_then(|d| d.checked_sub_signed(Duration::days(1))),
                    _ => Some(first),
                }
                .ok_or_else(|| fail("Дата периода вне диапазона."))?;
                if first.year() > 9999 || end.year() > 9999 {
                    return Err(fail("Дата периода вне диапазона."));
                }
                let id = format!("legacy-{}", i + 1);
                self.db.execute(
                    "INSERT INTO converted_reports VALUES(?1,?2,?3,?4,?5,?6,0,0,0,0)",
                    params![
                        i + 1,
                        id,
                        cfg.sheet,
                        cfg.kind,
                        first.to_string(),
                        end.to_string()
                    ],
                )?;
                let mut context = Context {
                    db: &self.db,
                    cfg,
                    m: mapping,
                    id: &id,
                    active: None,
                    by_org: [(0., 0.); 4],
                    present: [false; 4],
                    rows: 0,
                    issues: 0,
                    totals: 0,
                    sum: (0., 0.),
                    mismatch: false,
                    overall: false,
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
                    )?;
                }
                for (j, (q, a)) in context.by_org.iter().enumerate() {
                    if context.present[j] {
                        context.db.execute(
                            "INSERT INTO converted_organizations VALUES(?1,?2,?3,?4)",
                            params![id, cfg.organizations[j], q, a],
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
    fn conversion_ready(&self) -> Result<()> {
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
        let mut s=self.db.prepare("SELECT id,start,end,kind,row_count,count,amount,has_mismatch FROM converted_reports WHERE ordinal>?1 ORDER BY ordinal LIMIT ?2")?;
        let r = s.query_map(params![after, limit], |r| {
            Ok(ReportInfo {
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
        let mut s=self.db.prepare("SELECT seq,kind,reason,source_row,organization,work_cell,count_cell,amount_cell,expected_count,expected_amount,actual_count,actual_amount FROM converted_issues WHERE report_id=?1 AND seq>?2 ORDER BY seq LIMIT ?3")?;
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
}
struct Context<'a> {
    db: &'a Connection,
    cfg: &'a LegacyOptions,
    m: &'a TripleMapping,
    id: &'a str,
    active: Option<Active>,
    by_org: [(f64, f64); 4],
    present: [bool; 4],
    rows: u64,
    issues: u64,
    totals: u64,
    sum: (f64, f64),
    mismatch: bool,
    overall: bool,
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
    ) -> Result<()> {
        self.issues += 1;
        self.db.execute(
            "INSERT INTO converted_issues VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
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
                actual.map(|x| x.1)
            ],
        )?;
        Ok(())
    }
    fn check_block(&mut self) -> Result<()> {
        if let Some(a) = self.active {
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
                )?;
            }
        }
        Ok(())
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
        self.present[org] = true;
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
                )?;
            } else if values[1..].iter().any(|v| {
                v.as_deref()
                    .is_some_and(|s| !s.trim_matches(js_space).is_empty())
            }) {
                self.issue(
                    "invalid",
                    "Строка без названия содержит неполные или некорректные значения",
                    Some(row),
                    active,
                    (Some(cell(self.m.work_col, row)), bad_cells.1, bad_cells.2),
                    None,
                    None,
                )?;
            }
            return Ok(false);
        }
        let lower = work.to_lowercase();
        let total = ["итого", "всего"].iter().any(|p| {
            lower.strip_prefix(p).is_some_and(|tail| {
                tail.is_empty()
                    || tail.starts_with(':')
                    || tail.chars().next().is_some_and(char::is_whitespace)
            })
        });
        if total {
            self.check_block()?;
            if self.rows > 0 {
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
            if let (Some(q), Some(a)) = (q, a) {
                self.active = Some(Active {
                    org,
                    count: q,
                    amount: a,
                    row,
                });
                self.present[org] = true;
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
                )?;
                self.active = None;
            }
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
            )?;
        }
        Ok(false)
    }
}
#[cfg(test)]
mod numeric_tests {
    use super::*;
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
