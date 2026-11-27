//! Calendar bucket queries for timelines and daily rollups.
use super::*;

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
    pub first_start: String,
    pub last_start: String,
    pub has_activity: bool,
    pub start: String,
    pub periods: i64,
    pub missing: i64,
    pub total: AnalysisTotals,
    pub available_total: AnalysisTotals,
}
#[derive(Debug, Serialize)]
pub struct TimelineSummary {
    pub granularity: String,
    pub points: Vec<TimelinePoint>,
    pub zero_buckets_with_nonzero_works: Vec<String>,
    pub buckets_with_reports: Vec<String>,
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
    pub first_start: String,
    pub last_start: String,
    pub has_activity: bool,
    pub start: String,
    pub periods: i64,
    pub missing: i64,
    pub total: AnalysisTotals,
    pub available_total: AnalysisTotals,
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
    pub buckets_with_reports: Vec<String>,
}

pub(super) fn timeline_grouping(s: &AnalysisScope, q: &RangeQuery, grouping: Option<&str>, periods: i64) -> Result<(i64, &'static str), String> {
    let Some(grouping)=grouping else { return Ok(if periods>120 {(1,"month")} else {(0,"period")}); };
    if s.report_type!="day" {return Err("Calendar grouping requires daily sources".into());}
    let (from,to)=range_bounds("day",q)?;
    match grouping {
        "day"=>Ok(if periods>120 {(1,"month")} else {(0,"period")}),
        "week" if from.weekday()==chrono::Weekday::Mon && to.weekday()==chrono::Weekday::Sun =>
            Ok(if periods/7>124 {(1,"month")} else {(2,"week")}),
        "month" if from.day()==1 && to.succ_opt().is_some_and(|d| d.day()==1) =>Ok((1,"month")),
        "year" if from.month()==1 && from.day()==1 && to.month()==12 && to.day()==31 =>Ok((3,"year")),
        _=>Err("Select aligned full calendar periods for daily grouping".into()),
    }
}

