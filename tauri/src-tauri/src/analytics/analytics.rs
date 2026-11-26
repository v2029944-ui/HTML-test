//! Bounded analysis API; no GUI commands. See storage-core/ANALYTICS.md.
use super::csv_export::{csv_record, export, Cell};
use super::read_model::{check_metadata_bounds, check_revision};
use super::*;
use chrono::{Datelike, Duration, Months};
use rusqlite::OptionalExtension;
use std::io::Write;
#[path = "presentation/advanced_analysis.rs"]
mod advanced_analysis;
pub use advanced_analysis::*;
#[path = "presentation/analysis_presentation.rs"]
mod analysis_presentation;
#[path = "presentation/reference_trend_csv.rs"]
mod reference_trend_csv;
pub use reference_trend_csv::*;
#[path = "presentation/reference_changes.rs"]
mod reference_changes;
pub use reference_changes::*;
#[path = "presentation/unified_analytics.rs"]
mod unified_analytics;
pub use unified_analytics::*;
#[path = "presentation/reference_detail_csv.rs"]
mod reference_detail_csv;
pub use reference_detail_csv::*;
#[path = "presentation/unified_group_audit.rs"]
mod unified_group_audit;
pub use unified_group_audit::*;
#[path = "presentation/analysis_coverage.rs"]
mod analysis_coverage;
pub use analysis_coverage::*;
const BOUND: usize = 65536;
#[path = "index.rs"]
mod index;
#[path = "workspace.rs"]
mod workspace;
#[path = "ranges.rs"]
mod ranges;
#[path = "trends.rs"]
mod trends;
#[path = "changes.rs"]
mod changes;
#[path = "comparison.rs"]
mod comparison;
#[path = "csv.rs"]
mod csv;

pub use changes::{AverageChange, AverageFlags, ChangeCursor, ChangeMeasure, ChangePair, ChangePairCursor, ChangeQuery, ChangeValue, ChangesSummary};
pub use comparison::{ComparisonDashboard, ComparisonQuery, ComparisonSummary, ComparisonValue, DivisionPeriodChange};
pub use index::{AnalysisNameOption, AnalysisNameOptions};
pub use ranges::{AnalysisIssue, IssueCursor, MissingPeriod, OrganizationSummary, RangeSummary, RangeValue, RankingSummary};
pub use trends::{DailyRollupPage, DailyRollupPoint, OrganizationTrendPoint, OrganizationTrendSeries, OrganizationTrends, TimelinePoint, TimelineSummary, WorkTrendSummary};

use changes::{average_change, change_value, changes_summary, fill_changes, finish_changes, measure, status, status_with_observed_amounts, thresholds};
use comparison::{comparison, comparison_value};
use csv::{change_csv, changes_csv_header, write_changes_rows};
pub(super) use index::initialize;
#[cfg(test)]
pub(super) use workspace::CONFLICT_SQL;
use index::filter_check;
use ranges::{org_check, range_summary};
use trends::{fill_timeline_report_buckets, timeline_grouping, TIMELINE_BUCKET_TOTALS};
use workspace::{complete, count_sql, create_name_catalog, fill_range, observed_reports, selected_organization_count, validate_numbers, workspace};

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

#[derive(Debug, Serialize)]
pub struct AnalysisTotals {
    pub count: Option<f64>,
    pub amount: Option<f64>,
    pub average: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_average_quantity: Option<f64>,
}

/// Runs analytical reads against the storage's revision and filter handles.
struct Analytics<'a> {
    database: &'a dyn AnalysisDatabase,
}

trait AnalysisDatabase {
    fn connect(&self) -> Result<Connection, String>;
    fn create_organization_filter(
        &self,
        revision: i64,
        organizations: &[String],
    ) -> Result<String, String>;
    fn delete_filter(&self, id: &str) -> Result<(), String>;
}

impl AnalysisDatabase for Storage {
    fn connect(&self) -> Result<Connection, String> {
        Storage::connect(self)
    }

    fn create_organization_filter(
        &self,
        revision: i64,
        organizations: &[String],
    ) -> Result<String, String> {
        let id = self.analysis_filter_create(revision, "organizations")?;
        let prepared = self
            .analysis_filter_append(revision, &id, organizations)
            .and_then(|_| self.analysis_filter_seal(revision, &id));
        if let Err(error) = prepared {
            let _ = self.analysis_filter_delete(&id);
            return Err(error);
        }
        Ok(id)
    }

    fn delete_filter(&self, id: &str) -> Result<(), String> {
        self.analysis_filter_delete(id)
    }
}

impl Storage {
    fn analysis(&self) -> Analytics<'_> {
        Analytics { database: self }
    }
}

impl Analytics<'_> {
    fn with_snapshot<T>(
        &self,
        revision: i64,
        scope: &AnalysisScope,
        read: impl FnOnce(&Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut connection = self.database.connect()?;
        let transaction = connection.transaction().map_err(err)?;
        workspace(&transaction, revision, scope)?;
        read(&transaction)
    }
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
// Report coverage and monetary completeness are independent. An absent work in
// a covered report is zero, while a present row can have an unknown amount.
fn totals(count: f64, amount: Option<f64>, complete: bool) -> Result<AnalysisTotals, String> {
    finite(count)?;
    amount.map(finite).transpose()?;
    Ok(AnalysisTotals {
        count: complete.then_some(count),
        partial_average_quantity: None,
        amount: if complete { amount } else { None },
        average: if complete && count > 0. {
            amount.map(|amount| finite(amount / count)).transpose()?
        } else {
            None
        },
    })
}
// Partial absolute amounts only sum the known cells.
fn observed_totals(count: f64, amount: Option<f64>, known_amount: Option<f64>, observed: bool) -> Result<AnalysisTotals, String> {
    let mut result = totals(count, amount, observed)?;
    if observed && amount.is_none() {
        result.amount = known_amount.map(finite).transpose()?;
    }
    Ok(result)
}
// Quantities are comparable for the same original work and report format.
// The SQL passes a denominator only when that identity is uniform and uses
// exactly the rows with a known amount.
fn paired_observed_totals(
    count: f64,
    amount: Option<f64>,
    known_amount: Option<f64>,
    paired_count: Option<f64>,
    observed: bool,
) -> Result<AnalysisTotals, String> {
    let mut result = observed_totals(count, amount, known_amount, observed)?;
    if observed && amount.is_none() {
        let denominator = paired_count.map(finite).transpose()?.filter(|count| *count > 0.);
        if let Some((sum, quantity)) = result.amount.zip(denominator) {
            result.average = Some(finite(sum / quantity)?);
            result.partial_average_quantity = Some(quantity);
        }
    }
    Ok(result)
}
fn comparable_averages(
    base: &AnalysisTotals,
    current: &AnalysisTotals,
    same_work: bool,
) -> Option<(f64, f64)> {
    let partial = base.partial_average_quantity.is_some() || current.partial_average_quantity.is_some();
    base.average.zip(current.average).filter(|_| !partial || same_work)
}

fn observed_average_change(before: &RangeSummary, after: &RangeSummary, threshold: f64) -> Result<AverageChange, String> {
    let base = &before.available_total;
    let current = &after.available_total;
    let same_work = before.source_work.as_ref().zip(after.source_work.as_ref()).is_some_and(|(b, a)| b == a);
    if let Some((b, a)) = comparable_averages(base, current, same_work) {
        return average_change(Some(b), Some(a), threshold);
    }
    Ok(AverageChange { base: base.average, current: current.average, delta: None, percent: None, flags: None })
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
