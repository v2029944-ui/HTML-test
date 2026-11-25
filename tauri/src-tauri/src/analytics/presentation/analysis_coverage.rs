//! Read-only coverage facts over indexed, page-local report windows.
use super::*;
use std::collections::BTreeSet;

#[derive(Debug, Serialize)]
pub struct QualityMatrixPeriod {
    pub start: String,
    pub end: String,
    pub unassigned_invalid: i64,
}
#[derive(Debug, Serialize)]
pub struct QualityMatrixCell {
    pub start: String,
    pub available: bool,
    pub matching_reports: i64,
    pub invalid_reports: i64,
    pub duplicate_reports: i64,
    pub overlap_reports: i64,
    pub shifted_reports: i64,
    pub warned_reports: i64,
    pub count: Option<f64>,
    pub amount: Option<f64>,
    pub numeric_zero: bool,
    pub has_activity: bool,
}
#[derive(Debug, Serialize)]
pub struct QualityMatrixRow {
    pub organization: String,
    pub cells: Vec<QualityMatrixCell>,
}
#[derive(Debug, Serialize)]
pub struct QualityMatrixPage {
    pub revision: i64,
    pub report_type: String,
    pub periods: Vec<QualityMatrixPeriod>,
    pub rows: Vec<QualityMatrixRow>,
}
#[derive(Debug, Serialize)]
pub struct QualityMatrixReport {
    pub id: String,
    pub start: String,
    pub end: String,
    pub source_name: Option<String>,
    pub valid: bool,
    pub issue: Option<String>,
    pub warned: bool,
    pub duplicate: bool,
    pub overlap: bool,
    pub shifted: bool,
}
#[derive(Debug, Serialize)]
pub struct QualityMatrixReportsPage {
    pub total_rows: i64,
    pub revision: i64,
    pub rows: Vec<QualityMatrixReport>,
    pub next: Option<String>,
}

// Explicit range access avoids a scan through unrelated historical JSON or rows.
const CANDIDATES_SQL: &str = "SELECT r.id,r.period_start,r.period_end,r.source_name,a.valid,a.issue,a.warned
    FROM reports r INDEXED BY reports_by_period JOIN analysis_reports a ON a.report_id=r.id
    WHERE r.report_type=?1 AND r.period_start BETWEEN ?2 AND ?3
      AND (?4 IS NULL OR EXISTS(SELECT 1 FROM analysis_orgs o INDEXED BY analysis_org_lookup WHERE o.org=?4 AND o.report_id=r.id))
      AND (?5 IS NULL OR r.id>?5)";
const PARTNER_SQL: &str = "SELECT
    EXISTS(SELECT 1 FROM reports b INDEXED BY reports_by_period
      JOIN analysis_reports m ON m.report_id=b.id AND m.valid=1
      JOIN analysis_orgs o INDEXED BY analysis_org_lookup ON o.org=?4 AND o.report_id=b.id
      WHERE b.report_type=?1 AND b.period_start BETWEEN ?2 AND ?3 AND b.period_end>=a.period_start
        AND b.id<>a.id AND b.period_start=a.period_start AND b.period_end=a.period_end),
    EXISTS(SELECT 1 FROM reports b INDEXED BY reports_by_period
      JOIN analysis_reports m ON m.report_id=b.id AND m.valid=1
      JOIN analysis_orgs o INDEXED BY analysis_org_lookup ON o.org=?4 AND o.report_id=b.id
      WHERE b.report_type=?1 AND b.period_start BETWEEN ?2 AND ?3 AND b.period_end>=a.period_start
        AND b.id<>a.id AND (b.period_start<>a.period_start OR b.period_end<>a.period_end))
    FROM reports a WHERE a.id=?5";