// A missing analytic value may still have a stored report. Keep incomplete,
// duplicate and overlapping reports visible, even if none can supply a value.
// Invalid reports have no trusted organization index, so keep their dates for
// every organization selection. Weekly reports can overlap within six days.
pub(super) fn fill_timeline_report_buckets(c: &Connection, mode: i64, s: &AnalysisScope) -> Result<(), String> {
    c.execute_batch("CREATE TEMP TABLE timeline_report_buckets(label TEXT PRIMARY KEY) WITHOUT ROWID;")
        .map_err(err)?;
    c.execute(r#"
        WITH bounds AS (
            SELECT CASE ?1 WHEN 1 THEN substr(start,1,7)
                           WHEN 2 THEN date(start,'-'||((cast(strftime('%w',start) AS integer)+6)%7)||' days')
                           WHEN 3 THEN substr(start,1,4)||'-01-01' ELSE start END AS label,
                   CASE WHEN ?2='week' THEN date(start,'-6 days') ELSE start END AS first_start,
                   CASE WHEN ?2='week' THEN date(start,'+6 days') ELSE start END AS last_start
            FROM temp.an_periods WHERE slot=1
        )
        INSERT INTO temp.timeline_report_buckets
        SELECT DISTINCT b.label FROM bounds b
        WHERE EXISTS (
            SELECT 1 FROM analysis_filter_items f CROSS JOIN temp.an_candidates a
            WHERE f.id=?3 AND a.org=f.item AND a.start BETWEEN b.first_start AND b.last_start
        ) OR EXISTS (
            SELECT 1 FROM reports r INDEXED BY reports_by_period JOIN analysis_reports a ON a.report_id=r.id
            WHERE r.report_type=?2 AND r.period_start BETWEEN b.first_start AND b.last_start AND a.valid=0
        )"#, params![mode, &s.report_type, &s.organizations]).map_err(err)?;
    Ok(())
}

fn timeline_report_buckets(c: &Connection, mode: i64, s: &AnalysisScope) -> Result<Vec<String>, String> {
    fill_timeline_report_buckets(c, mode, s)?;
    let mut stmt = c.prepare("SELECT label FROM temp.timeline_report_buckets ORDER BY label").map_err(err)?;
    let rows = stmt.query_map([], |row| row.get(0)).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

// Aggregate missing evidence separately to avoid multiplying amounts by detail rows.
pub(super) const TIMELINE_BUCKET_TOTALS: &str = r#"
            WITH buckets AS (
                SELECT CASE ?1 WHEN 1 THEN substr(start,1,7) WHEN 2 THEN date(start, '-' || ((CAST(strftime('%w',start) AS INTEGER)+6)%7) || ' days') WHEN 3 THEN substr(start,1,4)||'-01-01' ELSE start END AS label,
                       COUNT(*) AS periods, MIN(start) AS first_start, MAX(start) AS last_start
                FROM temp.an_periods WHERE slot=1 GROUP BY label
            ), missing AS (
                SELECT CASE ?1 WHEN 1 THEN substr(start,1,7) WHEN 2 THEN date(start, '-' || ((CAST(strftime('%w',start) AS INTEGER)+6)%7) || ' days') WHEN 3 THEN substr(start,1,4)||'-01-01' ELSE start END AS label,
                       COUNT(*) AS missing
                FROM temp.an_missing WHERE slot=1 GROUP BY label
            ), values_by_bucket AS (
                SELECT CASE ?1 WHEN 1 THEN substr(p.start,1,7) WHEN 2 THEN date(p.start, '-' || ((CAST(strftime('%w',p.start) AS INTEGER)+6)%7) || ' days') WHEN 3 THEN substr(p.start,1,4)||'-01-01' ELSE p.start END AS label,
                       SUM(x.count) AS count, CASE WHEN COUNT(x.amount)=COUNT(*) THEN SUM(x.amount) END AS amount, MAX(x.count<>0 OR x.amount IS NULL OR x.amount<>0) AS active, SUM(x.amount) AS known_amount, CASE WHEN MIN(x.work) FILTER (WHERE x.amount IS NOT NULL)=MAX(x.work) FILTER (WHERE x.amount IS NOT NULL) THEN SUM(x.count) FILTER (WHERE x.amount IS NOT NULL) END AS paired_count
                FROM temp.an_periods p
                JOIN temp.an_valid v ON v.start=p.start
                JOIN analysis_filter_items f ON f.id=?2 AND f.item=v.org
                JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org
                LEFT JOIN analysis_groups g ON g.work=x.work
                WHERE p.slot=1 AND (?3 IS NULL OR EXISTS(
                    SELECT 1 FROM analysis_filter_items names
                    WHERE names.id=?3 AND names.item=COALESCE(g.name,x.work)))
                GROUP BY label
            )
            SELECT b.label,b.periods,COALESCE(m.missing,0),
                   COALESCE(v.count,0),CASE WHEN v.count IS NULL THEN 0 ELSE v.amount END, COALESCE(v.active,0), b.first_start, b.last_start, v.known_amount, v.paired_count
            FROM buckets b
            LEFT JOIN missing m ON m.label=b.label
            LEFT JOIN values_by_bucket v ON v.label=b.label
            ORDER BY b.label
        "#;

// A nonzero work on another page keeps the period visible even when the
// selected works offset one another in the overall total.
fn zero_buckets_with_nonzero_works(
    conn: &Connection,
    mode: i64,
    scope: &AnalysisScope,
    points: &[TimelinePoint],
) -> Result<Vec<String>, String> {
    let candidates: Vec<_> = points.iter()
        .filter(|point| {
            point.missing == 0 && point.has_activity
                && point.total.count == Some(0.) && point.total.amount == Some(0.)
        })
        .map(|point| point.start.as_str())
        .collect();
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    conn.execute_batch("CREATE TEMP TABLE trend_zero_buckets(label TEXT PRIMARY KEY) WITHOUT ROWID;").map_err(err)?;
    let mut insert = conn.prepare("INSERT INTO trend_zero_buckets VALUES(?1)").map_err(err)?;
    for label in candidates {
        insert.execute([label]).map_err(err)?;
    }
    let mut stmt = conn.prepare(
        "WITH dates AS (
            SELECT start, CASE ?1 WHEN 1 THEN substr(start,1,7)
                WHEN 2 THEN date(start,'-'||((CAST(strftime('%w',start) AS INTEGER)+6)%7)||' days')
                WHEN 3 THEN substr(start,1,4)||'-01-01' ELSE start END AS label
            FROM an_periods WHERE slot=1
         ), nonzero AS (
            SELECT d.label FROM dates d JOIN trend_zero_buckets z USING(label)
            JOIN an_valid v ON v.start=d.start
            JOIN analysis_filter_items f ON f.id=?2 AND f.item=v.org
            JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org
            LEFT JOIN analysis_groups g ON g.work=x.work
            WHERE ?3 IS NULL OR EXISTS(SELECT 1 FROM analysis_filter_items n
                WHERE n.id=?3 AND n.item=COALESCE(g.name,x.work))
            GROUP BY d.label,COALESCE(g.key,'work:'||x.work)
            HAVING SUM(x.count)<>0 OR SUM(x.amount)<>0
         ) SELECT DISTINCT label FROM nonzero ORDER BY label"
    ).map_err(err)?;
    let rows = stmt.query_map(params![mode, &scope.organizations, &scope.names], |row| row.get(0)).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

impl Analytics<'_> {
    pub(super) fn analysis_timeline(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
    ) -> Result<TimelineSummary, String> {
        self.analysis_timeline_grouped(expected, s, q, None)
    }
    pub(super) fn analysis_timeline_grouped(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        grouping: Option<&str>,
    ) -> Result<TimelineSummary, String> {
        let mut c = self.database.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        fill_range(&tx, s, q, 1)?;
        let period_count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM temp.an_periods WHERE slot=1",
            [], |r| r.get(0),
        ).map_err(err)?;
        let (mode, granularity) = timeline_grouping(s,q,grouping,period_count)?;
        let organizations = selected_organization_count(&tx, s)?;
        let mut stmt = tx.prepare(TIMELINE_BUCKET_TOTALS).map_err(err)?;
        let mut rows = stmt.query(params![mode, &s.organizations, &s.names]).map_err(err)?;
        let mut points = Vec::new();
        while let Some(row) = rows.next().map_err(err)? {
            if points.len() >= 124 {
                return Err("Analysis timeline exceeds 124 points".into());
            }
            let missing: i64 = row.get(2).map_err(err)?;
            let periods: i64 = row.get(1).map_err(err)?;
            points.push(TimelinePoint {
                first_start: row.get(6).map_err(err)?,
                last_start: row.get(7).map_err(err)?,
                has_activity: row.get(5).map_err(err)?,
                start: row.get(0).map_err(err)?,
                periods: row.get(1).map_err(err)?,
                missing,
                total: totals(row.get(3).map_err(err)?, row.get(4).map_err(err)?, missing == 0)?,
                available_total: paired_observed_totals(row.get(3).map_err(err)?, row.get(4).map_err(err)?,
                    row.get(8).map_err(err)?, row.get(9).map_err(err)?, missing < periods * organizations)?,
            });
        }
        let zero_buckets_with_nonzero_works = zero_buckets_with_nonzero_works(&tx, mode, s, &points)?;
        Ok(TimelineSummary {
            granularity: granularity.into(),
            points,
            zero_buckets_with_nonzero_works,
            buckets_with_reports: timeline_report_buckets(&tx, mode, s)?,
        })
    }
    pub(super) fn analysis_organization_trends(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
    ) -> Result<OrganizationTrends, String> {
        self.analysis_organization_trends_grouped(expected, s, q, None)
    }
    pub(super) fn analysis_organization_trends_grouped(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        grouping: Option<&str>,
    ) -> Result<OrganizationTrends, String> {
        let mut c = self.database.connect()?;
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
        let (mode, granularity) = timeline_grouping(s,q,grouping,periods)?;
        // Missing evidence and values are aggregated separately, otherwise
        // a join on detail rows duplicates both missing counts and amounts.
        let sql = r#"
            WITH selected AS (
                SELECT item AS org,ordinal FROM analysis_filter_items
                WHERE id=?2 ORDER BY ordinal LIMIT 8
            ), buckets AS (
                SELECT CASE ?1 WHEN 1 THEN substr(start,1,7) WHEN 2 THEN date(start, '-' || ((CAST(strftime('%w',start) AS INTEGER)+6)%7) || ' days') WHEN 3 THEN substr(start,1,4)||'-01-01' ELSE start END AS label,
                       COUNT(*) AS periods, MIN(start) AS first_start, MAX(start) AS last_start
                FROM temp.an_periods WHERE slot=1 GROUP BY label
            ), missing AS (
                SELECT org,
                       CASE ?1 WHEN 1 THEN substr(start,1,7) WHEN 2 THEN date(start, '-' || ((CAST(strftime('%w',start) AS INTEGER)+6)%7) || ' days') WHEN 3 THEN substr(start,1,4)||'-01-01' ELSE start END AS label,
                       COUNT(*) AS missing
                FROM temp.an_missing WHERE slot=1
                GROUP BY org,label
            ), values_by_bucket AS (
                SELECT v.org,
                       CASE ?1 WHEN 1 THEN substr(p.start,1,7) WHEN 2 THEN date(p.start, '-' || ((CAST(strftime('%w',p.start) AS INTEGER)+6)%7) || ' days') WHEN 3 THEN substr(p.start,1,4)||'-01-01' ELSE p.start END AS label,
                       SUM(x.count) AS count,CASE WHEN COUNT(x.amount)=COUNT(*) THEN SUM(x.amount) END AS amount, MAX(x.count<>0 OR x.amount IS NULL OR x.amount<>0) AS active, SUM(x.amount) AS known_amount, CASE WHEN MIN(x.work) FILTER (WHERE x.amount IS NOT NULL)=MAX(x.work) FILTER (WHERE x.amount IS NOT NULL) THEN SUM(x.count) FILTER (WHERE x.amount IS NOT NULL) END AS paired_count
                FROM temp.an_periods p
                JOIN temp.an_valid v ON v.start=p.start
                JOIN selected s ON s.org=v.org
                JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org
                LEFT JOIN analysis_groups g ON g.work=x.work
                WHERE p.slot=1 AND (?3 IS NULL OR EXISTS(
                    SELECT 1 FROM analysis_filter_items names
                    WHERE names.id=?3 AND names.item=COALESCE(g.name,x.work)))
                GROUP BY v.org,label
            )
            SELECT s.org,b.label,b.periods,COALESCE(m.missing,0),
                   COALESCE(v.count,0),CASE WHEN v.count IS NULL THEN 0 ELSE v.amount END, COALESCE(v.active,0), b.first_start, b.last_start, v.known_amount, v.paired_count
            FROM selected s CROSS JOIN buckets b
            LEFT JOIN missing m ON m.org=s.org AND m.label=b.label
            LEFT JOIN values_by_bucket v ON v.org=s.org AND v.label=b.label
            ORDER BY s.ordinal,b.label
        "#;
        let mut stmt = tx.prepare(sql).map_err(err)?;
        let mut rows = stmt.query(params![mode,&s.organizations,&s.names]).map_err(err)?;
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
            let periods: i64 = row.get(2).map_err(err)?;
            current.points.push(OrganizationTrendPoint {
                first_start: row.get(7).map_err(err)?,
                last_start: row.get(8).map_err(err)?,
                has_activity: row.get(6).map_err(err)?,
                start:row.get(1).map_err(err)?,
                periods:row.get(2).map_err(err)?,
                missing,
                total:totals(row.get(4).map_err(err)?,row.get(5).map_err(err)?,missing==0)?,
                available_total: paired_observed_totals(row.get(4).map_err(err)?,row.get(5).map_err(err)?,
                    row.get(9).map_err(err)?,row.get(10).map_err(err)?,missing < periods)?,
            });
        }
        Ok(OrganizationTrends{
            granularity: granularity.into(),
            selected_organizations,
            series,
            buckets_with_reports: timeline_report_buckets(&tx, mode, s)?,
        })
    }
    pub(super) fn analysis_daily_rollup(
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
        let mut c = self.database.connect()?;
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
                       CASE WHEN COUNT(x.row_index)=0 THEN 0 WHEN COUNT(x.amount)=COUNT(x.row_index) THEN SUM(x.amount) END AS total_amount,
                       SUM(x.amount) AS known_amount,
                       CASE WHEN MIN(x.work) FILTER (WHERE x.amount IS NOT NULL)=MAX(x.work) FILTER (WHERE x.amount IS NOT NULL)
                            THEN SUM(x.count) FILTER (WHERE x.amount IS NOT NULL) END AS paired_count
                FROM temp.an_valid v
                JOIN temp.an_periods p ON p.slot=1 AND p.start=v.start
                LEFT JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org
                AND (?3 IS NULL OR EXISTS(
                    SELECT 1 FROM analysis_filter_items names
                    LEFT JOIN analysis_groups g ON g.work=x.work
                    WHERE names.id=?3 AND names.item=COALESCE(g.name,x.work)))
                GROUP BY label
            )
            SELECT b.label,b.days,COALESCE(m.absent,0),COALESCE(o.reports,0),
                   COALESCE(t.total_count,0),CASE WHEN t.label IS NULL THEN 0 ELSE t.total_amount END,t.known_amount,t.paired_count
            FROM buckets b
            LEFT JOIN missing m ON m.label=b.label
            LEFT JOIN observed o ON o.label=b.label
            LEFT JOIN sums t ON t.label=b.label
            WHERE (?1 IS NULL OR b.label>?1)
            ORDER BY b.label LIMIT ?2"
        );
        let mut stmt=tx.prepare(&sql).map_err(err)?;
        let mut rows=stmt.query(params![after,(page_size+1) as i64,&s.names]).map_err(err)?;
        let mut points=Vec::new();
        while let Some(row)=rows.next().map_err(err)? {
            if points.len()>page_size {return Err("Daily rollup result exceeds page limit".into());}
            let missing:i64=row.get(2).map_err(err)?;
            let available_reports:i64=row.get(3).map_err(err)?;
            let amount:Option<f64>=row.get(5).map_err(err)?;
            let count:f64=row.get(4).map_err(err)?;
            points.push(DailyRollupPoint {
                start:row.get(0).map_err(err)?,
                days:row.get(1).map_err(err)?,
                missing,
                available_reports,
                complete:missing==0 && available_reports>0,
                total:totals(count,amount,missing==0 && available_reports>0)?,
                available_total:paired_observed_totals(count,amount,row.get(6).map_err(err)?,
                    row.get(7).map_err(err)?,available_reports>0)?,
            });
        }
        let next_cursor=if points.len()>page_size {
            points.pop();
            points.last().map(|p|p.start.clone())
        } else {None};
        Ok(DailyRollupPage{granularity:granularity.into(),points,next_cursor})
    }

    pub(super) fn analysis_work_timeline(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        organization: Option<&str>,
        key: &str,
    ) -> Result<WorkTrendSummary, String> {
        self.analysis_work_timeline_grouped(expected, s, q, organization, key, None)
    }
    pub(super) fn analysis_work_timeline_grouped(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        organization: Option<&str>,
        key: &str,
        grouping: Option<&str>,
    ) -> Result<WorkTrendSummary, String> {
        key_bound(key)?;
        let mut c = self.database.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        org_check(&tx, s, organization)?;
        fill_range(&tx, s, q, 1)?;
        // The paged picker covers globally known identities, including works
        // absent throughout this range. Resolve identity independently of its
        // activity; coverage below decides whether absence is zero or unknown.
        // A fabricated key and a key outside an explicit name selection still
        // fail instead of becoming a made-up zero-valued service.
        let descriptor: Option<(String,String)> = if let Some(work)=key.strip_prefix("work:") {
            tx.query_row(
                "SELECT x.work,'work' FROM report_rows x WHERE x.work=?1
                 AND NOT EXISTS(SELECT 1 FROM analysis_groups g WHERE g.work=x.work)
                 GROUP BY x.work LIMIT 1",[work],|r|Ok((r.get(0)?,r.get(1)?)),
            ).optional().map_err(err)?
        } else if key.starts_with("group:") {
            tx.query_row("SELECT name,'group' FROM analysis_groups WHERE key=?1 GROUP BY name LIMIT 1",[key],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(err)?
        } else {None};
        let (name,kind)=descriptor.ok_or("Selected work/group not found in analysis scope")?;
        if let Some(filter)=s.names.as_deref() {
            let selected:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM analysis_filter_items WHERE id=?1 AND item=?2)",params![filter,name],|r|r.get(0)).map_err(err)?;
            if !selected {return Err("Selected work/group not found in analysis scope".into());}
        }
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM temp.an_periods WHERE slot=1",
            [], |row| row.get(0),
        ).map_err(err)?;
        let (mode, granularity) = timeline_grouping(s,q,grouping,count)?;
        let sql = r#"
            WITH buckets AS (
                SELECT CASE ?1 WHEN 1 THEN substr(start,1,7) WHEN 2 THEN date(start, '-' || ((CAST(strftime('%w',start) AS INTEGER)+6)%7) || ' days') WHEN 3 THEN substr(start,1,4)||'-01-01' ELSE start END AS label,
                       COUNT(*) AS periods, MIN(start) AS first_start, MAX(start) AS last_start
                FROM temp.an_periods WHERE slot=1 GROUP BY label
            ), missing AS (
                SELECT CASE ?1 WHEN 1 THEN substr(start,1,7) WHEN 2 THEN date(start, '-' || ((CAST(strftime('%w',start) AS INTEGER)+6)%7) || ' days') WHEN 3 THEN substr(start,1,4)||'-01-01' ELSE start END AS label,
                       COUNT(*) AS missing
                FROM temp.an_missing
                WHERE slot=1 AND (?2 IS NULL OR org=?2) GROUP BY label
            ), aggregates AS (
                SELECT CASE ?1 WHEN 1 THEN substr(p.start,1,7) WHEN 2 THEN date(p.start, '-' || ((CAST(strftime('%w',p.start) AS INTEGER)+6)%7) || ' days') WHEN 3 THEN substr(p.start,1,4)||'-01-01' ELSE p.start END AS label,
                       SUM(x.count) AS count,CASE WHEN COUNT(x.amount)=COUNT(*) THEN SUM(x.amount) END AS amount, MAX(x.count<>0 OR x.amount IS NULL OR x.amount<>0) AS active, SUM(x.amount) AS known_amount, CASE WHEN MIN(x.work) FILTER (WHERE x.amount IS NOT NULL)=MAX(x.work) FILTER (WHERE x.amount IS NOT NULL) THEN SUM(x.count) FILTER (WHERE x.amount IS NOT NULL) END AS paired_count
                FROM temp.an_periods p
                JOIN temp.an_valid v ON v.start=p.start
                JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org
                LEFT JOIN analysis_groups g ON g.work=x.work
                WHERE p.slot=1 AND (?2 IS NULL OR v.org=?2)
                  AND COALESCE(g.key,'work:'||x.work)=?3
                GROUP BY label
            )
            SELECT b.label,b.periods,COALESCE(m.missing,0),
                   COALESCE(a.count,0),CASE WHEN a.count IS NULL THEN 0 ELSE a.amount END, COALESCE(a.active,0), b.first_start, b.last_start, a.known_amount, a.paired_count
            FROM buckets b
            LEFT JOIN missing m ON m.label=b.label
            LEFT JOIN aggregates a ON a.label=b.label
            ORDER BY b.label
        "#;
        let organizations = if organization.is_some() { 1 } else { selected_organization_count(&tx, s)? };
        let mut stmt = tx.prepare(sql).map_err(err)?;
        let mut rows = stmt.query(params![mode,organization,key]).map_err(err)?;
        let mut points=Vec::new();
        while let Some(row)=rows.next().map_err(err)? {
            if points.len()>=124 {return Err("Work timeline exceeds 124 points".into());}
            let missing:i64=row.get(2).map_err(err)?;
            let periods:i64=row.get(1).map_err(err)?;
            points.push(TimelinePoint{
                first_start: row.get(6).map_err(err)?,
                last_start: row.get(7).map_err(err)?,
                has_activity: row.get(5).map_err(err)?,
                start:row.get(0).map_err(err)?,
                periods:row.get(1).map_err(err)?,
                missing,
                total:totals(row.get(3).map_err(err)?,row.get(4).map_err(err)?,missing==0)?,
                available_total:paired_observed_totals(row.get(3).map_err(err)?,row.get(4).map_err(err)?,
                    row.get(8).map_err(err)?,row.get(9).map_err(err)?,missing < periods * organizations)?,
            });
        }
        Ok(WorkTrendSummary {
            key:key.to_string(),
            name,kind,
            granularity:granularity.into(),
            points,
        })
    }
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
        self.analysis().analysis_timeline(expected, s, q)
    }

    pub fn analysis_timeline_grouped(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        grouping: Option<&str>,
    ) -> Result<TimelineSummary, String> {
        self.analysis().analysis_timeline_grouped(expected, s, q, grouping)
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
        self.analysis().analysis_organization_trends(expected, s, q)
    }

    pub fn analysis_organization_trends_grouped(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        grouping: Option<&str>,
    ) -> Result<OrganizationTrends, String> {
        self.analysis().analysis_organization_trends_grouped(expected, s, q, grouping)
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
        self.analysis().analysis_daily_rollup(expected, s, q, granularity, after, page_size)
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
        self.analysis().analysis_work_timeline(expected, s, q, organization, key)
    }

    pub fn analysis_work_timeline_grouped(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        organization: Option<&str>,
        key: &str,
        grouping: Option<&str>,
    ) -> Result<WorkTrendSummary, String> {
        self.analysis().analysis_work_timeline_grouped(expected, s, q, organization, key, grouping)
    }
}
