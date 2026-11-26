//! Comparison summaries and pages over two independent ranges.
use super::*;

#[derive(Debug, Serialize)]
pub struct ComparisonValue {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub status: Option<String>,
    pub count: Option<ChangeMeasure>,
    pub amount: Option<ChangeMeasure>,
    pub average: AverageChange,
    /// Exact values of the sole usable side when a delta cannot be computed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available: Option<AnalysisTotals>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_before: Option<AnalysisTotals>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_after: Option<AnalysisTotals>,
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

pub(super) fn comparison(
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
    c.execute_batch("CREATE TEMP TABLE an_comparison(key TEXT PRIMARY KEY,name TEXT,kind TEXT,bc REAL,ba REAL,cc REAL,ca REAL,b_known REAL,a_known REAL,b_paired REAL,a_paired REAL,same_source_work INTEGER) WITHOUT ROWID;").map_err(err)?;
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
    c.execute(&format!("INSERT INTO temp.an_comparison WITH b AS (SELECT * FROM {table} WHERE slot=1 {predicate}),a AS (SELECT * FROM {table} WHERE slot=2 {predicate}),keys AS (SELECT key FROM b UNION SELECT key FROM a) SELECT keys.key,COALESCE(b.name,a.name),COALESCE(b.kind,a.kind),COALESCE(b.count,0),CASE WHEN b.key IS NULL THEN 0 ELSE b.amount END,COALESCE(a.count,0),CASE WHEN a.count IS NULL THEN 0 ELSE a.amount END,b.known_amount,a.known_amount,CASE WHEN b.source_work IS NOT NULL THEN b.paired_count END,CASE WHEN a.source_work IS NOT NULL THEN a.paired_count END,b.source_work IS NOT NULL AND b.source_work=a.source_work FROM keys LEFT JOIN b USING(key) LEFT JOIN a USING(key)"),[org]).map_err(err)?;
    c.execute("INSERT INTO temp.an_comparison SELECT 'absent:'||f.item,f.item,'work',0,0,0,0,NULL,NULL,NULL,NULL,0 FROM analysis_filter_items f WHERE f.id=?1 AND NOT EXISTS(SELECT 1 FROM temp.an_comparison a WHERE a.name=f.item)",[&s.names]).map_err(err)?;
    Ok((complete(c, 1, org)?, complete(c, 2, org)?))
}
pub(super) fn comparison_value(
    r: &rusqlite::Row<'_>,
    q: &ComparisonQuery,
    b: bool,
    a: bool,
) -> Result<ComparisonValue, String> {
    let bc: f64 = r.get(3).map_err(err)?;
    let ba: Option<f64> = r.get(4).map_err(err)?;
    let cc: f64 = r.get(5).map_err(err)?;
    let ca: Option<f64> = r.get(6).map_err(err)?;
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
            ba.zip(ca).map(|(b,a)| measure(b,a,q.min_amount,q.percent)).transpose()?
        } else {
            None
        },
        average: average_change(bt.average, at.average, q.percent)?,
        observed_before: None,
        observed_after: None,
        available: match (b,a) {
            (true,false) => Some(bt),
            (false,true) => Some(at),
            _ => None,
        },
    })
}
impl Analytics<'_> {
    pub(super) fn analysis_comparison_summary(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ComparisonQuery,
    ) -> Result<ComparisonSummary, String> {
        let mut c = self.database.connect()?;
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
    pub(super) fn analysis_comparison_dashboard(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ComparisonQuery,
    ) -> Result<ComparisonDashboard, String> {
        let mut c = self.database.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        let (before_complete, after_complete) = comparison(&tx, s, q, None)?;
        let before = range_summary(&tx, s, 1)?;
        let after = range_summary(&tx, s, 2)?;
        let count = if let (Some(b), Some(a)) = (before.available_total.count, after.available_total.count) {
            Some(measure(b, a, q.min_count, q.percent)?)
        } else { None };
        let amount = if let (Some(b), Some(a)) = (before.comparison_amount(), after.comparison_amount()) {
            Some(measure(b, a, q.min_amount, q.percent)?)
        } else { None };
        let average = observed_average_change(&before, &after, q.percent)?;
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
                let (c,a):(f64,Option<f64>) = tx.query_row(
                    "SELECT COALESCE(SUM(count),0),CASE WHEN COUNT(*)=0 THEN 0 WHEN COUNT(amount)=COUNT(*) THEN SUM(amount) END
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
        if summary.before.available_reports > 0 && summary.after.available_reports > 0 {
            for metric in ["amount","count"] {
                let sql = "SELECT key,name,kind,bc,ba,cc,ca FROM temp.an_comparison
                    WHERE ?1='count' OR (ba IS NOT NULL AND ca IS NOT NULL)
                    ORDER BY ABS(CASE WHEN ?1='amount' THEN ca-ba ELSE cc-bc END) DESC,
                             key ASC LIMIT 12";
                let mut ranking = tx.prepare(sql).map_err(err)?;
                let mut values=ranking.query([metric]).map_err(err)?;
                while let Some(row)=values.next().map_err(err)? {
                    let mut item=comparison_value(row,q,true,true)?;
                    if !(before_complete && after_complete) {
                        item.status = None;
                    }
                    if metric=="amount" {top_amount.push(item);}
                    else {top_count.push(item);}
                }
            }
        }
        Ok(ComparisonDashboard{summary,divisions,top_amount,top_count})
    }
    pub(super) fn analysis_comparison_window(&self,expected:i64,s:&AnalysisScope,q:&ComparisonQuery,org:Option<&str>,page_index:usize,size:usize)->Result<SqlPage<ComparisonValue>,String> {
        let limit=cap(size)?;
        let mut connection=self.database.connect()?;
        let tx=connection.transaction().map_err(err)?;
        workspace(&tx,expected,s)?;
        let (before,after)=comparison(&tx,s,q,org)?;
        let total_rows=count_sql(&tx,"SELECT count(*) FROM temp.an_comparison")?;
        let offset=crate::storage::direct_page_offset(page_index,size,total_rows)?;
        let b_observed = observed_reports(&tx,1,org)?;
        let a_observed = observed_reports(&tx,2,org)?;
        let mut statement=tx.prepare("SELECT key,name,kind,bc,ba,cc,ca,b_known,a_known,b_paired,a_paired,same_source_work FROM temp.an_comparison ORDER BY key LIMIT ?1 OFFSET ?2").map_err(err)?;
        let mut result=statement.query(params![limit,offset]).map_err(err)?;
        let mut rows=Vec::new();
        while let Some(row)=result.next().map_err(err)? {
            let mut value = comparison_value(row,q,b_observed,a_observed)?;
            if !(before && after) {
                value.status = None;
            }
            let base = paired_observed_totals(row.get(3).map_err(err)?, row.get(4).map_err(err)?,
                row.get(7).map_err(err)?, row.get(9).map_err(err)?, b_observed)?;
            let current = paired_observed_totals(row.get(5).map_err(err)?, row.get(6).map_err(err)?,
                row.get(8).map_err(err)?, row.get(10).map_err(err)?, a_observed)?;
            let same_source_work = row.get::<_, Option<bool>>(11).map_err(err)?.unwrap_or(false);
            if same_source_work {
                value.average = average_change(base.average, current.average, q.percent)?;
            }
            value.observed_before = Some(base);
            value.observed_after = Some(current);
            rows.push(value);
        }
        Ok(SqlPage {rows,total_rows})
    }
    pub(super) fn analysis_comparison_page(
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
        let mut c = self.database.connect()?;
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

impl Storage {
    pub fn analysis_comparison_summary(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ComparisonQuery,
    ) -> Result<ComparisonSummary, String> {
        self.analysis().analysis_comparison_summary(expected, s, q)
    }

    /// One bounded, revision-consistent comparison of two exact date ranges.
    /// Totals and work rankings compare observed reports in both ranges.
    /// Division comparisons still require complete coverage for that division.
    pub fn analysis_comparison_dashboard(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ComparisonQuery,
    ) -> Result<ComparisonDashboard, String> {
        self.analysis().analysis_comparison_dashboard(expected, s, q)
    }

    pub fn analysis_comparison_window(&self,expected:i64,s:&AnalysisScope,q:&ComparisonQuery,org:Option<&str>,page_index:usize,size:usize)->Result<SqlPage<ComparisonValue>,String> {
        self.analysis().analysis_comparison_window(expected, s, q, org, page_index, size)
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
        self.analysis().analysis_comparison_page(expected, s, q, org, after, size)
    }
}
