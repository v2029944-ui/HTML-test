//! Bounded analysis API; no GUI commands. See storage-core/ANALYTICS.md.
use super::read_model::{check_metadata_bounds, check_revision};
use super::*;
use chrono::{Datelike, Duration, Months};
use rusqlite::OptionalExtension;
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::io::{BufReader, Read, Write};
const BOUND: usize = 65536;

pub(super) fn initialize(c: &Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS analysis_reports(report_id TEXT PRIMARY KEY, valid INTEGER NOT NULL, issue TEXT, warned INTEGER NOT NULL);
 CREATE TABLE IF NOT EXISTS analysis_orgs(report_id TEXT NOT NULL,org TEXT NOT NULL,PRIMARY KEY(report_id,org)) WITHOUT ROWID;
 CREATE INDEX IF NOT EXISTS analysis_org_lookup ON analysis_orgs(org,report_id);
 CREATE TABLE IF NOT EXISTS analysis_settings(singleton INTEGER PRIMARY KEY CHECK(singleton=1));
 CREATE TABLE IF NOT EXISTS analysis_groups(work TEXT PRIMARY KEY,key TEXT NOT NULL,name TEXT NOT NULL) WITHOUT ROWID;
 CREATE TABLE IF NOT EXISTS analysis_filters(id TEXT PRIMARY KEY,revision INTEGER NOT NULL,kind TEXT NOT NULL,sealed INTEGER NOT NULL);
 CREATE TABLE IF NOT EXISTS analysis_filter_items(id TEXT NOT NULL,item TEXT NOT NULL,ordinal INTEGER NOT NULL,PRIMARY KEY(id,item)) WITHOUT ROWID;
 CREATE TRIGGER IF NOT EXISTS analysis_report_insert AFTER INSERT ON reports BEGIN
 DELETE FROM analysis_reports WHERE report_id=NEW.id;DELETE FROM analysis_orgs WHERE report_id=NEW.id;END;
 CREATE TRIGGER IF NOT EXISTS analysis_report_update AFTER UPDATE ON reports BEGIN
 DELETE FROM analysis_reports WHERE report_id IN(OLD.id,NEW.id);DELETE FROM analysis_orgs WHERE report_id IN(OLD.id,NEW.id);END;
 CREATE TRIGGER IF NOT EXISTS analysis_report_delete AFTER DELETE ON reports BEGIN
 DELETE FROM analysis_reports WHERE report_id=OLD.id;DELETE FROM analysis_orgs WHERE report_id=OLD.id;END;
 CREATE TRIGGER IF NOT EXISTS analysis_settings_insert AFTER INSERT ON kv WHEN NEW.name='settings' BEGIN DELETE FROM analysis_settings;DELETE FROM analysis_groups;END;
 CREATE TRIGGER IF NOT EXISTS analysis_settings_update AFTER UPDATE ON kv WHEN OLD.name='settings' OR NEW.name='settings' BEGIN DELETE FROM analysis_settings;DELETE FROM analysis_groups;END;
 CREATE TRIGGER IF NOT EXISTS analysis_settings_delete AFTER DELETE ON kv WHEN OLD.name='settings' BEGIN DELETE FROM analysis_settings;DELETE FROM analysis_groups;END;").map_err(err)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisScope {
    pub report_type: String,
    pub organizations: String,
    pub names: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeQuery {
    pub from: String,
    pub to: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeQuery {
    pub method: String,
    pub from: String,
    pub to: String,
    pub percent: f64,
    pub min_count: f64,
    pub min_amount: f64,
}
#[derive(Debug, Serialize)]
pub struct AnalysisTotals {
    pub count: Option<f64>,
    pub amount: Option<f64>,
    pub average: Option<f64>,
}
#[derive(Debug, Serialize)]
pub struct RangeSummary {
    pub complete: bool,
    pub periods: i64,
    pub organizations: i64,
    pub missing: i64,
    pub values: i64,
    /// Complete totals: always null when any required report is missing.
    pub total: AnalysisTotals,
    /// Reports actually observed inside the requested range and scope.
    pub available_reports: i64,
    /// Observed-only totals, never an estimate for missing reports; null if no reports.
    pub available_total: AnalysisTotals,
    pub invalid_reports: i64,
    pub duplicate_issues: i64,
    pub overlap_issues: i64,
    pub import_warning_reports: i64,
}
#[derive(Debug, Serialize)]
pub struct DailyRollupPoint {
    pub start: String,
    pub days: i64,
    pub missing: i64,
    pub available_reports: i64,
    pub complete: bool,
    pub total: AnalysisTotals,
    pub available_total: AnalysisTotals,
}
#[derive(Debug, Serialize)]
pub struct DailyRollupPage {
    pub granularity: String,
    pub points: Vec<DailyRollupPoint>,
    pub next_cursor: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct TimelinePoint {
    pub start: String,
    pub periods: i64,
    pub missing: i64,
    pub total: AnalysisTotals,
}
#[derive(Debug, Serialize)]
pub struct TimelineSummary {
    pub granularity: String,
    pub points: Vec<TimelinePoint>,
}
#[derive(Debug, Serialize)]
pub struct WorkTrendSummary {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub granularity: String,
    pub points: Vec<TimelinePoint>,
}
#[derive(Debug, Serialize)]
pub struct OrganizationTrendPoint {
    pub start: String,
    pub periods: i64,
    pub missing: i64,
    pub total: AnalysisTotals,
}
#[derive(Debug, Serialize)]
pub struct OrganizationTrendSeries {
    pub organization: String,
    pub points: Vec<OrganizationTrendPoint>,
}
#[derive(Debug, Serialize)]
pub struct OrganizationTrends {
    pub granularity: String,
    pub selected_organizations: i64,
    pub series: Vec<OrganizationTrendSeries>,
}
#[derive(Debug, Serialize)]
pub struct RankingSummary {
    pub complete: bool,
    pub rows: Vec<RangeValue>,
}
#[derive(Debug, Serialize)]
pub struct RangeValue {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub count: Option<f64>,
    pub amount: Option<f64>,
    pub average: Option<f64>,
    pub active: bool,
}
#[derive(Debug, Serialize)]
pub struct MissingPeriod {
    pub org: String,
    pub start: String,
}
#[derive(Debug, Serialize)]
pub struct OrganizationSummary {
    pub org: String,
    pub complete: bool,
    pub missing: i64,
    pub total: AnalysisTotals,
}
#[derive(Debug, Serialize)]
pub struct AnalysisIssue {
    pub reason_code: Option<String>,
    pub report_id: String,
    pub org: String,
    pub kind: String,
    pub other_report_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueCursor {
    pub report_id: String,
    pub org: String,
    pub kind: String,
    pub other_report_id: String,
}
impl AnalysisIssue {
    pub fn cursor(&self) -> IssueCursor {
        IssueCursor {
            report_id: self.report_id.clone(),
            org: self.org.clone(),
            kind: self.kind.clone(),
            other_report_id: self.other_report_id.clone(),
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeMeasure {
    pub base: f64,
    pub current: f64,
    pub delta: f64,
    pub percent: Option<f64>,
    pub negative: bool,
    pub small_base: bool,
    pub notable: bool,
    pub significant: bool,
}
#[derive(Debug, Serialize)]
pub struct AverageChange {
    pub base: Option<f64>,
    pub current: Option<f64>,
    pub delta: Option<f64>,
    pub percent: Option<f64>,
    #[serde(flatten)]
    pub flags: Option<AverageFlags>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AverageFlags {
    pub negative: bool,
    pub small_base: bool,
    pub notable: bool,
    pub significant: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeCursor {
    pub current_start: String,
    pub org: String,
    pub key: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeValue {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub org: String,
    pub base_start: String,
    pub current_start: String,
    pub status: String,
    pub count: ChangeMeasure,
    pub amount: ChangeMeasure,
    pub average: AverageChange,
}
impl ChangeValue {
    pub fn cursor(&self) -> ChangeCursor {
        ChangeCursor {
            current_start: self.current_start.clone(),
            org: self.org.clone(),
            key: self.key.clone(),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct ChangesSummary {
    pub count: ChangeMeasure,
    pub amount: ChangeMeasure,
    pub average: AverageChange,
    pub rows: i64,
    pub new: i64,
    pub gone: i64,
    pub existing: i64,
    pub missing_pairs: i64,
    pub import_warning_pairs: i64,
    pub invalid_reports: i64,
    pub duplicate_issues: i64,
    pub overlap_issues: i64,
}
#[derive(Debug, Serialize)]
pub struct ComparisonValue {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub status: Option<String>,
    pub count: Option<ChangeMeasure>,
    pub amount: Option<ChangeMeasure>,
    pub average: AverageChange,
}

fn bounded(s: &str) -> Result<(), String> {
    if s.len() > BOUND {
        Err("Analysis label/cursor exceeds 64 KiB".into())
    } else {
        Ok(())
    }
}
fn key_bound(s: &str) -> Result<(), String> {
    if s.len() > BOUND + 7 {
        Err("Analysis key cursor exceeds 64 KiB plus generated prefix".into())
    } else {
        Ok(())
    }
}
fn cap(n: usize) -> Result<i64, String> {
    if (1..=500).contains(&n) {
        Ok(n as i64)
    } else {
        Err("Analysis page/batch limit must be 1..=500".into())
    }
}
fn finite(n: f64) -> Result<f64, String> {
    if n.is_finite() {
        Ok(n)
    } else {
        Err("Analysis numeric overflow".into())
    }
}
fn totals(count: f64, amount: f64, complete: bool) -> Result<AnalysisTotals, String> {
    finite(count)?;
    finite(amount)?;
    Ok(AnalysisTotals {
        count: complete.then_some(count),
        amount: complete.then_some(amount),
        average: if complete && count > 0. {
            Some(finite(amount / count)?)
        } else {
            None
        },
    })
}
fn date(s: &str) -> Result<NaiveDate, String> {
    if !valid_date(s) {
        return Err("Invalid analysis date".into());
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(err)
}
fn shift(kind: &str, d: NaiveDate, n: i32) -> Result<NaiveDate, String> {
    let out = if kind == "month" {
        if n >= 0 {
            d.checked_add_months(Months::new(n as u32))
        } else {
            d.checked_sub_months(Months::new(n.unsigned_abs()))
        }
    } else {
        d.checked_add_signed(Duration::days(
            i64::from(n) * if kind == "week" { 7 } else { 1 },
        ))
    };
    out.filter(|d| (0..=9999).contains(&d.year()))
        .ok_or_else(|| "Analysis period exceeds supported calendar".into())
}
fn end(kind: &str, start: NaiveDate) -> Result<NaiveDate, String> {
    if kind == "month" {
        shift(kind, start, 1)?
            .checked_sub_signed(Duration::days(1))
            .ok_or("Invalid period end".into())
    } else {
        start
            .checked_add_signed(Duration::days(if kind == "week" { 6 } else { 0 }))
            .ok_or("Invalid period end".into())
    }
}
fn valid_bounds(kind: &str, start: &str, stop: &str) -> bool {
    date(start)
        .and_then(|d| {
            if kind == "month" && d.day() != 1 {
                return Err("month start".into());
            }
            Ok(end(kind, d)?.to_string() == stop)
        })
        .unwrap_or(false)
}
fn range_bounds(kind: &str, q: &RangeQuery) -> Result<(NaiveDate, NaiveDate), String> {
    let a = date(&q.from)?;
    let b = date(&q.to)?;
    if a > b
        || !REPORT_TYPES.contains(&kind)
        || (kind == "month" && (a.day() != 1 || b.day() != 1))
        || (kind == "week" && (b - a).num_days() % 7 != 0)
    {
        return Err(
            "Invalid analysis range: endpoints must be period starts in one exact sequence".into(),
        );
    }
    Ok((a, b))
}

// Scalars only: unknown composite fields are consumed without allocating a Value.
#[derive(Default)]
enum Scalar {
    Text(String),
    Num(f64),
    Bool(bool),
    #[default]
    Other,
    Composite,
}
impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Scalar;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON value")
            }
            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Scalar, E> {
                Ok(Scalar::Text(s.into()))
            }
            fn visit_string<E: serde::de::Error>(self, s: String) -> Result<Scalar, E> {
                Ok(Scalar::Text(s))
            }
            fn visit_f64<E: serde::de::Error>(self, n: f64) -> Result<Scalar, E> {
                Ok(Scalar::Num(n))
            }
            fn visit_i64<E: serde::de::Error>(self, n: i64) -> Result<Scalar, E> {
                Ok(Scalar::Num(n as f64))
            }
            fn visit_u64<E: serde::de::Error>(self, n: u64) -> Result<Scalar, E> {
                Ok(Scalar::Num(n as f64))
            }
            fn visit_bool<E: serde::de::Error>(self, b: bool) -> Result<Scalar, E> {
                Ok(Scalar::Bool(b))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Scalar, E> {
                Ok(Scalar::Other)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Scalar, A::Error> {
                while a.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Scalar::Composite)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Scalar, A::Error> {
                while a.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(Scalar::Composite)
            }
        }
        d.deserialize_any(V)
    }
}
impl Scalar {
    fn text(self) -> Option<String> {
        if let Self::Text(s) = self {
            Some(s)
        } else {
            None
        }
    }
    fn number(self) -> Option<f64> {
        if let Self::Num(n) = self {
            Some(n)
        } else {
            None
        }
    }
    fn truthy(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Text(s) => !s.is_empty(),
            Self::Num(n) => *n != 0.,
            Self::Composite => true,
            _ => false,
        }
    }
}
#[derive(Default)]
struct ParsedRow {
    org: Option<String>,
    work: Option<String>,
    count: Option<f64>,
    amount: Option<f64>,
}
impl<'de> Deserialize<'de> for ParsedRow {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = ParsedRow;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("report row")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<ParsedRow, A::Error> {
                let mut r = ParsedRow::default();
                let mut seen = [false; 4];
                while let Some(k) = a.next_key::<String>()? {
                    if let Some(i) = ["organization", "work", "count", "amount"]
                        .iter()
                        .position(|x| *x == k)
                    {
                        if seen[i] {
                            return Err(serde::de::Error::custom("Duplicate row field"));
                        }
                        seen[i] = true;
                        let v = a.next_value::<Scalar>()?;
                        match i {
                            0 => r.org = v.text(),
                            1 => r.work = v.text(),
                            2 => r.count = v.number(),
                            _ => r.amount = v.number(),
                        }
                    } else {
                        a.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(r)
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<ParsedRow, E> {
                Ok(ParsedRow::default())
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<ParsedRow, E> {
                Ok(ParsedRow::default())
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<ParsedRow, E> {
                Ok(ParsedRow::default())
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<ParsedRow, E> {
                Ok(ParsedRow::default())
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<ParsedRow, E> {
                Ok(ParsedRow::default())
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<ParsedRow, E> {
                Ok(ParsedRow::default())
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<ParsedRow, A::Error> {
                while a.next_element::<IgnoredAny>()?.is_some() {}
                Ok(ParsedRow::default())
            }
        }
        d.deserialize_any(V)
    }
}
struct Rows<'a>(&'a Connection);
impl<'de> DeserializeSeed<'de> for Rows<'_> {
    type Value = bool;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<bool, D::Error> {
        struct V<'a>(&'a Connection);
        impl<'de> Visitor<'de> for V<'_> {
            type Value = bool;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("rows array")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<bool, A::Error> {
                use serde::de::Error;
                let mut n = 0i64;
                let mut valid = true;
                let mut stmt = self
                    .0
                    .prepare("INSERT INTO temp.an_row VALUES(?1,?2,?3,?4,?5)")
                    .map_err(A::Error::custom)?;
                while let Some(r) = a.next_element::<ParsedRow>()? {
                    match (r.work, r.count, r.amount) {
                        (Some(w), Some(c), Some(m)) if c.is_finite() && m.is_finite() => {
                            if let Some(o) = r.org {
                                bounded(&w)
                                    .and_then(|_| bounded(&o))
                                    .map_err(A::Error::custom)?;
                                self.0
                                    .execute("INSERT OR IGNORE INTO temp.an_org VALUES(?1)", [&o])
                                    .map_err(A::Error::custom)?;
                                stmt.execute(params![n, o, w, c, m])
                                    .map_err(A::Error::custom)?;
                            } else {
                                // Old JS ignores these rows for string organization selections.
                                // Do not certify the reduced rows as a complete D1 raw index.
                                self.0
                                    .execute("INSERT OR IGNORE INTO temp.an_omitted VALUES(1)", [])
                                    .map_err(A::Error::custom)?;
                            }
                        }
                        _ => valid = false,
                    }
                    n = n
                        .checked_add(1)
                        .ok_or_else(|| A::Error::custom("Too many rows"))?;
                }
                Ok(valid)
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<bool, E> {
                Ok(false)
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<bool, E> {
                Ok(false)
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<bool, E> {
                Ok(false)
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<bool, E> {
                Ok(false)
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<bool, E> {
                Ok(false)
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<bool, E> {
                Ok(false)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<bool, A::Error> {
                while a.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(false)
            }
        }
        d.deserialize_any(V(self.0))
    }
}
struct Orgs<'a>(&'a Connection);
impl<'de> DeserializeSeed<'de> for Orgs<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        struct V<'a>(&'a Connection);
        impl<'de> Visitor<'de> for V<'_> {
            type Value = ();
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("organization array")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
                use serde::de::Error;
                while let Some(v) = a.next_element::<Scalar>()? {
                    if let Some(s) = v.text() {
                        bounded(&s).map_err(A::Error::custom)?;
                        self.0
                            .execute("INSERT OR IGNORE INTO temp.an_org VALUES(?1)", [s])
                            .map_err(A::Error::custom)?;
                    }
                }
                Ok(())
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
                Ok(())
            }
        }
        d.deserialize_any(V(self.0))
    }
}
struct Warnings;
impl<'de> DeserializeSeed<'de> for Warnings {
    type Value = bool;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<bool, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = bool;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("warning array or null")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<bool, A::Error> {
                let mut any = false;
                while a.next_element::<IgnoredAny>()?.is_some() {
                    any = true
                }
                Ok(any)
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<bool, E> {
                Ok(false)
            }
            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<bool, E> {
                Ok(!s.is_empty())
            }
        }
        d.deserialize_any(V)
    }
}
struct Report<'a> {
    c: &'a Connection,
    metadata: [&'a str; 4],
}
impl<'de> DeserializeSeed<'de> for Report<'_> {
    type Value = (bool, bool);
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        struct V<'a>(Report<'a>);
        impl<'de> Visitor<'de> for V<'_> {
            type Value = (bool, bool);
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("report object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                use serde::de::Error;
                let mut seen = [false; 8];
                let mut valid = false;
                let mut warned = false;
                while let Some(k) = a.next_key::<String>()? {
                    if let Some(i) = [
                        "id",
                        "type",
                        "start",
                        "end",
                        "rows",
                        "organizations",
                        "warnings",
                        "hasMismatch",
                    ]
                    .iter()
                    .position(|x| *x == k)
                    {
                        if seen[i] {
                            return Err(A::Error::custom("Duplicate report field"));
                        }
                        seen[i] = true;
                        match i {
                            0..=3 => {
                                let s = a.next_value::<String>()?;
                                if s != self.0.metadata[i] {
                                    return Err(A::Error::custom(
                                        "JSON metadata differs from columns",
                                    ));
                                }
                            }
                            4 => valid = a.next_value_seed(Rows(self.0.c))?,
                            5 => a.next_value_seed(Orgs(self.0.c))?,
                            6 => warned |= a.next_value_seed(Warnings)?,
                            _ => warned |= a.next_value::<Scalar>()?.truthy(),
                        }
                    } else {
                        a.next_value::<IgnoredAny>()?;
                    }
                }
                if seen[..4].iter().any(|x| !*x) {
                    return Err(A::Error::custom("Missing report metadata"));
                }
                Ok((valid, warned))
            }
        }
        d.deserialize_map(V(self))
    }
}
fn parse<R: Read, S, T>(reader: R, seed: S) -> Result<T, String>
where
    S: for<'de> DeserializeSeed<'de, Value = T>,
{
    let bounded = super::read_model::TokenBounded::new(reader);
    let mut de = serde_json::Deserializer::from_reader(BufReader::with_capacity(8192, bounded));
    let result = seed.deserialize(&mut de).map_err(err)?;
    de.end().map_err(err)?;
    Ok(result)
}
// Group work lists stage on disk even when they precede id/name in JSON.
struct Works<'a> {
    c: &'a Connection,
    n: i64,
}
impl<'de> DeserializeSeed<'de> for Works<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        struct V<'a>(Works<'a>);
        impl<'de> Visitor<'de> for V<'_> {
            type Value = ();
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("group work array")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
                use serde::de::Error;
                while let Some(s) = a.next_element::<String>()? {
                    bounded(&s).map_err(A::Error::custom)?;
                    self.0
                        .c
                        .execute(
                            "INSERT OR IGNORE INTO temp.an_members VALUES(?1,?2)",
                            params![self.0.n, s],
                        )
                        .map_err(A::Error::custom)?;
                }
                Ok(())
            }
        }
        d.deserialize_seq(V(self))
    }
}
struct Group<'a> {
    c: &'a Connection,
    n: i64,
}
impl<'de> DeserializeSeed<'de> for Group<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        struct V<'a>(Group<'a>);
        impl<'de> Visitor<'de> for V<'_> {
            type Value = ();
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("group object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
                use serde::de::Error;
                let mut id = None;
                let mut name = None;
                let mut works = false;
                while let Some(k) = a.next_key::<String>()? {
                    match k.as_str() {
                        "id" if id.is_none() => id = Some(a.next_value::<String>()?),
                        "name" if name.is_none() => name = Some(a.next_value::<String>()?),
                        "works" if !works => {
                            a.next_value_seed(Works {
                                c: self.0.c,
                                n: self.0.n,
                            })?;
                            works = true
                        }
                        "id" | "name" | "works" => {
                            return Err(A::Error::custom("Duplicate group field"))
                        }
                        _ => {
                            a.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                let id = id.ok_or_else(|| A::Error::custom("Missing group id"))?;
                let name = name.ok_or_else(|| A::Error::custom("Missing group name"))?;
                bounded(&id)
                    .and_then(|_| bounded(&name))
                    .map_err(A::Error::custom)?;
                if !works {
                    return Err(A::Error::custom("Missing group works"));
                }
                self.0
                    .c
                    .execute(
                        "INSERT INTO temp.an_descriptors VALUES(?1,?2,?3)",
                        params![self.0.n, id, name],
                    )
                    .map_err(A::Error::custom)?;
                Ok(())
            }
        }
        d.deserialize_map(V(self))
    }
}
struct Groups<'a>(&'a Connection);
impl<'de> DeserializeSeed<'de> for Groups<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        struct V<'a>(&'a Connection);
        impl<'de> Visitor<'de> for V<'_> {
            type Value = ();
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("groups array")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
                use serde::de::Error;
                let mut n = 0i64;
                while a.next_element_seed(Group { c: self.0, n })?.is_some() {
                    n = n
                        .checked_add(1)
                        .ok_or_else(|| A::Error::custom("Too many groups"))?;
                }
                Ok(())
            }
        }
        d.deserialize_seq(V(self.0))
    }
}
struct Settings<'a>(&'a Connection);
impl<'de> DeserializeSeed<'de> for Settings<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        struct V<'a>(&'a Connection);
        impl<'de> Visitor<'de> for V<'_> {
            type Value = ();
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("settings object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
                let mut seen = false;
                while let Some(k) = a.next_key::<String>()? {
                    if k == "groups" {
                        if seen {
                            return Err(serde::de::Error::custom("Duplicate groups field"));
                        }
                        seen = true;
                        a.next_value_seed(Groups(self.0))?;
                    } else {
                        a.next_value::<IgnoredAny>()?;
                    }
                }
                if !seen {
                    return Err(serde::de::Error::custom("Missing settings groups"));
                }
                Ok(())
            }
        }
        d.deserialize_map(V(self.0))
    }
}

