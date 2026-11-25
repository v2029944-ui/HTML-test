//! Range summaries, ranking and paged report evidence.
use super::*;

#[derive(Debug, Serialize)]
pub struct RangeSummary {
    pub single_missing: Option<MissingPeriod>,
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
    pub amount_missing_rows: bool,
    pub invalid_reports: i64,
    pub duplicate_issues: i64,
    pub overlap_issues: i64,
    pub import_warning_reports: i64,
}

impl RangeSummary {
    pub(super) fn comparison_amount(&self) -> Option<f64> {
        self.available_total.amount.filter(|_| !self.amount_missing_rows)
    }
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
    pub available_total: AnalysisTotals,
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

pub(super) fn range_summary(c: &Connection, s: &AnalysisScope, slot: i64) -> Result<RangeSummary, String> {
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
    let (values,count,amount,known_amount):(i64,f64,Option<f64>,Option<f64>)=c.query_row("SELECT COUNT(*),COALESCE(SUM(count),0),CASE WHEN COUNT(*)=0 THEN 0 WHEN COUNT(amount)=COUNT(*) THEN SUM(amount) END,SUM(known_amount) FROM temp.an_combined WHERE slot=?1",[slot],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(err)?;
    let complete = periods > 0 && organizations > 0 && missing == 0;
    let available_reports: i64 = c.query_row(
        "SELECT COUNT(*) FROM temp.an_valid v
         JOIN temp.an_periods p ON p.slot=?1 AND p.start=v.start",
        [slot],
        |r| r.get(0),
    ).map_err(err)?;
    let single_missing = if missing == 1 {
        Some(
            c.query_row(
                "SELECT org,start FROM temp.an_missing WHERE slot=?1",
                [slot],
                |r| {
                    Ok(MissingPeriod {
                        org: r.get(0)?,
                        start: r.get(1)?,
                    })
                },
            )
            .map_err(err)?,
        )
    } else {
        None
    };
    Ok(RangeSummary {
        single_missing,
        complete,
        periods,
        organizations,
        missing,
        values,
        total: totals(count, amount, complete)?,
        available_reports,
        available_total: observed_totals(count, amount, known_amount, available_reports > 0)?,
        amount_missing_rows: amount.is_none(),
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
pub(super) fn org_check(c: &Connection, s: &AnalysisScope, org: Option<&str>) -> Result<(), String> {
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

impl Analytics<'_> {
    pub(super) fn analysis_ranking(
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
        if !matches!(category, "all" | "work" | "group" | "selectable") {
            return Err("Unknown ranking category".into());
        }
        let mut c = self.database.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        org_check(&tx, s, org)?;
        fill_range(&tx, s, q, 1)?;
        if !complete(&tx, 1, org)? {
            return Ok(RankingSummary { complete: false, rows: Vec::new() });
        }
        // The manual work filter accepts unique names of at most 512 UTF-8 bytes.
        // Apply its catalog before the ranking limit, preserving the unrestricted ranking.
        let names_filter = if category == "selectable" {
            create_name_catalog(&tx, false)?;
            "AND key IN (SELECT key FROM unified_names)"
        } else {
            ""
        };
        let (table, division) = if org.is_some() {
            ("temp.an_values", "org=?3")
        } else {
            ("temp.an_combined", "?3 IS NULL")
        };
        let sql = format!(
            "SELECT key,name,kind,count,amount,active FROM {table}
             WHERE slot=1 AND {division} AND (?1 IN ('all','selectable') OR kind=?1) {names_filter}
               AND (?2='count' OR amount IS NOT NULL)
             ORDER BY ABS(CASE WHEN ?2='count' THEN count ELSE amount END) DESC, key ASC LIMIT 12"
        );
        let mut stmt = tx.prepare(&sql).map_err(err)?;
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
    pub(super) fn analysis_range_summary(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
    ) -> Result<RangeSummary, String> {
        self.with_snapshot(expected, s, |tx| {
            fill_range(tx, s, q, 1)?;
            range_summary(tx, s, 1)
        })
    }
    pub(super) fn analysis_range_page(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        org: Option<&str>,
        after: Option<&str>,
        size: usize,
    ) -> Result<Vec<RangeValue>, String> {
        let n = cap(size)?;
        self.with_snapshot(expected, s, |tx| {
            org_check(tx, s, org)?;
            fill_range(tx, s, q, 1)?;
            range_rows(tx, 1, org, after, n)
        })
    }
    pub(super) fn analysis_missing_page(
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
        let mut c = self.database.connect()?;
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
    pub(super) fn analysis_organization_page(
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
        let mut c = self.database.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        fill_range(&tx, s, q, 1)?;
        let mut stmt=tx.prepare("SELECT f.item,(SELECT COUNT(*) FROM temp.an_missing m WHERE slot=1 AND m.org=f.item),COALESCE(SUM(v.count),0),CASE WHEN COUNT(v.key)=0 THEN 0 WHEN COUNT(v.amount)=COUNT(v.key) THEN SUM(v.amount) END,SUM(v.known_amount) FROM analysis_filter_items f LEFT JOIN temp.an_values v ON v.slot=1 AND v.org=f.item WHERE f.id=?1 AND (?2 IS NULL OR f.item>?2) GROUP BY f.item ORDER BY f.item LIMIT ?3").map_err(err)?;
        let mut rows = stmt
            .query(params![s.organizations, after, n])
            .map_err(err)?;
        let periods = count_sql(&tx, "SELECT COUNT(*) FROM temp.an_periods WHERE slot=1")?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().map_err(err)? {
            let missing: i64 = r.get(1).map_err(err)?;
            out.push(OrganizationSummary {
                org: r.get(0).map_err(err)?,
                complete: missing == 0,
                missing,
                total: totals(r.get(2).map_err(err)?, r.get(3).map_err(err)?, missing == 0)?,
                available_total: observed_totals(r.get(2).map_err(err)?, r.get(3).map_err(err)?,
                    r.get(4).map_err(err)?, missing < periods)?,
            })
        }
        Ok(out)
    }
    pub(super) fn analysis_issue_page(
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
        let mut c = self.database.connect()?;
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

impl Storage {
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
        self.analysis().analysis_ranking(expected, s, q, org, metric, category)
    }

    pub fn analysis_range_summary(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
    ) -> Result<RangeSummary, String> {
        self.analysis().analysis_range_summary(expected, s, q)
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
        self.analysis().analysis_range_page(expected, s, q, org, after, size)
    }

    pub fn analysis_missing_page(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        after: Option<(&str, &str)>,
        size: usize,
    ) -> Result<Vec<MissingPeriod>, String> {
        self.analysis().analysis_missing_page(expected, s, q, after, size)
    }

    pub fn analysis_organization_page(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        after: Option<&str>,
        size: usize,
    ) -> Result<Vec<OrganizationSummary>, String> {
        self.analysis().analysis_organization_page(expected, s, q, after, size)
    }

    pub fn analysis_issue_page(
        &self,
        expected: i64,
        s: &AnalysisScope,
        after: Option<&IssueCursor>,
        size: usize,
    ) -> Result<Vec<AnalysisIssue>, String> {
        self.analysis().analysis_issue_page(expected, s, after, size)
    }
}
