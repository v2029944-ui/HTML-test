//! Ordinary report column mappings and period selection.
use crate::{fail, Result};
use chrono::{Datelike, Duration, Months, NaiveDate};
use serde::Serialize;

#[path = "ordinary_date.rs"]
mod ordinary_date;
pub(crate) use ordinary_date::check as check_workbook_dates;

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

pub(crate) fn legacy_periods(
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
