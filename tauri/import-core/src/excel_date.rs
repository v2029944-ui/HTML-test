//! Excel cell date parsing and number-format styles shared by both report layouts.
use crate::{check_cancel, fail, Result};
use chrono::{Duration, NaiveDate};
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{atomic::AtomicBool, LazyLock};

pub(crate) fn setup_date_styles(db: &Connection, cancel: &AtomicBool) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS date_styles(style INTEGER PRIMARY KEY) WITHOUT ROWID; CREATE TABLE IF NOT EXISTS custom_formats(id INTEGER PRIMARY KEY,format TEXT); DELETE FROM date_styles; DELETE FROM custom_formats;")?;
    let mut formats=db.prepare("SELECT i.value,coalesce(f.value,'') FROM nodes n JOIN attributes i ON i.component=n.component AND i.seq=n.seq AND i.key='numFmtId' LEFT JOIN attributes f ON f.component=n.component AND f.seq=n.seq AND f.key='formatCode' WHERE n.component='xl/styles.xml' AND n.tag='numFmt' ORDER BY n.seq")?;
    let mut rows = formats.query([])?;
    while let Some(row) = rows.next()? {
        check_cancel(cancel)?;
        let id: String = row.get(0)?;
        let id = id
            .parse::<u32>()
            .map_err(|_| fail("invalid Excel number format id"))?;
        let format: String = row.get(1)?;
        db.execute("INSERT INTO custom_formats VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET format=excluded.format",params![id,format])?;
    }
    let mut styles=db.prepare("SELECT coalesce(a.value,'0') FROM nodes n LEFT JOIN attributes a ON a.component=n.component AND a.seq=n.seq AND a.key='numFmtId' WHERE n.component='xl/styles.xml' AND n.tag='xf' AND n.parent='cellXfs' ORDER BY n.seq")?;
    let mut rows = styles.query([])?;
    let mut index = 0u32;
    while let Some(row) = rows.next()? {
        check_cancel(cancel)?;
        let id: String = row.get(0)?;
        let id = id
            .parse::<u32>()
            .map_err(|_| fail("invalid Excel style format id"))?;
        let format: Option<String> = db
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
            db.execute("INSERT INTO date_styles VALUES(?1)", [index])?;
        }
        index += 1;
    }
    Ok(())
}

pub(crate) fn uses_1904_epoch(db: &Connection) -> Result<bool> {
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM nodes n JOIN attributes a ON a.component=n.component AND a.seq=n.seq AND a.key='date1904' WHERE n.component='xl/workbook.xml' AND n.tag='workbookPr' AND a.value IN ('1','true'))",[],|r|r.get(0))?)
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
