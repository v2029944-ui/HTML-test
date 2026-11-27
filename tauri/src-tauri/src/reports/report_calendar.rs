//! Saved report counts by start date. All reads share one revision-checked snapshot.
use super::{err, read_model, Storage};
use rusqlite::{params, Connection, OpenFlags};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportCalendar {
    year: i32,
    first_year: Option<i32>,
    last_year: Option<i32>,
    days: Vec<CalendarDay>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct CalendarDay {
    date: String,
    day_reports: i64,
    week_reports: i64,
    month_reports: i64,
}

impl Storage {
    pub fn report_calendar(&self, revision: i64, year: i32) -> Result<ReportCalendar, String> {
        if !(1..=9999).contains(&year) {
            return Err("Год календаря должен быть от 1 до 9999.".into());
        }
        let mut conn =
            Connection::open_with_flags(self.db_path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(err)?;
        let tx = conn.transaction().map_err(err)?;
        read_model::check_revision(&tx, revision)?;
        let period_index = read_model::ReportPeriodIndex::detect(&tx)?;
        // Read the covering header index. Check lengths before parsing dates or
        // returning text so corrupt legacy metadata cannot inflate the response.
        let (first_year, last_year, invalid): (Option<i32>, Option<i32>, bool) = tx.query_row(
            &period_index.sql("SELECT MIN(CASE WHEN octet_length(period_start)=10 THEN CAST(substr(period_start,1,4) AS INTEGER) END),
                    MAX(CASE WHEN octet_length(period_start)=10 THEN CAST(substr(period_start,1,4) AS INTEGER) END),
                    COALESCE(MAX(CASE
                      WHEN typeof(period_start) <> 'text' OR octet_length(period_start) <> 10 THEN 1
                      WHEN octet_length(report_type) NOT BETWEEN 3 AND 5 THEN 1
                      WHEN report_type NOT IN ('day','week','month') THEN 1
                      WHEN period_start NOT GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                        OR substr(period_start,1,4) = '0000'
                        OR date(period_start,'+0 days') IS NOT period_start THEN 1
                      ELSE 0 END),0)
             FROM reports INDEXED BY reports_by_period"),
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).map_err(err)?;
        if invalid {
            return Err("Некорректная дата начала или тип сохранённого отчёта.".into());
        }

        // Group in the covering index's order. At most three counters per
        // valid date leave SQLite; IDs, source names and report JSON stay out.
        let mut days = BTreeMap::new();
        let mut stmt = tx
            .prepare(
                "SELECT report_type,period_start,COUNT(*) FROM reports
             WHERE report_type IN ('day','week','month') AND period_start BETWEEN ?1 AND ?2
             GROUP BY report_type,period_start",
            )
            .map_err(err)?;
        let mut rows = stmt
            .query(params![
                format!("{year:04}-01-01"),
                format!("{year:04}-12-31")
            ])
            .map_err(err)?;
        while let Some(row) = rows.next().map_err(err)? {
            let date: String = row.get(1).map_err(err)?;
            let day = days.entry(date.clone()).or_insert_with(|| CalendarDay {
                date,
                ..Default::default()
            });
            let count = row.get(2).map_err(err)?;
            match row.get::<_, String>(0).map_err(err)?.as_str() {
                "day" => day.day_reports = count,
                "week" => day.week_reports = count,
                "month" => day.month_reports = count,
                _ => return Err("Неизвестный тип отчёта.".into()),
            }
        }
        drop(rows);
        drop(stmt);
        tx.commit().map_err(err)?;
        Ok(ReportCalendar {
            year,
            first_year,
            last_year,
            days: days.into_values().collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn insert(conn: &Connection, id: &str, kind: &str, start: &str) {
        conn.execute(
            "INSERT INTO reports VALUES(?1,?2,?3,?3,NULL,x'FF')",
            params![id, kind, start],
        )
        .unwrap();
    }

    #[test]
    fn report_calendar_counts_saved_headers_by_start_without_reading_json() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::new(dir.path().into()).unwrap();
        let conn = store.connect().unwrap();
        for (id, kind, date) in [
            ("a", "day", "2024-02-29"),
            ("b", "day", "2024-02-29"),
            ("c", "week", "2024-02-29"),
            ("d", "month", "2024-02-29"),
            ("e", "day", "0001-01-01"),
            ("f", "day", "9999-12-31"),
        ] {
            insert(&conn, id, kind, date);
        }
        let rev = store.revision().unwrap();
        let calendar = store.report_calendar(rev, 2024).unwrap();
        assert_eq!(calendar.first_year, Some(1));
        assert_eq!(calendar.last_year, Some(9999));
        assert_eq!(
            serde_json::to_value(calendar).unwrap()["days"],
            json!([
                {"date":"2024-02-29","dayReports":2,"weekReports":1,"monthReports":1}
            ])
        );
        assert_eq!(store.report_calendar(rev, 9999).unwrap().days.len(), 1);
        assert!(store.report_calendar(rev, 2023).unwrap().days.is_empty());
        for year in [0, 10000] {
            assert!(store.report_calendar(rev, year).is_err());
        }
        assert!(store
            .report_calendar(rev - 1, 2024)
            .unwrap_err()
            .contains("Stale"));
    }

    #[test]
    fn report_calendar_emits_at_most_one_row_per_date_in_a_leap_year() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::new(dir.path().into()).unwrap();
        let conn = store.connect().unwrap();
        conn.execute_batch(
            "WITH RECURSIVE dates(d) AS (
               SELECT '2024-01-01' UNION ALL SELECT date(d,'+1 day') FROM dates WHERE d<'2024-12-31'
             ) INSERT INTO reports SELECT d,'day',d,d,NULL,x'FF' FROM dates",
        )
        .unwrap();
        let calendar = store
            .report_calendar(store.revision().unwrap(), 2024)
            .unwrap();
        assert_eq!(calendar.days.len(), 366);
        assert_eq!(calendar.days.first().unwrap().date, "2024-01-01");
        assert_eq!(calendar.days.last().unwrap().date, "2024-12-31");
    }

    #[test]
    fn report_calendar_distinguishes_empty_database_and_invalid_headers() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::new(dir.path().into()).unwrap();
        let conn = store.connect().unwrap();
        let calendar = store
            .report_calendar(store.revision().unwrap(), 2024)
            .unwrap();
        assert_eq!((calendar.first_year, calendar.last_year), (None, None));
        assert!(calendar.days.is_empty());
        for (kind, date) in [
            ("day", "2023-02-29"),
            ("week", "0000-01-01"),
            ("unknown", "2024-02-01"),
            ("month", &"9".repeat(100_000)),
        ] {
            insert(&conn, "bad", kind, date);
            assert!(store
                .report_calendar(store.revision().unwrap(), 2024)
                .is_err());
            conn.execute("DELETE FROM reports", []).unwrap();
        }
    }
}