fn organization(value: &str) -> Result<(), String> {
    bounded(value)?;
    if value.is_empty() {
        return Err("Select a nonempty organization".into());
    }
    Ok(())
}
fn window(kind: &str, start: NaiveDate, stop: NaiveDate) -> Result<(String, String), String> {
    date(&stop.to_string())?;
    let halo = if kind == "week" { 6 } else { 0 };
    // Clamp only the search halo, never the requested period itself.
    let first = start
        .checked_sub_signed(Duration::days(halo))
        .filter(|d| d.year() >= 0)
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(0, 1, 1).unwrap());
    // Valid daily/monthly reports can intersect only an identical period start.
    Ok((
        first.to_string(),
        if kind == "week" { stop } else { start }.to_string(),
    ))
}
fn require_manifests(c: &Connection, kind: &str, lower: &str, upper: &str) -> Result<(), String> {
    let missing:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM reports r INDEXED BY reports_by_period LEFT JOIN analysis_reports a ON a.report_id=r.id WHERE r.report_type=?1 AND r.period_start BETWEEN ?2 AND ?3 AND a.report_id IS NULL)",params![kind,lower,upper],|r|r.get(0)).map_err(err)?;
    if missing {
        return Err(
            "Coverage contains unindexed reports or conflict partners; index this report window"
                .into(),
        );
    }
    Ok(())
}
fn conflicts(
    c: &Connection,
    kind: &str,
    report: &QualityMatrixReport,
    org: &str,
) -> Result<(bool, bool), String> {
    if !report.valid {
        return Ok((false, false));
    }
    let (lower, upper) = window(kind, date(&report.start)?, date(&report.end)?)?;
    require_manifests(c, kind, &lower, &upper)?;
    c.query_row(
        PARTNER_SQL,
        params![kind, lower, upper, org, report.id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .map_err(err)
}
fn report(r: &rusqlite::Row<'_>, expected: &str) -> rusqlite::Result<QualityMatrixReport> {
    let start: String = r.get(1)?;
    Ok(QualityMatrixReport {
        id: r.get(0)?,
        shifted: start != expected,
        start,
        end: r.get(2)?,
        source_name: r.get(3)?,
        valid: r.get(4)?,
        issue: r.get(5)?,
        warned: r.get(6)?,
        duplicate: false,
        overlap: false,
    })
}
fn matrix_cell(
    c: &Connection,
    kind: &str,
    period: &QualityMatrixPeriod,
    org: &str,
) -> Result<QualityMatrixCell, String> {
    let (lower, upper) = window(kind, date(&period.start)?, date(&period.end)?)?;
    let mut cell = QualityMatrixCell {
        start: period.start.clone(),
        available: false,
        matching_reports: 0,
        invalid_reports: 0,
        duplicate_reports: 0,
        overlap_reports: 0,
        shifted_reports: 0,
        warned_reports: 0,
        count: None,
        amount: None,
        numeric_zero: false,
        has_activity: false,
    };
    let mut usable = None;
    let mut exact_valid = 0;
    let mut statement = c.prepare(CANDIDATES_SQL).map_err(err)?;
    let mut rows = statement
        .query(params![kind, lower, upper, org, None::<&str>])
        .map_err(err)?;
    while let Some(row) = rows.next().map_err(err)? {
        let r = report(row, &period.start).map_err(err)?;
        if r.shifted {
            if r.valid && r.end >= period.start {
                cell.shifted_reports += 1;
                cell.warned_reports += i64::from(r.warned);
            }
            continue;
        }
        cell.matching_reports += 1;
        cell.warned_reports += i64::from(r.warned);
        if !r.valid {
            cell.invalid_reports += 1;
            continue;
        }
        exact_valid += 1;
        let (duplicate, overlap) = conflicts(c, kind, &r, org)?;
        cell.duplicate_reports += i64::from(duplicate);
        cell.overlap_reports += i64::from(overlap);
        usable = Some(r.id);
    }
    cell.available = exact_valid == 1 && cell.duplicate_reports == 0 && cell.overlap_reports == 0;
    if cell.available {
        let (count,amount,activity):(f64,Option<f64>,bool)=c.query_row("SELECT coalesce(sum(count),0.0),CASE WHEN COUNT(*)=0 THEN 0 WHEN COUNT(amount)=COUNT(*) THEN SUM(amount) END,coalesce(max(count<>0 OR amount IS NULL OR amount<>0),0) FROM report_rows WHERE report_id=?1 AND organization=?2",params![usable.unwrap(),org],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(err)?;
        cell.count = Some(finite(count)?);
        cell.amount = amount.map(finite).transpose()?;
        cell.numeric_zero = count == 0. && amount == Some(0.);
        cell.has_activity = activity;
    }
    Ok(cell)
}
fn binding(revision: i64, kind: &str, start: &str, org: Option<&str>) -> Result<String, String> {
    let state = serde_json::to_vec(&(revision, kind, start, org)).map_err(err)?;
    Ok(format!("{:x}", Sha256::digest(state)))
}
impl Analytics<'_> {
    pub(super) fn quality_matrix_page(
        &self,
        revision: i64,
        report_type: &str,
        from: &str,
        to: &str,
        organizations: &[String],
    ) -> Result<QualityMatrixPage, String> {
        if !(1..=8).contains(&organizations.len())
            || organizations.iter().collect::<BTreeSet<_>>().len() != organizations.len()
        {
            return Err("Select 1..=8 unique organizations".into());
        }
        for org in organizations {
            organization(org)?;
        }
        let (first, last) = range_bounds(
            report_type,
            &RangeQuery {
                from: from.into(),
                to: to.into(),
            },
        )?;
        let cap = if report_type == "day" { 31 } else { 12 };
        let mut periods = Vec::new();
        let mut current = first;
        loop {
            if periods.len() >= cap {
                return Err("Coverage range exceeds the period page limit".into());
            }
            periods.push(QualityMatrixPeriod {
                start: current.to_string(),
                end: end(report_type, current)?.to_string(),
                unassigned_invalid: 0,
            });
            if current == last {
                break;
            }
            current = shift(report_type, current, 1)?;
        }
        let mut connection = self.database.connect()?;
        let tx = connection.transaction().map_err(err)?;
        check_revision(&tx, revision)?;
        check_metadata_bounds(&tx)?;
        for period in &mut periods {
            let (lower, upper) = window(report_type, date(&period.start)?, date(&period.end)?)?;
            require_manifests(&tx, report_type, &lower, &upper)?;
            period.unassigned_invalid=tx.query_row("SELECT count(*) FROM reports r INDEXED BY reports_by_period JOIN analysis_reports a ON a.report_id=r.id AND a.valid=0 WHERE r.report_type=?1 AND r.period_start=?2 AND NOT EXISTS(SELECT 1 FROM analysis_orgs o WHERE o.report_id=r.id)",params![report_type,period.start],|r|r.get(0)).map_err(err)?;
        }
        let mut rows = Vec::with_capacity(organizations.len());
        for org in organizations {
            let cells = periods
                .iter()
                .map(|p| matrix_cell(&tx, report_type, p, org))
                .collect::<Result<Vec<_>, _>>()?;
            rows.push(QualityMatrixRow {
                organization: org.clone(),
                cells,
            });
        }
        Ok(QualityMatrixPage {
            revision,
            report_type: report_type.into(),
            periods,
            rows,
        })
    }
    pub(super) fn quality_matrix_reports(
        &self,
        revision: i64,
        report_type: &str,
        start: &str,
        organization: Option<&str>,
        after: Option<&str>,
        limit: usize,
    ) -> Result<QualityMatrixReportsPage, String> {
        self.quality_reports_window(revision, report_type, start, organization, after, 0, limit)
    }

    pub(super) fn quality_matrix_reports_at_page(
        &self, revision: i64, report_type: &str, start: &str, organization: Option<&str>,
        page_index: usize, limit: usize,
    ) -> Result<QualityMatrixReportsPage, String> {
        self.quality_reports_window(revision, report_type, start, organization, None, page_index, limit)
    }

    fn quality_reports_window(
        &self, revision: i64, report_type: &str, start: &str, organization: Option<&str>,
        after: Option<&str>, page_index: usize, limit: usize,
    ) -> Result<QualityMatrixReportsPage, String> {
        if !(1..=20).contains(&limit) {
            return Err("Coverage report page limit must be 1..=20".into());
        }
        if let Some(org) = organization {
            self::organization(org)?;
        }
        let (first, _) = range_bounds(
            report_type,
            &RangeQuery {
                from: start.into(),
                to: start.into(),
            },
        )?;
        let last = end(report_type, first)?;
        let scope = binding(revision, report_type, start, organization)?;
        let prefix = format!("{scope}:");
        let after_id = after
            .map(|cursor| {
                let id = cursor
                    .strip_prefix(&prefix)
                    .ok_or("Coverage cursor does not match revision or query")?;
                bounded(id)?;
                Ok::<_, String>(id)
            })
            .transpose()?;
        let mut connection = self.database.connect()?;
        let tx = connection.transaction().map_err(err)?;
        check_revision(&tx, revision)?;
        check_metadata_bounds(&tx)?;
        let (lower, upper) = if organization.is_some() {
            window(report_type, first, last)?
        } else {
            (start.into(), start.into())
        };
        require_manifests(&tx, report_type, &lower, &upper)?;
        let predicate = if organization.is_some() {
            " AND (r.period_start=?6 OR (a.valid=1 AND r.period_end>=?6))"
        } else {
            " AND r.period_start=?6 AND a.valid=0 AND NOT EXISTS(SELECT 1 FROM analysis_orgs o WHERE o.report_id=r.id)"
        };
        let candidates = format!("{CANDIDATES_SQL}{predicate}");
        let total_rows = tx.query_row(
            &format!("SELECT COUNT(*) FROM ({candidates})"),
            params![report_type, lower, upper, organization, Option::<&str>::None, start],
            |row| row.get(0),
        ).map_err(err)?;
        let offset = direct_page_offset(page_index, limit, total_rows)?;
        let mut statement = tx
            .prepare(&format!(
                "{candidates} ORDER BY r.id LIMIT ?7 OFFSET ?8"
            ))
            .map_err(err)?;
        let mut result = statement
            .query(params![
                report_type,
                lower,
                upper,
                organization,
                after_id,
                start,
                limit as i64 + 1,
                offset
            ])
            .map_err(err)?;
        let mut rows = Vec::with_capacity(limit + 1);
        while let Some(row) = result.next().map_err(err)? {
            let mut r = report(row, start).map_err(err)?;
            if let Some(reason) = &r.issue {
                bounded(reason)?;
            }
            if let Some(org) = organization {
                (r.duplicate, r.overlap) = conflicts(&tx, report_type, &r, org)?;
            }
            rows.push(r);
        }
        let next = if rows.len() > limit {
            rows.pop();
            rows.last().map(|r| format!("{prefix}{}", r.id))
        } else {
            None
        };
        Ok(QualityMatrixReportsPage {
            total_rows,
            revision,
            rows,
            next,
        })
    }
}

impl Storage {
    pub fn quality_matrix_page(
        &self,
        revision: i64,
        report_type: &str,
        from: &str,
        to: &str,
        organizations: &[String],
    ) -> Result<QualityMatrixPage, String> {
        self.analysis().quality_matrix_page(revision, report_type, from, to, organizations)
    }

    pub fn quality_matrix_reports(
        &self,
        revision: i64,
        report_type: &str,
        start: &str,
        organization: Option<&str>,
        after: Option<&str>,
        limit: usize,
    ) -> Result<QualityMatrixReportsPage, String> {
        self.analysis().quality_matrix_reports(revision, report_type, start, organization, after, limit)
    }

    pub fn quality_matrix_reports_at_page(
        &self, revision: i64, report_type: &str, start: &str, organization: Option<&str>,
        page_index: usize, limit: usize,
    ) -> Result<QualityMatrixReportsPage, String> {
        self.analysis().quality_matrix_reports_at_page(revision, report_type, start, organization, page_index, limit)
    }
}
#[cfg(test)]
#[path = "../tests/analysis_coverage_tests.rs"]
mod tests;
