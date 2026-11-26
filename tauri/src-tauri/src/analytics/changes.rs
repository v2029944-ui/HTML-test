//! Pair coverage, numeric change measures and detail pages.
use super::*;

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
    pub amount: Option<ChangeMeasure>,
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
    pub amount: Option<ChangeMeasure>,
    pub average: AverageChange,
    pub rows: i64,
    pub new: i64,
    pub gone: i64,
    pub existing: i64,
    pub missing_pairs: i64,
    pub partial_pairs: i64,
    pub import_warning_pairs: i64,
    pub invalid_reports: i64,
    pub duplicate_issues: i64,
    pub overlap_issues: i64,
}

pub(super) fn thresholds(q: &ChangeQuery) -> Result<(), String> {
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
pub(super) fn measure(base: f64, current: f64, minimum: f64, threshold: f64) -> Result<ChangeMeasure, String> {
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
pub(super) fn average_change(b: Option<f64>, a: Option<f64>, threshold: f64) -> Result<AverageChange, String> {
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
pub(super) fn status(bc: f64, ba: Option<f64>, cc: f64, ca: Option<f64>) -> String {
    let b = bc != 0. || ba.is_some_and(|n| n != 0.);
    let c = cc != 0. || ca.is_some_and(|n| n != 0.);
    if bc == 0. && ba == Some(0.) && c {
        "new"
    } else if b && cc == 0. && ca == Some(0.) {
        "gone"
    } else {
        "existing"
    }
    .into()
}
pub(super) fn fill_changes(c: &Connection, s: &AnalysisScope, q: &ChangeQuery) -> Result<(), String> {
    thresholds(q)?;
    c.execute_batch("CREATE TEMP TABLE an_currents(start TEXT PRIMARY KEY,base TEXT) WITHOUT ROWID;
 CREATE TEMP TABLE an_pairs(start TEXT,base TEXT,org TEXT,base_id TEXT,current_id TEXT,warned INTEGER,partial INTEGER DEFAULT 0,PRIMARY KEY(start,org)) WITHOUT ROWID;
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
    c.execute("INSERT INTO temp.an_pairs(start,base,org,base_id,current_id,warned) SELECT p.start,p.base,f.item,b.report_id,v.report_id,COALESCE(b.warned,0) OR COALESCE(v.warned,0) FROM temp.an_currents p CROSS JOIN analysis_filter_items f LEFT JOIN temp.an_valid b ON b.org=f.item AND b.start=p.base LEFT JOIN temp.an_valid v ON v.org=f.item AND v.start=p.start WHERE f.id=?1",[&s.organizations]).map_err(err)?;
    c.execute("INSERT INTO temp.an_period_values SELECT v.start,v.org,COALESCE(g.key,'work:'||x.work),COALESCE(g.name,x.work),CASE WHEN g.key IS NULL THEN 'work' ELSE 'group' END,SUM(x.count),CASE WHEN COUNT(x.amount)=COUNT(*) THEN SUM(x.amount) END FROM temp.an_valid v JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org LEFT JOIN analysis_groups g ON g.work=x.work WHERE EXISTS(SELECT 1 FROM temp.an_pairs p WHERE p.org=v.org AND p.base_id IS NOT NULL AND p.current_id IS NOT NULL AND (p.base=v.start OR p.start=v.start)) AND (?1 IS NULL OR EXISTS(SELECT 1 FROM analysis_filter_items f WHERE f.id=?1 AND f.item=COALESCE(g.name,x.work))) GROUP BY v.start,v.org,COALESCE(g.key,'work:'||x.work)",[&s.names]).map_err(err)?;
    finish_changes(c)
}
pub(super) fn finish_changes(c: &Connection) -> Result<(), String> {
    c.execute_batch("INSERT INTO temp.an_changes SELECT p.start,p.base,p.org,k.key,COALESCE(b.name,a.name),COALESCE(b.kind,a.kind),COALESCE(b.count,0),CASE WHEN b.key IS NULL THEN 0 ELSE b.amount END,COALESCE(a.count,0),CASE WHEN a.count IS NULL THEN 0 ELSE a.amount END FROM temp.an_pairs p JOIN (SELECT p.start,p.org,v.key FROM temp.an_pairs p JOIN temp.an_period_values v ON v.org=p.org AND (v.start=p.base OR v.start=p.start) WHERE p.base_id IS NOT NULL AND p.current_id IS NOT NULL GROUP BY p.start,p.org,v.key) k ON k.start=p.start AND k.org=p.org LEFT JOIN temp.an_period_values b ON b.start=p.base AND b.org=p.org AND b.key=k.key LEFT JOIN temp.an_period_values a ON a.start=p.start AND a.org=p.org AND a.key=k.key;").map_err(err)?;
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
pub(super) fn change_value(r: &rusqlite::Row<'_>, q: &ChangeQuery) -> Result<ChangeValue, String> {
    let bc: f64 = r.get(6).map_err(err)?;
    let ba: Option<f64> = r.get(7).map_err(err)?;
    let cc: f64 = r.get(8).map_err(err)?;
    let ca: Option<f64> = r.get(9).map_err(err)?;
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
        amount: ba.zip(ca).map(|(b,a)| measure(b,a,q.min_amount,q.percent)).transpose()?,
        average: average_change(b.average, a.average, q.percent)?,
    })
}
impl Analytics<'_> {
    pub(super) fn analysis_changes_page(
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
        let mut c = self.database.connect()?;
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
    pub(super) fn analysis_changes_summary(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
    ) -> Result<ChangesSummary, String> {
        self.with_snapshot(expected, s, |tx| {
            fill_changes(tx, s, q)?;
            changes_summary(tx, q)
        })
    }
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
        self.analysis().analysis_changes_page(expected, s, q, after, size)
    }

    pub fn analysis_changes_summary(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
    ) -> Result<ChangesSummary, String> {
        self.analysis().analysis_changes_summary(expected, s, q)
    }
}
pub(super) fn change_pair_coverage(tx: &Connection) -> Result<(i64, i64), String> {
    tx.query_row(
        "SELECT COUNT(*) FILTER (WHERE base_id IS NULL OR current_id IS NULL),
                COUNT(*) FILTER (WHERE partial<>0)
         FROM temp.an_pairs",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .map_err(err)
}

pub(super) fn changes_summary(tx: &Connection, q: &ChangeQuery) -> Result<ChangesSummary, String> {
    let (rows,new,gone):(i64,i64,i64)=tx.query_row("SELECT COUNT(*),COALESCE(SUM((bc=0 AND ba=0) AND (cc<>0 OR ca<>0)),0),COALESCE(SUM((bc<>0 OR ba<>0) AND (cc=0 AND ca=0)),0) FROM temp.an_changes",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(err)?;
    let (bc,ba,cc,ca):(f64,Option<f64>,f64,Option<f64>)=tx.query_row("SELECT COALESCE(SUM(bc),0),CASE WHEN COUNT(*)=0 THEN 0 WHEN COUNT(ba)=COUNT(*) THEN SUM(ba) END,COALESCE(SUM(cc),0),CASE WHEN COUNT(*)=0 THEN 0 WHEN COUNT(ca)=COUNT(*) THEN SUM(ca) END FROM temp.an_changes",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(err)?;
    let bt = totals(bc, ba, true)?;
    let at = totals(cc, ca, true)?;
    let (missing_pairs, partial_pairs) = change_pair_coverage(tx)?;
    Ok(ChangesSummary {
        count: measure(bc, cc, q.min_count, q.percent)?,
        amount: ba.zip(ca).map(|(b, a)| measure(b, a, q.min_amount, q.percent)).transpose()?,
        average: average_change(bt.average, at.average, q.percent)?,
        rows,
        new,
        gone,
        existing: rows - new - gone,
        missing_pairs,
        partial_pairs,
        import_warning_pairs: count_sql(tx, "SELECT COUNT(*) FROM temp.an_pairs WHERE base_id IS NOT NULL AND current_id IS NOT NULL AND warned<>0")?,
        invalid_reports: count_sql(tx, "SELECT COUNT(*) FROM temp.an_issues WHERE kind='invalid'")?,
        duplicate_issues: count_sql(tx, "SELECT COUNT(*) FROM temp.an_issues WHERE kind='duplicate'")?,
        overlap_issues: count_sql(tx, "SELECT COUNT(*) FROM temp.an_issues WHERE kind='overlap'")?,
    })
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
impl Analytics<'_> {
    pub(super) fn analysis_change_pair_page(
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
        let mut c = self.database.connect()?;
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
        self.analysis().analysis_change_pair_page(expected, s, q, problems_only, after, size)
    }
}
