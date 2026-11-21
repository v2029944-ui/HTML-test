//! Explicit dates in the first column block of an ordinary report.
use super::{cell, LegacyOptions, TripleMapping};
use crate::{check_cancel, wide::excel_serial_date, Result, Stage};
use chrono::{Datelike, Duration, Months, NaiveDate};
use regex::Regex;
use rusqlite::{params, OptionalExtension};
use std::sync::{atomic::AtomicBool, LazyLock};

static LABEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(
    r"(?i)^(?:дата\s+)?начал[оа]\s+(?:(?:отч[её]тного|первого|первой)\s+)?(периода|недели|месяца|дня)(?:\s*[:=—-]\s*(.*))?$"
).unwrap());
static RANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(
    r"(?i)^(?:отч[её]тный\s+)?период\s*[:=—-]?\s*с\s+(\S+)\s+по\s+(\S+)$"
).unwrap());

pub(super) struct Notice {
    pub row: Option<u32>,
    pub address: Option<String>,
    pub reason: String,
}

fn text_date(text: &str, kind: &str) -> Option<NaiveDate> {
    let text = text.trim();
    let date = match text.len() {
        10 => ["%Y-%m-%d", "%d.%m.%Y", "%d/%m/%Y"].iter()
            .find_map(|format| NaiveDate::parse_from_str(text, format).ok()),
        7 if kind == "month" => NaiveDate::parse_from_str(&format!("{text}-01"), "%Y-%m-%d").ok(),
        _ => None,
    }?;
    (date.year() >= 1900 && (kind != "month" || date.day() == 1)).then_some(date)
}

fn range_start(text: &str, kind: &str) -> Option<NaiveDate> {
    let range = RANGE.captures(text)?;
    let start = text_date(&range[1], "day")?;
    if kind == "month" && start.day() != 1 { return None; }
    let end = text_date(&range[2], "day")?;
    let expected_end = match kind {
        "week" => start.checked_add_signed(Duration::days(6)),
        "month" => start.checked_add_months(Months::new(1)).and_then(|d| d.pred_opt()),
        _ => Some(start),
    };
    (expected_end == Some(end)).then_some(start)
}

fn adjacent_date(
    stage: &Stage<'_>, cfg: &LegacyOptions, row: u32, col: u32,
    styles_ready: &mut bool, cancel: &AtomicBool,
) -> Result<Option<NaiveDate>> {
    let adjacent: Option<(String, String, i64)> = stage.db.query_row(
        "SELECT CASE WHEN length(COALESCE(s.value,c.value))<=4096 THEN COALESCE(s.value,c.value) ELSE '' END,c.kind,CAST(COALESCE(c.style,'0') AS INTEGER)
         FROM cells c LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER)
         WHERE c.sheet=?1 AND c.row_num=?2 AND c.col_num=?3",
        params![cfg.sheet, row, col], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).optional()?;
    let Some((value, kind, style)) = adjacent else { return Ok(None) };
    if !matches!(kind.as_str(), "" | "n") {
        let date = if kind == "d" { value.split('T').next().unwrap_or("") } else { &value };
        return Ok(text_date(date, &cfg.kind));
    }
    let Some(serial) = value.parse::<f64>().ok().filter(|n| (0.0..2958466.0).contains(n))
        else { return Ok(None) };
    if !*styles_ready {
        stage.setup_date_styles(cancel)?;
        *styles_ready = true;
    }
    let styled: bool = stage.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM date_styles WHERE style=?1)", [style], |r| r.get(0),
    )?;
    if !styled { return Ok(None); }
    let epoch: bool = stage.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM nodes n JOIN attributes a ON n.component=a.component AND n.seq=a.seq
         WHERE n.component='xl/workbook.xml' AND n.tag='workbookPr' AND a.key='date1904' AND a.value IN ('1','true'))",
        [], |r| r.get(0),
    )?;
    // Excel's fictitious 1900-02-29 and values outside its calendar provide no date evidence.
    Ok(excel_serial_date(serial, epoch).ok().filter(|date|
        (1900..=9999).contains(&date.year()) && (cfg.kind != "month" || date.day()==1)))
}

pub(super) fn check(
    stage: &Stage<'_>, cfg: &LegacyOptions, mappings: &[TripleMapping],
    selected: NaiveDate, cancel: &AtomicBool,
) -> Result<Vec<Notice>> {
    let first = &mappings[0];
    let left = first.work_col;
    let right = mappings.iter().filter(|m| m.work_col > left).map(|m| m.work_col - 1)
        .fold(left.max(first.count_col).max(first.amount_col), u32::min);
    let mut truncated = first.header_row > 64 || right-left >= 64;
    let right = right.min(left+63);
    let mut query = stage.db.prepare(
        "SELECT c.row_num,c.col_num,substr(COALESCE(s.value,c.value),1,4097)
         FROM cells c LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER)
         WHERE c.sheet=?1 AND c.row_num<=?2 AND c.col_num BETWEEN ?3 AND ?4
         AND c.kind IN ('s','inlineStr','str') ORDER BY c.row_num,c.col_num"
    )?;
    let mut rows = query.query(params![cfg.sheet, first.header_row.min(64), left, right])?;
    let mut seen = 0;
    let mut notices = Vec::new();
    let mut styles_ready = false;
    while let Some(item) = rows.next()? {
        check_cancel(cancel)?;
        let (row, col): (u32, u32) = (item.get(0)?, item.get(1)?);
        let value = item.get::<_, Option<String>>(2)?.unwrap_or_default();
        if value.chars().count() > 4096 { truncated = true; continue; }
        let text = value.trim();
        let (date, source_col) = if let Some(label) = LABEL.captures(text) {
            let wanted = match label[1].to_lowercase().as_str() {
                "недели" => "week", "месяца" => "month", "дня" => "day", _ => &cfg.kind,
            };
            if wanted != cfg.kind { continue; }
            if let Some(inline) = label.get(2).filter(|m| !m.as_str().trim().is_empty()) {
                (text_date(inline.as_str(), &cfg.kind), col)
            } else if col < right {
                (adjacent_date(stage, cfg, row, col+1, &mut styles_ready, cancel)?, col+1)
            } else { continue; }
        } else {
            (range_start(text, &cfg.kind), col)
        };
        let Some(date) = date else { continue };
        seen += 1;
        if seen > 16 { truncated = true; break; }
        if date != selected {
            let address = cell(source_col, row);
            notices.push(Notice {
                row: Some(row), address: Some(address.clone()),
                reason: format!("Начало периода в Excel ({address}): {date}. Выбрано {selected}. Исправьте первую дату или подтвердите замечание при ручном сохранении."),
            });
        }
    }
    if truncated {
        notices.push(Notice {
            row: None, address: None,
            reason: "Шапка первого набора превышает область проверки даты. Сверьте начало периода с Excel перед ручным сохранением.".into(),
        });
    }
    Ok(notices)
}