impl Storage {
    /// One report per call. Parsing stages in FILE temp tables, publishing by revision CAS.
    pub fn analysis_index_report(&self, id: &str, expected: i64) -> Result<(), String> {
        self.analysis_index_report_before_publish(id, expected, || {})
    }
    pub(super) fn analysis_index_report_before_publish(
        &self,
        id: &str,
        expected: i64,
        before: impl FnOnce(),
    ) -> Result<(), String> {
        bounded(id)?;
        let mut c = self.connect()?;
        c.execute_batch("PRAGMA temp_store=FILE;PRAGMA temp.cache_size=-2048;CREATE TEMP TABLE an_row(n INTEGER PRIMARY KEY,org TEXT,work TEXT,count REAL,amount REAL);CREATE TEMP TABLE an_org(org TEXT PRIMARY KEY) WITHOUT ROWID;CREATE TEMP TABLE an_omitted(singleton INTEGER PRIMARY KEY);").map_err(err)?;
        let tx = c.transaction().map_err(err)?;
        check_revision(&tx, expected)?;
        check_metadata_bounds(&tx)?;
        let (rowid, kind, start, stop): (i64, String, String, String) = tx
            .query_row(
                "SELECT rowid,report_type,period_start,period_end FROM reports WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(err)?
            .ok_or("Report not found")?;
        if !REPORT_TYPES.contains(&kind.as_str()) {
            return Err("Unsupported report type".into());
        }
        let (rows_valid, warned) = parse(
            tx.blob_open("main", "reports", "json", rowid, true)
                .map_err(err)?,
            Report {
                c: &tx,
                metadata: [id, &kind, &start, &stop],
            },
        )?;
        let valid = valid_bounds(&kind, &start, &stop) && rows_valid;
        before();
        check_revision(&tx, expected)?;
        tx.execute("DELETE FROM analysis_reports WHERE report_id=?1", [id])
            .map_err(err)?;
        tx.execute("DELETE FROM analysis_orgs WHERE report_id=?1", [id])
            .map_err(err)?;
        tx.execute(
            "INSERT INTO analysis_orgs SELECT ?1,org FROM temp.an_org",
            [id],
        )
        .map_err(err)?;
        if valid {
            tx.execute("DELETE FROM report_rows WHERE report_id=?1", [id])
                .map_err(err)?;
            tx.execute(
                "INSERT INTO report_rows SELECT ?1,n,org,work,count,amount FROM temp.an_row",
                [id],
            )
            .map_err(err)?;
            tx.execute("DELETE FROM report_row_index WHERE report_id=?1", [id])
                .map_err(err)?;
            tx.execute("INSERT INTO report_row_index SELECT ?1,(SELECT COUNT(*) FROM temp.an_row) WHERE NOT EXISTS(SELECT 1 FROM temp.an_omitted)",[id]).map_err(err)?;
        }
        tx.execute(
            "INSERT INTO analysis_reports VALUES(?1,?2,?3,?4)",
            params![
                id,
                valid,
                if valid {
                    None
                } else if !rows_valid {
                    Some("invalid_rows")
                } else {
                    Some("invalid_bounds")
                },
                warned
            ],
        )
        .map_err(err)?;
        tx.commit().map_err(err)
    }
    pub fn analysis_index_settings(&self, expected: i64) -> Result<(), String> {
        let mut c = self.connect()?;
        c.execute_batch("PRAGMA temp_store=FILE;PRAGMA temp.cache_size=-2048;CREATE TEMP TABLE an_members(n INTEGER,work TEXT,PRIMARY KEY(n,work)) WITHOUT ROWID;CREATE TEMP TABLE an_descriptors(n INTEGER PRIMARY KEY,id TEXT,name TEXT);").map_err(err)?;
        let tx = c.transaction().map_err(err)?;
        check_revision(&tx, expected)?;
        let rowid: Option<i64> = tx
            .query_row("SELECT rowid FROM kv WHERE name='settings'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(err)?;
        if let Some(rowid) = rowid {
            parse(
                tx.blob_open("main", "kv", "json", rowid, true)
                    .map_err(err)?,
                Settings(&tx),
            )?;
        }
        let ambiguous:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM temp.an_descriptors GROUP BY id HAVING COUNT(DISTINCT name)>1)",[],|r|r.get(0)).map_err(err)?;
        if ambiguous {
            return Err(
                "Unsupported settings: duplicate group id with different display names".into(),
            );
        }
        tx.execute("DELETE FROM analysis_groups", []).map_err(err)?;
        tx.execute_batch("INSERT OR IGNORE INTO analysis_groups SELECT m.work,'group:'||d.id,d.name FROM temp.an_members m JOIN temp.an_descriptors d ON d.n=m.n ORDER BY m.n;INSERT OR REPLACE INTO analysis_settings VALUES(1);").map_err(err)?;
        tx.commit().map_err(err)
    }
    pub fn analysis_filter_create(&self, expected: i64, kind: &str) -> Result<String, String> {
        if !matches!(kind, "organizations" | "names") {
            return Err("Unknown analysis filter kind".into());
        }
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        check_revision(&tx, expected)?;
        let id: String = tx
            .query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))
            .map_err(err)?;
        tx.execute(
            "INSERT INTO analysis_filters VALUES(?1,?2,?3,0)",
            params![id, expected, kind],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(id)
    }
    pub fn analysis_filter_append(
        &self,
        expected: i64,
        id: &str,
        items: &[String],
    ) -> Result<(), String> {
        bounded(id)?;
        cap(items.len())?;
        for s in items {
            bounded(s)?;
        }
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        check_revision(&tx, expected)?;
        let open:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM analysis_filters WHERE id=?1 AND revision=?2 AND sealed=0)",params![id,expected],|r|r.get(0)).map_err(err)?;
        if !open {
            return Err("Analysis filter absent, stale, or sealed".into());
        }
        let mut ordinal: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(ordinal)+1,0) FROM analysis_filter_items WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .map_err(err)?;
        for item in items {
            tx.execute(
                "INSERT OR IGNORE INTO analysis_filter_items VALUES(?1,?2,?3)",
                params![id, item, ordinal],
            )
            .map_err(err)?;
            ordinal = ordinal.checked_add(1).ok_or("Filter ordinal overflow")?;
        }
        tx.commit().map_err(err)
    }
    pub fn analysis_filter_seal(&self, expected: i64, id: &str) -> Result<(), String> {
        bounded(id)?;
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        check_revision(&tx, expected)?;
        if tx
            .execute(
                "UPDATE analysis_filters SET sealed=1 WHERE id=?1 AND revision=?2",
                params![id, expected],
            )
            .map_err(err)?
            != 1
        {
            return Err("Unknown or stale analysis filter".into());
        }
        tx.commit().map_err(err)
    }
    pub fn analysis_filter_delete(&self, id: &str) -> Result<(), String> {
        bounded(id)?;
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        tx.execute("DELETE FROM analysis_filter_items WHERE id=?1", [id])
            .map_err(err)?;
        tx.execute("DELETE FROM analysis_filters WHERE id=?1", [id])
            .map_err(err)?;
        tx.commit().map_err(err)
    }
}
fn filter_check(c: &Connection, id: &str, rev: i64, kind: &str) -> Result<(), String> {
    bounded(id)?;
    let ok:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM analysis_filters WHERE id=?1 AND revision=?2 AND kind=?3 AND sealed=1)",params![id,rev,kind],|r|r.get(0)).map_err(err)?;
    if ok {
        Ok(())
    } else {
        Err("Analysis filter absent, stale, unsealed, or wrong kind".into())
    }
}
pub(super) const CONFLICT_SQL:&str="INSERT INTO temp.an_issues SELECT a.report_id,a.org,CASE WHEN a.start=b.start AND a.end=b.end THEN 'duplicate' ELSE 'overlap' END,b.report_id FROM temp.an_candidates a JOIN temp.an_candidates b ON a.org=b.org AND b.start BETWEEN date(a.start,?1) AND a.end AND a.start<=b.end AND a.report_id<>b.report_id";
// Connection-local relational workspaces are deliberately on disk; no history Vec.
fn workspace(c: &Connection, expected: i64, s: &AnalysisScope) -> Result<(), String> {
    check_revision(c, expected)?;
    check_metadata_bounds(c)?;
    if !REPORT_TYPES.contains(&s.report_type.as_str()) {
        return Err("Invalid analysis report type".into());
    }
    filter_check(c, &s.organizations, expected, "organizations")?;
    if let Some(id) = &s.names {
        filter_check(c, id, expected, "names")?;
    }
    let missing:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM reports r LEFT JOIN analysis_reports a ON a.report_id=r.id WHERE r.report_type=?1 AND a.report_id IS NULL)",[&s.report_type],|r|r.get(0)).map_err(err)?;
    if missing {
        return Err(
            "Analysis contains unindexed history; index every report of the selected type".into(),
        );
    }
    let settings: bool = c
        .query_row("SELECT EXISTS(SELECT 1 FROM analysis_settings)", [], |r| {
            r.get(0)
        })
        .map_err(err)?;
    if !settings {
        return Err("Analysis settings unindexed; backfill settings".into());
    }
    c.execute_batch("PRAGMA temp_store=FILE;PRAGMA temp.cache_size=-2048;
 CREATE TEMP TABLE an_candidates(org TEXT,start TEXT,end TEXT,report_id TEXT,warned INTEGER,PRIMARY KEY(org,start,report_id)) WITHOUT ROWID;
 CREATE TEMP TABLE an_issues(report_id TEXT,org TEXT,kind TEXT,other_report_id TEXT,PRIMARY KEY(report_id,org,kind,other_report_id)) WITHOUT ROWID;
 CREATE TEMP TABLE an_valid(org TEXT,start TEXT,end TEXT,report_id TEXT,warned INTEGER,PRIMARY KEY(org,start)) WITHOUT ROWID;
 CREATE TEMP TABLE an_periods(slot INTEGER,start TEXT,PRIMARY KEY(slot,start)) WITHOUT ROWID;
 CREATE TEMP TABLE an_values(slot INTEGER,org TEXT,key TEXT,name TEXT,kind TEXT,count REAL,amount REAL,active INTEGER,PRIMARY KEY(slot,org,key)) WITHOUT ROWID;
 CREATE TEMP TABLE an_missing(slot INTEGER,org TEXT,start TEXT,PRIMARY KEY(slot,org,start)) WITHOUT ROWID;
 CREATE TEMP TABLE an_combined(slot INTEGER,key TEXT,name TEXT,kind TEXT,count REAL,amount REAL,active INTEGER,PRIMARY KEY(slot,key)) WITHOUT ROWID;").map_err(err)?;
    c.execute("INSERT INTO temp.an_candidates SELECT o.org,r.period_start,r.period_end,r.id,a.warned FROM reports r JOIN analysis_reports a ON a.report_id=r.id AND a.valid=1 JOIN analysis_orgs o ON o.report_id=r.id JOIN analysis_filter_items f ON f.id=?1 AND f.item=o.org WHERE r.report_type=?2",params![s.organizations,s.report_type]).map_err(err)?;
    c.execute("INSERT INTO temp.an_issues SELECT r.id,'','invalid','' FROM reports r JOIN analysis_reports a ON a.report_id=r.id WHERE r.report_type=?1 AND a.valid=0",[&s.report_type]).map_err(err)?;
    // A valid day/month can overlap only the same start; a seven-day window
    // can start at most six days earlier. Constrain BOTH B-tree range ends.
    c.execute(
        CONFLICT_SQL,
        [if s.report_type == "week" {
            "-6 day"
        } else {
            "0 day"
        }],
    )
    .map_err(err)?;
    c.execute("INSERT INTO temp.an_valid SELECT * FROM temp.an_candidates a WHERE NOT EXISTS(SELECT 1 FROM temp.an_issues i WHERE i.report_id=a.report_id AND i.org=a.org)",[]).map_err(err)?;
    Ok(())
}
fn fill_range(c: &Connection, s: &AnalysisScope, q: &RangeQuery, slot: i64) -> Result<(), String> {
    let (mut d, b) = range_bounds(&s.report_type, q)?;
    let mut insert = c
        .prepare("INSERT INTO temp.an_periods VALUES(?1,?2)")
        .map_err(err)?;
    loop {
        insert.execute(params![slot, d.to_string()]).map_err(err)?;
        if d == b {
            break;
        }
        d = shift(&s.report_type, d, 1)?;
    }
    drop(insert);
    c.execute("INSERT INTO temp.an_missing SELECT ?1,f.item,p.start FROM analysis_filter_items f CROSS JOIN temp.an_periods p LEFT JOIN temp.an_valid v ON v.org=f.item AND v.start=p.start WHERE f.id=?2 AND p.slot=?1 AND v.report_id IS NULL",params![slot,s.organizations]).map_err(err)?;
    c.execute("INSERT INTO temp.an_values SELECT ?1,v.org,COALESCE(g.key,'work:'||x.work),COALESCE(g.name,x.work),CASE WHEN g.key IS NULL THEN 'work' ELSE 'group' END,SUM(x.count),SUM(x.amount),MAX(x.count<>0 OR x.amount<>0) FROM temp.an_valid v JOIN temp.an_periods p ON p.slot=?1 AND p.start=v.start JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org LEFT JOIN analysis_groups g ON g.work=x.work WHERE (?2 IS NULL OR EXISTS(SELECT 1 FROM analysis_filter_items f WHERE f.id=?2 AND f.item=COALESCE(g.name,x.work))) GROUP BY v.org,COALESCE(g.key,'work:'||x.work)",params![slot,s.names]).map_err(err)?;
    c.execute("INSERT INTO temp.an_combined SELECT slot,key,name,kind,SUM(count),SUM(amount),MAX(active) FROM temp.an_values WHERE slot=?1 GROUP BY key",[slot]).map_err(err)?;
    validate_numbers(c, "SELECT count,amount FROM temp.an_combined")?;
    validate_numbers(c, "SELECT count,amount FROM temp.an_values")?;
    validate_numbers(
        c,
        "SELECT SUM(count),SUM(amount) FROM temp.an_values GROUP BY slot,org",
    )?;
    validate_numbers(
        c,
        "SELECT SUM(count),SUM(amount) FROM temp.an_combined GROUP BY slot",
    )?;
    Ok(())
}
fn validate_numbers(c: &Connection, sql: &str) -> Result<(), String> {
    let mut stmt = c.prepare(sql).map_err(err)?;
    let mut rows = stmt.query([]).map_err(err)?;
    while let Some(r) = rows.next().map_err(err)? {
        for column in 0..2 {
            finite(
                r.get::<_, Option<f64>>(column)
                    .map_err(err)?
                    .ok_or("Analysis numeric overflow")?,
            )?;
        }
    }
    Ok(())
}
fn count_sql(c: &Connection, sql: &str) -> Result<i64, String> {
    c.query_row(sql, [], |r| r.get(0)).map_err(err)
}
fn complete(c: &Connection, slot: i64, org: Option<&str>) -> Result<bool, String> {
    c.query_row("SELECT EXISTS(SELECT 1 FROM temp.an_periods WHERE slot=?1) AND NOT EXISTS(SELECT 1 FROM temp.an_missing WHERE slot=?1 AND (?2 IS NULL OR org=?2)) AND EXISTS(SELECT 1 FROM temp.an_valid WHERE ?2 IS NULL OR org=?2)",params![slot,org],|r|r.get(0)).map_err(err)
}
fn range_summary(c: &Connection, s: &AnalysisScope, slot: i64) -> Result<RangeSummary, String> {
    let organizations: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM analysis_filter_items WHERE id=?1",
            [&s.organizations],
            |r| r.get(0),
        )
        .map_err(err)?;
    let missing: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM temp.an_missing WHERE slot=?1",
            [slot],
            |r| r.get(0),
        )
        .map_err(err)?;
    let periods: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM temp.an_periods WHERE slot=?1",
            [slot],
            |r| r.get(0),
        )
        .map_err(err)?;
    let (values,count,amount):(i64,f64,f64)=c.query_row("SELECT COUNT(*),COALESCE(SUM(count),0),COALESCE(SUM(amount),0) FROM temp.an_combined WHERE slot=?1",[slot],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(err)?;
    let complete = periods > 0 && organizations > 0 && missing == 0;
    let available_reports: i64 = c.query_row(
        "SELECT COUNT(*) FROM temp.an_valid v
         JOIN temp.an_periods p ON p.slot=?1 AND p.start=v.start",
        [slot],
        |r| r.get(0),
    ).map_err(err)?;
    Ok(RangeSummary {
        complete,
        periods,
        organizations,
        missing,
        values,
        total: totals(count, amount, complete)?,
        available_reports,
        available_total: totals(count, amount, available_reports > 0)?,
        invalid_reports: count_sql(
            c,
            "SELECT COUNT(*) FROM temp.an_issues WHERE kind='invalid'",
        )?,
        duplicate_issues: count_sql(
            c,
            "SELECT COUNT(*) FROM temp.an_issues WHERE kind='duplicate'",
        )?,
        overlap_issues: count_sql(
            c,
            "SELECT COUNT(*) FROM temp.an_issues WHERE kind='overlap'",
        )?,
        import_warning_reports: count_sql(
            c,
            "SELECT COUNT(DISTINCT report_id) FROM temp.an_candidates WHERE warned<>0",
        )?,
    })
}
fn org_check(c: &Connection, s: &AnalysisScope, org: Option<&str>) -> Result<(), String> {
    if let Some(o) = org {
        bounded(o)?;
        let ok: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM analysis_filter_items WHERE id=?1 AND item=?2)",
                params![s.organizations, o],
                |r| r.get(0),
            )
            .map_err(err)?;
        if !ok {
            return Err("Organization is outside analysis scope".into());
        }
    }
    Ok(())
}
fn range_rows(
    c: &Connection,
    slot: i64,
    org: Option<&str>,
    after: Option<&str>,
    n: i64,
) -> Result<Vec<RangeValue>, String> {
    if let Some(k) = after {
        key_bound(k)?;
    }
    let available = complete(c, slot, org)?;
    let sql = if org.is_some() {
        "SELECT key,name,kind,count,amount,active FROM temp.an_values WHERE slot=?1 AND org=?2 AND (?3 IS NULL OR key>?3) ORDER BY key LIMIT ?4"
    } else {
        "SELECT key,name,kind,count,amount,active FROM temp.an_combined WHERE slot=?1 AND ?2 IS NULL AND (?3 IS NULL OR key>?3) ORDER BY key LIMIT ?4"
    };
    let mut stmt = c.prepare(sql).map_err(err)?;
    let mut rows = stmt.query(params![slot, org, after, n]).map_err(err)?;
    let mut out = Vec::new();
    while let Some(r) = rows.next().map_err(err)? {
        let t = totals(r.get(3).map_err(err)?, r.get(4).map_err(err)?, available)?;
        out.push(RangeValue {
            key: r.get(0).map_err(err)?,
            name: r.get(1).map_err(err)?,
            kind: r.get(2).map_err(err)?,
            count: t.count,
            amount: t.amount,
            average: t.average,
            active: r.get(5).map_err(err)?,
        })
    }
    Ok(out)
}
impl Storage {
    /// Bounded data for the graphical timeline, with missing periods as nulls.
    /// Up to 120 source periods remain individual; longer selections are
    /// grouped by calendar month (at most 121 months in a 10-year UI range).
    pub fn analysis_timeline(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
    ) -> Result<TimelineSummary, String> {
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        fill_range(&tx, s, q, 1)?;
        let period_count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM temp.an_periods WHERE slot=1",
            [], |r| r.get(0),
        ).map_err(err)?;
        let monthly = period_count > 120;
        // Aggregate data and missing evidence in independent subqueries:
        // joining both to detail rows would multiply amounts.
        let sql = r#"
            WITH buckets AS (
                SELECT CASE WHEN ?1=1 THEN substr(start,1,7) ELSE start END AS label,
                       COUNT(*) AS periods
                FROM temp.an_periods WHERE slot=1 GROUP BY label
            ), missing AS (
                SELECT CASE WHEN ?1=1 THEN substr(start,1,7) ELSE start END AS label,
                       COUNT(*) AS missing
                FROM temp.an_missing WHERE slot=1 GROUP BY label
            ), values_by_bucket AS (
                SELECT CASE WHEN ?1=1 THEN substr(p.start,1,7) ELSE p.start END AS label,
                       SUM(x.count) AS count, SUM(x.amount) AS amount
                FROM temp.an_periods p
                JOIN temp.an_valid v ON v.start=p.start
                JOIN analysis_filter_items f ON f.id=?2 AND f.item=v.org
                JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org
                WHERE p.slot=1
                GROUP BY label
            )
            SELECT b.label,b.periods,COALESCE(m.missing,0),
                   COALESCE(v.count,0),COALESCE(v.amount,0)
            FROM buckets b
            LEFT JOIN missing m ON m.label=b.label
            LEFT JOIN values_by_bucket v ON v.label=b.label
            ORDER BY b.label
        "#;
        let mut stmt = tx.prepare(sql).map_err(err)?;
        let mut rows = stmt.query(params![i64::from(monthly), &s.organizations]).map_err(err)?;
        let mut points = Vec::new();
        while let Some(row) = rows.next().map_err(err)? {
            if points.len() >= 124 {
                return Err("Analysis timeline exceeds 124 points".into());
            }
            let missing: i64 = row.get(2).map_err(err)?;
            points.push(TimelinePoint {
                start: row.get(0).map_err(err)?,
                periods: row.get(1).map_err(err)?,
                missing,
                total: totals(row.get(3).map_err(err)?, row.get(4).map_err(err)?, missing == 0)?,
            });
        }
        Ok(TimelineSummary {
            granularity: if monthly { "month" } else { "period" }.into(),
            points,
        })
    }
    /// Up to eight individually verified organization series, up to 124 points
    /// each. All periods are generated by SQLite and incomplete buckets are
    /// null, even if some reports inside the bucket are valid.
    pub fn analysis_organization_trends(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
    ) -> Result<OrganizationTrends, String> {
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        fill_range(&tx, s, q, 1)?;
        let selected_organizations: i64 = tx.query_row(
            "SELECT COUNT(*) FROM analysis_filter_items WHERE id=?1",
            [&s.organizations],
            |r| r.get(0),
        ).map_err(err)?;
        let periods: i64 = tx.query_row(
            "SELECT COUNT(*) FROM temp.an_periods WHERE slot=1",
            [], |r| r.get(0),
        ).map_err(err)?;
        let monthly = periods > 120;
        // Missing evidence and values are aggregated separately, otherwise
        // a join on detail rows duplicates both missing counts and amounts.
        let sql = r#"
            WITH selected AS (
                SELECT item AS org,ordinal FROM analysis_filter_items
                WHERE id=?2 ORDER BY ordinal LIMIT 8
            ), buckets AS (
                SELECT CASE WHEN ?1=1 THEN substr(start,1,7) ELSE start END AS label,
                       COUNT(*) AS periods
                FROM temp.an_periods WHERE slot=1 GROUP BY label
            ), missing AS (
                SELECT org,
                       CASE WHEN ?1=1 THEN substr(start,1,7) ELSE start END AS label,
                       COUNT(*) AS missing
                FROM temp.an_missing WHERE slot=1
                GROUP BY org,label
            ), values_by_bucket AS (
                SELECT v.org,
                       CASE WHEN ?1=1 THEN substr(p.start,1,7) ELSE p.start END AS label,
                       SUM(x.count) AS count,SUM(x.amount) AS amount
                FROM temp.an_periods p
                JOIN temp.an_valid v ON v.start=p.start
                JOIN selected s ON s.org=v.org
                JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org
                WHERE p.slot=1
                GROUP BY v.org,label
            )
            SELECT s.org,b.label,b.periods,COALESCE(m.missing,0),
                   COALESCE(v.count,0),COALESCE(v.amount,0)
            FROM selected s CROSS JOIN buckets b
            LEFT JOIN missing m ON m.org=s.org AND m.label=b.label
            LEFT JOIN values_by_bucket v ON v.org=s.org AND v.label=b.label
            ORDER BY s.ordinal,b.label
        "#;
        let mut stmt = tx.prepare(sql).map_err(err)?;
        let mut rows = stmt.query(params![i64::from(monthly),&s.organizations]).map_err(err)?;
        let mut series: Vec<OrganizationTrendSeries> = Vec::new();
        let mut count = 0_usize;
        while let Some(row) = rows.next().map_err(err)? {
            count += 1;
            if count > 992 { return Err("Organization timeline exceeds 992 points".into()); }
            let org: String = row.get(0).map_err(err)?;
            if series.last().is_none_or(|s| s.organization != org) {
                if series.len() >= 8 { return Err("Too many organization series".into()); }
                series.push(OrganizationTrendSeries{organization:org.clone(),points:Vec::new()});
            }
            let missing: i64 = row.get(3).map_err(err)?;
            let current = series.last_mut().ok_or("Missing organization series")?;
            if current.points.len() >= 124 {
                return Err("Organization series exceeds 124 points".into());
            }
            current.points.push(OrganizationTrendPoint {
                start:row.get(1).map_err(err)?,
                periods:row.get(2).map_err(err)?,
                missing,
                total:totals(row.get(4).map_err(err)?,row.get(5).map_err(err)?,missing==0)?,
            });
        }
        Ok(OrganizationTrends{
            granularity: if monthly {"month"} else {"period"}.into(),
            selected_organizations,
            series,
        })
    }
    /// Day-source only calendar rollups. A report from a different source
    /// type is never counted, so uploaded week/month sources cannot double
    /// the same daily values. Keyset pagination bounds IPC to <=61 rows.
    pub fn analysis_daily_rollup(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        granularity: &str,
        after: Option<&str>,
        page_size: usize,
    ) -> Result<DailyRollupPage, String> {
        if s.report_type != "day" {
            return Err("Daily rollup requires indexed day reports".into());
        }
        if !matches!(granularity, "day"|"week"|"month"|"year") {
            return Err("Unknown daily rollup granularity".into());
        }
        if !(1..=60).contains(&page_size) {
            return Err("Daily rollup page must contain 1..=60 periods".into());
        }
        let (from,to) = range_bounds("day",q)?;
        match granularity {
            "week" if from.weekday()!=chrono::Weekday::Mon || to.weekday()!=chrono::Weekday::Sun =>
                return Err("Select complete Monday-through-Sunday weeks".into()),
            "month" if from.day()!=1 ||
                to.succ_opt().is_none_or(|d|d.day()!=1) =>
                return Err("Select complete calendar months".into()),
            "year" if from.month()!=1 || from.day()!=1 ||
                to.month()!=12 || to.day()!=31 =>
                return Err("Select complete calendar years".into()),
            _ => {}
        }
        if let Some(cursor) = after {
            if !date(cursor).is_ok() || cursor < q.from.as_str() || cursor > q.to.as_str() {
                return Err("Invalid daily rollup cursor".into());
            }
        }
        // The only dynamic SQL below is a fixed fragment selected by an enum.
        let bucket = match granularity {
            "day" => "start",
            "week" => "date(start, '-' || ((CAST(strftime('%w',start) AS INTEGER)+6)%7) || ' days')",
            "month" => "substr(start,1,7)||'-01'",
            _ => "substr(start,1,4)||'-01-01'",
        };
        let bucket_valid = bucket.replace("start", "v.start");
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        fill_range(&tx, s, q, 1)?;
        let sql = format!(
            "WITH buckets AS (
                SELECT {bucket} AS label, COUNT(*) AS days
                FROM temp.an_periods WHERE slot=1 GROUP BY label
            ), missing AS (
                SELECT {bucket} AS label, COUNT(*) AS absent
                FROM temp.an_missing WHERE slot=1 GROUP BY label
            ), observed AS (
                SELECT {bucket_valid} AS label, COUNT(*) AS reports
                FROM temp.an_valid v JOIN temp.an_periods p ON p.slot=1 AND p.start=v.start
                GROUP BY label
            ), sums AS (
                SELECT {bucket_valid} AS label,
                       COALESCE(SUM(x.count),0) AS total_count,
                       COALESCE(SUM(x.amount),0) AS total_amount
                FROM temp.an_valid v
                JOIN temp.an_periods p ON p.slot=1 AND p.start=v.start
                LEFT JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org
                GROUP BY label
            )
            SELECT b.label,b.days,COALESCE(m.absent,0),COALESCE(o.reports,0),
                   COALESCE(t.total_count,0),COALESCE(t.total_amount,0)
            FROM buckets b
            LEFT JOIN missing m ON m.label=b.label
            LEFT JOIN observed o ON o.label=b.label
            LEFT JOIN sums t ON t.label=b.label
            WHERE (?1 IS NULL OR b.label>?1)
            ORDER BY b.label LIMIT ?2"
        );
        let mut stmt=tx.prepare(&sql).map_err(err)?;
        let mut rows=stmt.query(params![after,(page_size+1) as i64]).map_err(err)?;
        let mut points=Vec::new();
        while let Some(row)=rows.next().map_err(err)? {
            if points.len()>page_size {return Err("Daily rollup result exceeds page limit".into());}
            let missing:i64=row.get(2).map_err(err)?;
            let available_reports:i64=row.get(3).map_err(err)?;
            let amount:f64=row.get(5).map_err(err)?;
            let count:f64=row.get(4).map_err(err)?;
            points.push(DailyRollupPoint {
                start:row.get(0).map_err(err)?,
                days:row.get(1).map_err(err)?,
                missing,
                available_reports,
                complete:missing==0 && available_reports>0,
                total:totals(count,amount,missing==0 && available_reports>0)?,
                available_total:totals(count,amount,available_reports>0)?,
            });
        }
        let next_cursor=if points.len()>page_size {
            points.pop();
            points.last().map(|p|p.start.clone())
        } else {None};
        Ok(DailyRollupPage{granularity:granularity.into(),points,next_cursor})
    }

    /// One selected work/group from the SQL detail view. An absent work in
    /// an otherwise complete period is an actual zero; a missing report
    /// invalidates its entire bucket. No full report history leaves SQLite.
    pub fn analysis_work_timeline(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        organization: Option<&str>,
        key: &str,
    ) -> Result<WorkTrendSummary, String> {
        key_bound(key)?;
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        org_check(&tx, s, organization)?;
        fill_range(&tx, s, q, 1)?;
        // Resolve the identity against the same selected range as the detail
        // table. A caller cannot retrieve arbitrary labels from other scopes.
        let (name, kind): (String, String) = if let Some(org) = organization {
            tx.query_row(
                "SELECT name,kind FROM temp.an_values WHERE slot=1 AND org=?1 AND key=?2",
                params![org, key],
                |row| Ok((row.get(0)?,row.get(1)?)),
            ).optional().map_err(err)?
        } else {
            tx.query_row(
                "SELECT name,kind FROM temp.an_combined WHERE slot=1 AND key=?1",
                [key],
                |row| Ok((row.get(0)?,row.get(1)?)),
            ).optional().map_err(err)?
        }.ok_or("Selected work/group not found in analysis scope")?;
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM temp.an_periods WHERE slot=1",
            [], |row| row.get(0),
        ).map_err(err)?;
        let monthly = count > 120;
        let sql = r#"
            WITH buckets AS (
                SELECT CASE WHEN ?1=1 THEN substr(start,1,7) ELSE start END AS label,
                       COUNT(*) AS periods
                FROM temp.an_periods WHERE slot=1 GROUP BY label
            ), missing AS (
                SELECT CASE WHEN ?1=1 THEN substr(start,1,7) ELSE start END AS label,
                       COUNT(*) AS missing
                FROM temp.an_missing
                WHERE slot=1 AND (?2 IS NULL OR org=?2) GROUP BY label
            ), aggregates AS (
                SELECT CASE WHEN ?1=1 THEN substr(p.start,1,7) ELSE p.start END AS label,
                       SUM(x.count) AS count,SUM(x.amount) AS amount
                FROM temp.an_periods p
                JOIN temp.an_valid v ON v.start=p.start
                JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org
                LEFT JOIN analysis_groups g ON g.work=x.work
                WHERE p.slot=1 AND (?2 IS NULL OR v.org=?2)
                  AND COALESCE(g.key,'work:'||x.work)=?3
                GROUP BY label
            )
            SELECT b.label,b.periods,COALESCE(m.missing,0),
                   COALESCE(a.count,0),COALESCE(a.amount,0)
            FROM buckets b
            LEFT JOIN missing m ON m.label=b.label
            LEFT JOIN aggregates a ON a.label=b.label
            ORDER BY b.label
        "#;
        let mut stmt = tx.prepare(sql).map_err(err)?;
        let mut rows = stmt.query(params![i64::from(monthly),organization,key]).map_err(err)?;
        let mut points=Vec::new();
        while let Some(row)=rows.next().map_err(err)? {
            if points.len()>=124 {return Err("Work timeline exceeds 124 points".into());}
            let missing:i64=row.get(2).map_err(err)?;
            points.push(TimelinePoint{
                start:row.get(0).map_err(err)?,
                periods:row.get(1).map_err(err)?,
                missing,
                total:totals(row.get(3).map_err(err)?,row.get(4).map_err(err)?,missing==0)?,
            });
        }
        Ok(WorkTrendSummary {
            key:key.to_string(),
            name,kind,
            granularity:if monthly {"month"} else {"period"}.into(),
            points,
        })
    }
    /// A small, sorted ranking of groups and works computed wholly in SQLite.
    /// Never show partial sums as a complete ranking; negative values remain
    /// distinguishable and ties have deterministic order.
    pub fn analysis_ranking(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        org: Option<&str>,
        metric: &str,
        category: &str,
    ) -> Result<RankingSummary, String> {
        if !matches!(metric, "amount" | "count") {
            return Err("Unknown ranking metric".into());
        }
        if !matches!(category, "all" | "work" | "group") {
            return Err("Unknown ranking category".into());
        }
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        org_check(&tx, s, org)?;
        fill_range(&tx, s, q, 1)?;
        if !complete(&tx, 1, org)? {
            return Ok(RankingSummary { complete: false, rows: Vec::new() });
        }
        // Every predicate and sort expression is fixed SQL, not user input.
        let sql = if org.is_some() {
            "SELECT key,name,kind,count,amount,active FROM temp.an_values
             WHERE slot=1 AND org=?3 AND (?1='all' OR kind=?1)
             ORDER BY ABS(CASE WHEN ?2='count' THEN count ELSE amount END) DESC,
                      key ASC LIMIT 12"
        } else {
            "SELECT key,name,kind,count,amount,active FROM temp.an_combined
             WHERE slot=1 AND ?3 IS NULL AND (?1='all' OR kind=?1)
             ORDER BY ABS(CASE WHEN ?2='count' THEN count ELSE amount END) DESC,
                      key ASC LIMIT 12"
        };
        let mut stmt = tx.prepare(sql).map_err(err)?;
        let mut cursor = stmt.query(params![category, metric, org]).map_err(err)?;
        let mut rows = Vec::new();
        while let Some(r) = cursor.next().map_err(err)? {
            let t = totals(r.get(3).map_err(err)?, r.get(4).map_err(err)?, true)?;
            rows.push(RangeValue {
                key: r.get(0).map_err(err)?,
                name: r.get(1).map_err(err)?,
                kind: r.get(2).map_err(err)?,
                count: t.count,
                amount: t.amount,
                average: t.average,
                active: r.get(5).map_err(err)?,
            });
        }
        Ok(RankingSummary { complete: true, rows })
    }
    pub fn analysis_range_summary(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
    ) -> Result<RangeSummary, String> {
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        fill_range(&tx, s, q, 1)?;
        range_summary(&tx, s, 1)
    }
    pub fn analysis_range_page(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        org: Option<&str>,
        after: Option<&str>,
        size: usize,
    ) -> Result<Vec<RangeValue>, String> {
        let n = cap(size)?;
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        org_check(&tx, s, org)?;
        fill_range(&tx, s, q, 1)?;
        range_rows(&tx, 1, org, after, n)
    }
    pub fn analysis_missing_page(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        after: Option<(&str, &str)>,
        size: usize,
    ) -> Result<Vec<MissingPeriod>, String> {
        let n = cap(size)?;
        let (org, start) = after.unwrap_or(("", ""));
        bounded(org)?;
        bounded(start)?;
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        fill_range(&tx, s, q, 1)?;
        let mut stmt=tx.prepare("SELECT org,start FROM temp.an_missing WHERE slot=1 AND (org,start)>(?1,?2) ORDER BY org,start LIMIT ?3").map_err(err)?;
        let result = stmt
            .query_map(params![org, start, n], |r| {
                Ok(MissingPeriod {
                    org: r.get(0)?,
                    start: r.get(1)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(result)
    }
    pub fn analysis_organization_page(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        after: Option<&str>,
        size: usize,
    ) -> Result<Vec<OrganizationSummary>, String> {
        let n = cap(size)?;
        if let Some(a) = after {
            bounded(a)?;
        }
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        fill_range(&tx, s, q, 1)?;
        let mut stmt=tx.prepare("SELECT f.item,(SELECT COUNT(*) FROM temp.an_missing m WHERE slot=1 AND m.org=f.item),COALESCE(SUM(v.count),0),COALESCE(SUM(v.amount),0) FROM analysis_filter_items f LEFT JOIN temp.an_values v ON v.slot=1 AND v.org=f.item WHERE f.id=?1 AND (?2 IS NULL OR f.item>?2) GROUP BY f.item ORDER BY f.item LIMIT ?3").map_err(err)?;
        let mut rows = stmt
            .query(params![s.organizations, after, n])
            .map_err(err)?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().map_err(err)? {
            let missing: i64 = r.get(1).map_err(err)?;
            out.push(OrganizationSummary {
                org: r.get(0).map_err(err)?,
                complete: missing == 0,
                missing,
                total: totals(r.get(2).map_err(err)?, r.get(3).map_err(err)?, missing == 0)?,
            })
        }
        Ok(out)
    }
    pub fn analysis_issue_page(
        &self,
        expected: i64,
        s: &AnalysisScope,
        after: Option<&IssueCursor>,
        size: usize,
    ) -> Result<Vec<AnalysisIssue>, String> {
        let n = cap(size)?;
        let empty = IssueCursor {
            report_id: "".into(),
            org: "".into(),
            kind: "".into(),
            other_report_id: "".into(),
        };
        let a = after.unwrap_or(&empty);
        for k in [&a.report_id, &a.org, &a.kind, &a.other_report_id] {
            bounded(k)?;
        }
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        let mut stmt=tx.prepare("SELECT report_id,org,kind,other_report_id,(SELECT issue FROM analysis_reports a WHERE a.report_id=an_issues.report_id) FROM temp.an_issues WHERE (?1=0 OR (report_id,org,kind,other_report_id)>(?2,?3,?4,?5)) ORDER BY report_id,org,kind,other_report_id LIMIT ?6").map_err(err)?;
        let out = stmt
            .query_map(
                params![
                    after.is_some(),
                    a.report_id,
                    a.org,
                    a.kind,
                    a.other_report_id,
                    n
                ],
                |r| {
                    Ok(AnalysisIssue {
                        reason_code: r.get(4)?,
                        report_id: r.get(0)?,
                        org: r.get(1)?,
                        kind: r.get(2)?,
                        other_report_id: r.get(3)?,
                    })
                },
            )
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(out)
    }
}
fn thresholds(q: &ChangeQuery) -> Result<(), String> {
    if !matches!(q.method.as_str(), "previous" | "range")
        || [q.percent, q.min_count, q.min_amount]
            .iter()
            .any(|x| !x.is_finite() || *x < 0.)
        || date(&q.from)? > date(&q.to)?
    {
        return Err("Invalid change method, dates, or nonnegative thresholds".into());
    }
    Ok(())
}
fn measure(base: f64, current: f64, minimum: f64, threshold: f64) -> Result<ChangeMeasure, String> {
    let delta = finite(current - base)?;
    let percent = if base == 0. {
        None
    } else {
        Some(finite(delta / base.abs() * 100.)?)
    };
    let passes = percent.is_none_or(|p| p.abs() >= threshold);
    Ok(ChangeMeasure {
        base,
        current,
        delta,
        percent,
        negative: base < 0. || current < 0.,
        small_base: base == 0. || base.abs() < minimum,
        notable: delta != 0. && passes,
        significant: delta != 0. && delta.abs() >= minimum && passes,
    })
}
fn average_change(b: Option<f64>, a: Option<f64>, threshold: f64) -> Result<AverageChange, String> {
    if let (Some(b), Some(a)) = (b, a) {
        let m = measure(b, a, 0., threshold)?;
        Ok(AverageChange {
            base: Some(b),
            current: Some(a),
            delta: Some(m.delta),
            percent: m.percent,
            flags: Some(AverageFlags {
                negative: m.negative,
                small_base: m.small_base,
                notable: m.notable,
                significant: m.significant,
            }),
        })
    } else {
        Ok(AverageChange {
            base: b,
            current: a,
            delta: None,
            percent: None,
            flags: None,
        })
    }
}
fn status(bc: f64, ba: f64, cc: f64, ca: f64) -> String {
    let b = bc != 0. || ba != 0.;
    let c = cc != 0. || ca != 0.;
    if !b && c {
        "new"
    } else if b && !c {
        "gone"
    } else {
        "existing"
    }
    .into()
}
fn fill_changes(c: &Connection, s: &AnalysisScope, q: &ChangeQuery) -> Result<(), String> {
    thresholds(q)?;
    c.execute_batch("CREATE TEMP TABLE an_currents(start TEXT PRIMARY KEY,base TEXT) WITHOUT ROWID;
 CREATE TEMP TABLE an_pairs(start TEXT,base TEXT,org TEXT,base_id TEXT,current_id TEXT,warned INTEGER,PRIMARY KEY(start,org)) WITHOUT ROWID;
 CREATE TEMP TABLE an_period_values(start TEXT,org TEXT,key TEXT,name TEXT,kind TEXT,count REAL,amount REAL,PRIMARY KEY(start,org,key)) WITHOUT ROWID;
 CREATE TEMP TABLE an_changes(start TEXT,base TEXT,org TEXT,key TEXT,name TEXT,kind TEXT,bc REAL,ba REAL,cc REAL,ca REAL,PRIMARY KEY(start,org,key)) WITHOUT ROWID;").map_err(err)?;
    if q.method == "previous" {
        let d = date(&q.to)?;
        let b = shift(&s.report_type, d, -1)?;
        c.execute(
            "INSERT INTO temp.an_currents VALUES(?1,?2)",
            params![q.to, b.to_string()],
        )
        .map_err(err)?;
    } else {
        let mut stmt=c.prepare("SELECT DISTINCT start FROM temp.an_candidates WHERE start>=?1 AND start<=?2 ORDER BY start").map_err(err)?;
        let mut rows = stmt.query(params![q.from, q.to]).map_err(err)?;
        while let Some(r) = rows.next().map_err(err)? {
            let start: String = r.get(0).map_err(err)?;
            let b = shift(&s.report_type, date(&start)?, -1)?.to_string();
            if b >= q.from {
                c.execute(
                    "INSERT INTO temp.an_currents VALUES(?1,?2)",
                    params![start, b],
                )
                .map_err(err)?;
            }
        }
    }
    c.execute("INSERT INTO temp.an_pairs SELECT p.start,p.base,f.item,b.report_id,v.report_id,COALESCE(b.warned,0) OR COALESCE(v.warned,0) FROM temp.an_currents p CROSS JOIN analysis_filter_items f LEFT JOIN temp.an_valid b ON b.org=f.item AND b.start=p.base LEFT JOIN temp.an_valid v ON v.org=f.item AND v.start=p.start WHERE f.id=?1",[&s.organizations]).map_err(err)?;
    c.execute("INSERT INTO temp.an_period_values SELECT v.start,v.org,COALESCE(g.key,'work:'||x.work),COALESCE(g.name,x.work),CASE WHEN g.key IS NULL THEN 'work' ELSE 'group' END,SUM(x.count),SUM(x.amount) FROM temp.an_valid v JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org LEFT JOIN analysis_groups g ON g.work=x.work WHERE EXISTS(SELECT 1 FROM temp.an_pairs p WHERE p.org=v.org AND p.base_id IS NOT NULL AND p.current_id IS NOT NULL AND (p.base=v.start OR p.start=v.start)) AND (?1 IS NULL OR EXISTS(SELECT 1 FROM analysis_filter_items f WHERE f.id=?1 AND f.item=COALESCE(g.name,x.work))) GROUP BY v.start,v.org,COALESCE(g.key,'work:'||x.work)",[&s.names]).map_err(err)?;
    c.execute_batch("INSERT INTO temp.an_changes SELECT p.start,p.base,p.org,k.key,COALESCE(b.name,a.name),COALESCE(b.kind,a.kind),COALESCE(b.count,0),COALESCE(b.amount,0),COALESCE(a.count,0),COALESCE(a.amount,0) FROM temp.an_pairs p JOIN (SELECT p.start,p.org,v.key FROM temp.an_pairs p JOIN temp.an_period_values v ON v.org=p.org AND (v.start=p.base OR v.start=p.start) WHERE p.base_id IS NOT NULL AND p.current_id IS NOT NULL GROUP BY p.start,p.org,v.key) k ON k.start=p.start AND k.org=p.org LEFT JOIN temp.an_period_values b ON b.start=p.base AND b.org=p.org AND b.key=k.key LEFT JOIN temp.an_period_values a ON a.start=p.start AND a.org=p.org AND a.key=k.key;").map_err(err)?;
    validate_numbers(c, "SELECT count,amount FROM temp.an_period_values")?;
    validate_numbers(
        c,
        "SELECT SUM(bc),SUM(ba) FROM temp.an_changes HAVING COUNT(*)>0",
    )?;
    validate_numbers(
        c,
        "SELECT SUM(cc),SUM(ca) FROM temp.an_changes HAVING COUNT(*)>0",
    )?;
    Ok(())
}
fn change_value(r: &rusqlite::Row<'_>, q: &ChangeQuery) -> Result<ChangeValue, String> {
    let bc: f64 = r.get(6).map_err(err)?;
    let ba: f64 = r.get(7).map_err(err)?;
    let cc: f64 = r.get(8).map_err(err)?;
    let ca: f64 = r.get(9).map_err(err)?;
    let b = totals(bc, ba, true)?;
    let a = totals(cc, ca, true)?;
    Ok(ChangeValue {
        current_start: r.get(0).map_err(err)?,
        base_start: r.get(1).map_err(err)?,
        org: r.get(2).map_err(err)?,
        key: r.get(3).map_err(err)?,
        name: r.get(4).map_err(err)?,
        kind: r.get(5).map_err(err)?,
        status: status(bc, ba, cc, ca),
        count: measure(bc, cc, q.min_count, q.percent)?,
        amount: measure(ba, ca, q.min_amount, q.percent)?,
        average: average_change(b.average, a.average, q.percent)?,
    })
}
impl Storage {
    pub fn analysis_changes_page(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        after: Option<&ChangeCursor>,
        size: usize,
    ) -> Result<Vec<ChangeValue>, String> {
        let n = cap(size)?;
        let empty = ChangeCursor {
            current_start: "".into(),
            org: "".into(),
            key: "".into(),
        };
        let a = after.unwrap_or(&empty);
        for k in [&a.current_start, &a.org] {
            bounded(k)?;
        }
        key_bound(&a.key)?;
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        fill_changes(&tx, s, q)?;
        let mut stmt=tx.prepare("SELECT start,base,org,key,name,kind,bc,ba,cc,ca FROM temp.an_changes WHERE (?1=0 OR (start,org,key)>(?2,?3,?4)) ORDER BY start,org,key LIMIT ?5").map_err(err)?;
        let mut rows = stmt
            .query(params![after.is_some(), a.current_start, a.org, a.key, n])
            .map_err(err)?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().map_err(err)? {
            out.push(change_value(r, q)?);
        }
        Ok(out)
    }
    pub fn analysis_changes_summary(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
    ) -> Result<ChangesSummary, String> {
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        fill_changes(&tx, s, q)?;
        let (rows,new,gone):(i64,i64,i64)=tx.query_row("SELECT COUNT(*),COALESCE(SUM((bc=0 AND ba=0) AND (cc<>0 OR ca<>0)),0),COALESCE(SUM((bc<>0 OR ba<>0) AND (cc=0 AND ca=0)),0) FROM temp.an_changes",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(err)?;
        let (bc,ba,cc,ca):(f64,f64,f64,f64)=tx.query_row("SELECT COALESCE(SUM(bc),0),COALESCE(SUM(ba),0),COALESCE(SUM(cc),0),COALESCE(SUM(ca),0) FROM temp.an_changes",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(err)?;
        let bt = totals(bc, ba, true)?;
        let at = totals(cc, ca, true)?;
        Ok(ChangesSummary{count:measure(bc,cc,q.min_count,q.percent)?,amount:measure(ba,ca,q.min_amount,q.percent)?,average:average_change(bt.average,at.average,q.percent)?,rows,new,gone,existing:rows-new-gone,missing_pairs:count_sql(&tx,"SELECT COUNT(*) FROM temp.an_pairs WHERE base_id IS NULL OR current_id IS NULL")?,import_warning_pairs:count_sql(&tx,"SELECT COUNT(*) FROM temp.an_pairs WHERE base_id IS NOT NULL AND current_id IS NOT NULL AND warned<>0")?,invalid_reports:count_sql(&tx,"SELECT COUNT(*) FROM temp.an_issues WHERE kind='invalid'")?,duplicate_issues:count_sql(&tx,"SELECT COUNT(*) FROM temp.an_issues WHERE kind='duplicate'")?,overlap_issues:count_sql(&tx,"SELECT COUNT(*) FROM temp.an_issues WHERE kind='overlap'")?})
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonQuery {
    pub before: RangeQuery,
    pub after: RangeQuery,
    pub percent: f64,
    pub min_count: f64,
    pub min_amount: f64,
}
#[derive(Debug, Serialize)]
pub struct ComparisonSummary {
    pub before: RangeSummary,
    pub after: RangeSummary,
    pub values: i64,
    pub count: Option<ChangeMeasure>,
    pub amount: Option<ChangeMeasure>,
    pub average: AverageChange,
}
#[derive(Debug, Serialize)]
pub struct DivisionPeriodChange {
    pub organization: String,
    pub before: AnalysisTotals,
    pub after: AnalysisTotals,
    pub count: Option<ChangeMeasure>,
    pub amount: Option<ChangeMeasure>,
}
#[derive(Debug, Serialize)]
pub struct ComparisonDashboard {
    pub summary: ComparisonSummary,
    pub divisions: Vec<DivisionPeriodChange>,
    pub top_amount: Vec<ComparisonValue>,
    pub top_count: Vec<ComparisonValue>,
}

fn comparison(
    c: &Connection,
    s: &AnalysisScope,
    q: &ComparisonQuery,
    org: Option<&str>,
) -> Result<(bool, bool), String> {
    if [q.percent, q.min_count, q.min_amount]
        .iter()
        .any(|x| !x.is_finite() || *x < 0.)
    {
        return Err("Invalid comparison thresholds".into());
    }
    org_check(c, s, org)?;
    fill_range(c, s, &q.before, 1)?;
    fill_range(c, s, &q.after, 2)?;
    c.execute_batch("CREATE TEMP TABLE an_comparison(key TEXT PRIMARY KEY,name TEXT,kind TEXT,bc REAL,ba REAL,cc REAL,ca REAL) WITHOUT ROWID;").map_err(err)?;
    let table = if org.is_some() {
        "temp.an_values"
    } else {
        "temp.an_combined"
    };
    let predicate = if org.is_some() {
        "AND org=?1"
    } else {
        "AND ?1 IS NULL"
    };
    c.execute(&format!("INSERT INTO temp.an_comparison WITH b AS (SELECT * FROM {table} WHERE slot=1 {predicate}),a AS (SELECT * FROM {table} WHERE slot=2 {predicate}),keys AS (SELECT key FROM b UNION SELECT key FROM a) SELECT keys.key,COALESCE(b.name,a.name),COALESCE(b.kind,a.kind),COALESCE(b.count,0),COALESCE(b.amount,0),COALESCE(a.count,0),COALESCE(a.amount,0) FROM keys LEFT JOIN b USING(key) LEFT JOIN a USING(key)"),[org]).map_err(err)?;
    c.execute("INSERT INTO temp.an_comparison SELECT 'absent:'||f.item,f.item,'work',0,0,0,0 FROM analysis_filter_items f WHERE f.id=?1 AND NOT EXISTS(SELECT 1 FROM temp.an_comparison a WHERE a.name=f.item)",[&s.names]).map_err(err)?;
    Ok((complete(c, 1, org)?, complete(c, 2, org)?))
}
fn comparison_value(
    r: &rusqlite::Row<'_>,
    q: &ComparisonQuery,
    b: bool,
    a: bool,
) -> Result<ComparisonValue, String> {
    let bc: f64 = r.get(3).map_err(err)?;
    let ba: f64 = r.get(4).map_err(err)?;
    let cc: f64 = r.get(5).map_err(err)?;
    let ca: f64 = r.get(6).map_err(err)?;
    let bt = totals(bc, ba, b)?;
    let at = totals(cc, ca, a)?;
    Ok(ComparisonValue {
        key: r.get(0).map_err(err)?,
        name: r.get(1).map_err(err)?,
        kind: r.get(2).map_err(err)?,
        status: if b && a {
            Some(status(bc, ba, cc, ca))
        } else {
            None
        },
        count: if b && a {
            Some(measure(bc, cc, q.min_count, q.percent)?)
        } else {
            None
        },
        amount: if b && a {
            Some(measure(ba, ca, q.min_amount, q.percent)?)
        } else {
            None
        },
        average: average_change(bt.average, at.average, q.percent)?,
    })
}
impl Storage {
    pub fn analysis_comparison_summary(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ComparisonQuery,
    ) -> Result<ComparisonSummary, String> {
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        comparison(&tx, s, q, None)?;
        let b = range_summary(&tx, s, 1)?;
        let a = range_summary(&tx, s, 2)?;
        let count = if let (Some(b), Some(a)) = (b.total.count, a.total.count) {
            Some(measure(b, a, q.min_count, q.percent)?)
        } else {
            None
        };
        let amount = if let (Some(b), Some(a)) = (b.total.amount, a.total.amount) {
            Some(measure(b, a, q.min_amount, q.percent)?)
        } else {
            None
        };
        let average = average_change(b.total.average, a.total.average, q.percent)?;
        Ok(ComparisonSummary {
            before: b,
            after: a,
            values: count_sql(&tx, "SELECT COUNT(*) FROM temp.an_comparison")?,
            count,
            amount,
            average,
        })
    }
    /// One bounded, revision-consistent comparison of two exact date ranges.
    /// Returns no work rankings when either selected range is incomplete;
    /// division comparisons are independently hidden when their own history
    /// is incomplete. All aggregation occurs in SQLite.
    pub fn analysis_comparison_dashboard(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ComparisonQuery,
    ) -> Result<ComparisonDashboard, String> {
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        let (before_complete, after_complete) = comparison(&tx, s, q, None)?;
        let before = range_summary(&tx, s, 1)?;
        let after = range_summary(&tx, s, 2)?;
        let count = if let (Some(b), Some(a)) = (before.total.count, after.total.count) {
            Some(measure(b, a, q.min_count, q.percent)?)
        } else { None };
        let amount = if let (Some(b), Some(a)) = (before.total.amount, after.total.amount) {
            Some(measure(b, a, q.min_amount, q.percent)?)
        } else { None };
        let average = average_change(before.total.average, after.total.average, q.percent)?;
        let summary = ComparisonSummary {
            before, after,
            values: count_sql(&tx, "SELECT COUNT(*) FROM temp.an_comparison")?,
            count, amount, average,
        };
        let mut divisions = Vec::new();
        let mut stmt = tx.prepare(
            "SELECT item FROM analysis_filter_items WHERE id=?1 ORDER BY ordinal LIMIT 101"
        ).map_err(err)?;
        let mut orgs = stmt.query([&s.organizations]).map_err(err)?;
        while let Some(row) = orgs.next().map_err(err)? {
            if divisions.len()>=100 {return Err("Comparison exceeds 100 divisions".into());}
            let org: String = row.get(0).map_err(err)?;
            let available_before = complete(&tx,1,Some(&org))?;
            let available_after = complete(&tx,2,Some(&org))?;
            let get_totals = |slot:i64,ok:bool| -> Result<AnalysisTotals,String> {
                let (c,a):(f64,f64) = tx.query_row(
                    "SELECT COALESCE(SUM(count),0),COALESCE(SUM(amount),0)
                     FROM temp.an_values WHERE slot=?1 AND org=?2",
                    params![slot,&org],
                    |r| Ok((r.get(0)?,r.get(1)?)),
                ).map_err(err)?;
                totals(c,a,ok)
            };
            let b = get_totals(1,available_before)?;
            let a = get_totals(2,available_after)?;
            let dc = if let (Some(b),Some(a)) = (b.count,a.count) {
                Some(measure(b,a,q.min_count,q.percent)?)
            } else {None};
            let da = if let (Some(b),Some(a)) = (b.amount,a.amount) {
                Some(measure(b,a,q.min_amount,q.percent)?)
            } else {None};
            divisions.push(DivisionPeriodChange {
                organization:org,before:b,after:a,count:dc,amount:da
            });
        }
        drop(orgs);
        drop(stmt);
        let mut top_amount=Vec::new();
        let mut top_count=Vec::new();
        if before_complete && after_complete {
            for metric in ["amount","count"] {
                let sql = "SELECT key,name,kind,bc,ba,cc,ca FROM temp.an_comparison
                    ORDER BY ABS(CASE WHEN ?1='amount' THEN ca-ba ELSE cc-bc END) DESC,
                             key ASC LIMIT 12";
                let mut ranking = tx.prepare(sql).map_err(err)?;
                let mut values=ranking.query([metric]).map_err(err)?;
                while let Some(row)=values.next().map_err(err)? {
                    let item=comparison_value(row,q,true,true)?;
                    if metric=="amount" {top_amount.push(item);}
                    else {top_count.push(item);}
                }
            }
        }
        Ok(ComparisonDashboard{summary,divisions,top_amount,top_count})
    }
    pub fn analysis_comparison_page(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ComparisonQuery,
        org: Option<&str>,
        after: Option<&str>,
        size: usize,
    ) -> Result<Vec<ComparisonValue>, String> {
        let n = cap(size)?;
        if let Some(a) = after {
            key_bound(a)?;
        }
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        let (b, a) = comparison(&tx, s, q, org)?;
        let mut stmt=tx.prepare("SELECT key,name,kind,bc,ba,cc,ca FROM temp.an_comparison WHERE (?1 IS NULL OR key>?1) ORDER BY key LIMIT ?2").map_err(err)?;
        let mut rows = stmt.query(params![after, n]).map_err(err)?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().map_err(err)? {
            out.push(comparison_value(r, q, b, a)?);
        }
        Ok(out)
    }
}

enum Cell<'a> {
    Text(&'a str),
    Number(Option<f64>),
}
fn number_text(n: f64) -> String {
    if n == 0. {
        return "0".into();
    }
    let a = n.abs();
    if (1e-6..1e21).contains(&a) {
        return n.to_string().replace('.', ",");
    }
    let raw = format!("{n:e}");
    let (m, e) = raw.split_once('e').unwrap();
    let exponent: i32 = e.parse().unwrap();
    format!(
        "{}e{}{exponent}",
        m.replace('.', ","),
        if exponent >= 0 { "+" } else { "" }
    )
}
fn csv_record(w: &mut impl Write, cells: &[Cell<'_>]) -> Result<(), String> {
    for (i, c) in cells.iter().enumerate() {
        if i > 0 {
            w.write_all(b";").map_err(err)?;
        }
        w.write_all(b"\"").map_err(err)?;
        let text = match c {
            Cell::Text(s) => {
                let leading = s.trim_start_matches(|c: char| {
                    c <= '\u{001f}'
                        || matches!(
                            c,
                            ' ' | '\u{00a0}' | '\u{1680}' | '\u{2000}'
                                ..='\u{200a}'
                                    | '\u{2028}'
                                    | '\u{2029}'
                                    | '\u{202f}'
                                    | '\u{205f}'
                                    | '\u{3000}'
                                    | '\u{feff}'
                        )
                });
                if leading.starts_with(['=', '+', '-', '@']) {
                    format!("'{s}")
                } else {
                    s.to_string()
                }
            }
            Cell::Number(Some(n)) => number_text(finite(*n)?),
            Cell::Number(None) => String::new(),
        };
        for part in text.split_inclusive('"') {
            w.write_all(part.as_bytes()).map_err(err)?;
            if part.ends_with('"') {
                w.write_all(b"\"").map_err(err)?;
            }
        }
        w.write_all(b"\"").map_err(err)?;
    }
    w.write_all(b"\r\n").map_err(err)
}
fn export(
    destination: &Path,
    write: impl FnOnce(&mut std::io::BufWriter<fs::File>) -> Result<(), String>,
) -> Result<(), String> {
    export_before(destination, write, || {})
}
fn export_before(
    destination: &Path,
    write: impl FnOnce(&mut std::io::BufWriter<fs::File>) -> Result<(), String>,
    before: impl FnOnce(),
) -> Result<(), String> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err("Export destination already exists".into());
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stamp = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(err)?
        .as_nanos();
    let mut staging = None;
    for attempt in 0..100 {
        let path = parent.join(format!(
            ".analiz-export-{}-{stamp}-{attempt}.csv",
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
        {
            Ok(f) => {
                staging = Some((path, f));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(err(e)),
        }
    }
    let (path, file) = staging.ok_or("Could not reserve export staging file")?;
    let result = (|| {
        let mut w = std::io::BufWriter::with_capacity(8192, file);
        w.write_all(b"\xef\xbb\xbf").map_err(err)?;
        write(&mut w)?;
        w.flush().map_err(err)?;
        w.get_ref().sync_all().map_err(err)?;
        drop(w);
        before();
        fs::hard_link(&path, destination).map_err(err)
    })();
    let _ = fs::remove_file(&path);
    result
}
fn change_csv(w: &mut impl Write, v: &ChangeValue) -> Result<(), String> {
    use Cell::*;
    csv_record(
        w,
        &[
            Text(&v.key),
            Text(&v.name),
            Text(&v.kind),
            Text(&v.org),
            Text(&v.base_start),
            Text(&v.current_start),
            Text(&v.status),
            Number(Some(v.count.base)),
            Number(Some(v.count.current)),
            Number(Some(v.count.delta)),
            Number(v.count.percent),
            Number(Some(v.amount.base)),
            Number(Some(v.amount.current)),
            Number(Some(v.amount.delta)),
            Number(v.amount.percent),
            Number(v.average.base),
            Number(v.average.current),
            Number(v.average.delta),
            Number(v.average.percent),
        ],
    )
}
impl Storage {
    pub fn analysis_export_changes(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        destination: &Path,
    ) -> Result<(), String> {
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        fill_changes(&tx, s, q)?;
        export(destination, |w| {
            use Cell::*;
            csv_record(
                w,
                &[
                    Text("key"),
                    Text("name"),
                    Text("kind"),
                    Text("org"),
                    Text("baseStart"),
                    Text("currentStart"),
                    Text("status"),
                    Text("count.base"),
                    Text("count.current"),
                    Text("count.delta"),
                    Text("count.percent"),
                    Text("amount.base"),
                    Text("amount.current"),
                    Text("amount.delta"),
                    Text("amount.percent"),
                    Text("average.base"),
                    Text("average.current"),
                    Text("average.delta"),
                    Text("average.percent"),
                ],
            )?;
            let mut stmt=tx.prepare("SELECT start,base,org,key,name,kind,bc,ba,cc,ca FROM temp.an_changes ORDER BY start,org,key").map_err(err)?;
            let mut rows = stmt.query([]).map_err(err)?;
            while let Some(r) = rows.next().map_err(err)? {
                change_csv(w, &change_value(r, q)?)?;
            }
            Ok(())
        })
    }
    pub fn analysis_export_range(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        org: Option<&str>,
        destination: &Path,
    ) -> Result<(), String> {
        self.analysis_export_range_before_publish(expected, s, q, org, destination, || {})
    }
    pub(super) fn analysis_export_range_before_publish(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        org: Option<&str>,
        destination: &Path,
        before: impl FnOnce(),
    ) -> Result<(), String> {
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        org_check(&tx, s, org)?;
        fill_range(&tx, s, q, 1)?;
        let available = complete(&tx, 1, org)?;
        export_before(
            destination,
            |w| {
                use Cell::*;
                csv_record(
                    w,
                    &[
                        Text("key"),
                        Text("name"),
                        Text("kind"),
                        Text("count"),
                        Text("amount"),
                        Text("average"),
                        Text("active"),
                    ],
                )?;
                let sql = if org.is_some() {
                    "SELECT key,name,kind,count,amount,active FROM temp.an_values WHERE slot=1 AND org=?1 ORDER BY key"
                } else {
                    "SELECT key,name,kind,count,amount,active FROM temp.an_combined WHERE slot=1 AND ?1 IS NULL ORDER BY key"
                };
                let mut stmt = tx.prepare(sql).map_err(err)?;
                let mut rows = stmt.query([org]).map_err(err)?;
                while let Some(r) = rows.next().map_err(err)? {
                    let key: String = r.get(0).map_err(err)?;
                    let name: String = r.get(1).map_err(err)?;
                    let kind: String = r.get(2).map_err(err)?;
                    let t = totals(r.get(3).map_err(err)?, r.get(4).map_err(err)?, available)?;
                    let active: bool = r.get(5).map_err(err)?;
                    csv_record(
                        w,
                        &[
                            Text(&key),
                            Text(&name),
                            Text(&kind),
                            Number(t.count),
                            Number(t.amount),
                            Number(t.average),
                            Text(if active { "true" } else { "false" }),
                        ],
                    )?;
                }
                Ok(())
            },
            before,
        )
    }
    pub fn analysis_export_comparison(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ComparisonQuery,
        org: Option<&str>,
        destination: &Path,
    ) -> Result<(), String> {
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        let (b, a) = comparison(&tx, s, q, org)?;
        export(destination, |w| {
            use Cell::*;
            csv_record(
                w,
                &[
                    Text("key"),
                    Text("name"),
                    Text("kind"),
                    Text("status"),
                    Text("count.base"),
                    Text("count.current"),
                    Text("count.delta"),
                    Text("count.percent"),
                    Text("amount.base"),
                    Text("amount.current"),
                    Text("amount.delta"),
                    Text("amount.percent"),
                    Text("average.base"),
                    Text("average.current"),
                    Text("average.delta"),
                    Text("average.percent"),
                ],
            )?;
            let mut stmt = tx
                .prepare("SELECT key,name,kind,bc,ba,cc,ca FROM temp.an_comparison ORDER BY key")
                .map_err(err)?;
            let mut rows = stmt.query([]).map_err(err)?;
            while let Some(r) = rows.next().map_err(err)? {
                let v = comparison_value(r, q, b, a)?;
                csv_record(
                    w,
                    &[
                        Text(&v.key),
                        Text(&v.name),
                        Text(&v.kind),
                        Text(v.status.as_deref().unwrap_or("")),
                        Number(if b {
                            Some(r.get(3).map_err(err)?)
                        } else {
                            None
                        }),
                        Number(if a {
                            Some(r.get(5).map_err(err)?)
                        } else {
                            None
                        }),
                        Number(v.count.as_ref().map(|m| m.delta)),
                        Number(v.count.as_ref().and_then(|m| m.percent)),
                        Number(if b {
                            Some(r.get(4).map_err(err)?)
                        } else {
                            None
                        }),
                        Number(if a {
                            Some(r.get(6).map_err(err)?)
                        } else {
                            None
                        }),
                        Number(v.amount.as_ref().map(|m| m.delta)),
                        Number(v.amount.as_ref().and_then(|m| m.percent)),
                        Number(v.average.base),
                        Number(v.average.current),
                        Number(v.average.delta),
                        Number(v.average.percent),
                    ],
                )?;
            }
            Ok(())
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePairCursor {
    pub current_start: String,
    pub org: String,
}
#[derive(Debug, Serialize)]
pub struct ChangePair {
    pub current_start: String,
    pub base_start: String,
    pub org: String,
    pub missing_base: bool,
    pub missing_current: bool,
    pub import_warning: bool,
}
impl ChangePair {
    pub fn cursor(&self) -> ChangePairCursor {
        ChangePairCursor {
            current_start: self.current_start.clone(),
            org: self.org.clone(),
        }
    }
}
impl Storage {
    pub fn analysis_change_pair_page(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        problems_only: bool,
        after: Option<&ChangePairCursor>,
        size: usize,
    ) -> Result<Vec<ChangePair>, String> {
        let n = cap(size)?;
        let empty = ChangePairCursor {
            current_start: "".into(),
            org: "".into(),
        };
        let a = after.unwrap_or(&empty);
        bounded(&a.current_start)?;
        bounded(&a.org)?;
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        fill_changes(&tx, s, q)?;
        let mut stmt=tx.prepare("SELECT start,base,org,base_id IS NULL,current_id IS NULL,warned<>0 FROM temp.an_pairs WHERE (?1=0 OR base_id IS NULL OR current_id IS NULL OR warned<>0) AND (?2=0 OR (start,org)>(?3,?4)) ORDER BY start,org LIMIT ?5").map_err(err)?;
        let out = stmt
            .query_map(
                params![problems_only, after.is_some(), a.current_start, a.org, n],
                |r| {
                    Ok(ChangePair {
                        current_start: r.get(0)?,
                        base_start: r.get(1)?,
                        org: r.get(2)?,
                        missing_base: r.get(3)?,
                        missing_current: r.get(4)?,
                        import_warning: r.get(5)?,
                    })
                },
            )
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(out)
    }
}
