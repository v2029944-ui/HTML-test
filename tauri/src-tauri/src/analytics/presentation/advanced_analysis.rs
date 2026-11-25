//! Calendar-period facts for bounded advanced analyses. The source scope
//! determines the only report type admitted by the validated workspace.
use super::analysis_presentation;
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnifiedPatternOptions {
    /// Mean of 4/8 whole intervals of the same length as the selected interval.
    #[serde(default)]
    pub range_window: bool,
    /// Exact comparison interval for frequency, including non-adjacent periods.
    pub baseline: Option<RangeQuery>,
    pub search: String,
    pub status: String,
}

/// Filtered and ordered pattern rows are still in the active read transaction.
pub(super) struct PatternExport<'a> {
    pub term: &'a str,
    pub order: &'a str,
    pub used: i64,
    pub previous: i64,
    pub current_complete: bool,
    pub current_amount_incomplete: bool,
    pub base_kind: &'a str,
    pub base_from: &'a str,
    pub base_to: &'a str,
}
type PatternWriter<'a> = dyn Fn(&Connection, PatternExport<'_>) -> Result<(), String> + 'a;

#[derive(Serialize)]
pub struct AdvancedChangesPage {
    pub rows: Vec<ChangeValue>,
    pub next: Option<ChangeCursor>,
    pub summary: ChangesSummary,
    pub missing_examples: Vec<ChangePair>,
    pub first_base: Option<String>,
    pub last_current: Option<String>,
}

fn step(kind: &str, start: NaiveDate, n: i32) -> Result<NaiveDate, String> {
    if kind == "year" {
        shift("month", start, 12 * n)
    } else {
        shift(kind, start, n)
    }
}

struct Calendar {
    first: NaiveDate,
    last: NaiveDate,
    kind: String,
    bucket: &'static str,
}

impl Calendar {
    fn new(s: &AnalysisScope, q: &ChangeQuery, grouping: Option<&str>) -> Result<Self, String> {
        thresholds(q)?;
        let r = RangeQuery {
            from: q.from.clone(),
            to: q.to.clone(),
        };
        let (first, stop) = range_bounds(&s.report_type, &r)?;
        if (stop - first).num_days() > 3660 {
            return Err("Select up to ten years".into());
        }
        let kind = grouping.unwrap_or(&s.report_type);
        if grouping.is_some() {
            timeline_grouping(s, &r, grouping, 0)?;
        }
        let (last, bucket) = match grouping {
            Some("week") => (
                stop - Duration::days(6),
                "date(start,'-'||((cast(strftime('%w',start) as integer)+6)%7)||' days')",
            ),
            Some("month") => (
                stop.with_day(1).ok_or("Invalid month")?,
                "substr(start,1,7)||'-01'",
            ),
            Some("year") => (
                stop.with_ordinal(1).ok_or("Invalid year")?,
                "substr(start,1,4)||'-01-01'",
            ),
            _ => (stop, "start"),
        };
        Ok(Self {
            first,
            last,
            kind: kind.into(),
            bucket,
        })
    }

    fn current_bounds(&self, only_latest: bool) -> (NaiveDate, NaiveDate) {
        (if only_latest { self.last } else { self.first }, self.last)
    }

    fn prepare_change_facts(
        &self,
        c: &Connection,
        s: &AnalysisScope,
        q: &ChangeQuery,
    ) -> Result<(), String> {
        let lower = if q.method == "previous" {
            step(&self.kind, self.last, -1)?
        } else {
            self.first
        };
        self.facts(c, s, Some((lower.to_string(), &q.to)))
    }

    fn fill_change_periods(&self, c: &Connection, previous: bool) -> Result<(), String> {
        let first_current = if previous {
            self.last
        } else {
            step(&self.kind, self.first, 1)?
        };
        self.for_each_bucket(first_current, |current| {
            let base = step(&self.kind, current, -1)?;
            c.execute(
                "INSERT INTO an_currents VALUES(?1,?2)",
                params![current.to_string(), base.to_string()],
            )
            .map_err(err)?;
            Ok(())
        })
    }

    fn for_each_bucket(
        &self,
        mut current: NaiveDate,
        mut visit: impl FnMut(NaiveDate) -> Result<(), String>,
    ) -> Result<(), String> {
        while current <= self.last {
            visit(current)?;
            if current == self.last {
                break;
            }
            current = step(&self.kind, current, 1)?;
        }
        Ok(())
    }

    /// Keep full calendar buckets inside an exact baseline. Edge dates
    /// outside those buckets must not contribute even when reports exist.
    fn whole_buckets(&self, from: NaiveDate, to: NaiveDate) -> Result<Option<(NaiveDate, NaiveDate)>, String> {
        let start = |d: NaiveDate| match self.kind.as_str() {
            "week" => d - Duration::days(i64::from(d.weekday().num_days_from_monday())),
            "month" => d.with_day(1).expect("Every month has a first day"),
            "year" => d.with_ordinal(1).expect("Every year has a first day"),
            _ => d,
        };
        let ends_at_boundary = match self.kind.as_str() {
            "week" => to.weekday() == chrono::Weekday::Sun,
            "month" => to.succ_opt().is_some_and(|d| d.day() == 1),
            "year" => to.month() == 12 && to.day() == 31,
            _ => true,
        };
        let mut first = start(from);
        let mut last = start(to);
        if first == last && (from != first || !ends_at_boundary) {
            return Ok(None);
        }
        if from != first {
            first = step(&self.kind, first, 1)?;
        }
        if !ends_at_boundary {
            last = step(&self.kind, last, -1)?;
        }
        Ok((first <= last).then_some((first, last)))
    }

    fn facts(
        &self,
        c: &Connection,
        s: &AnalysisScope,
        range: Option<(String, &str)>,
    ) -> Result<(), String> {
        let bucket = self.bucket;
        c.execute(&format!("CREATE TEMP TABLE adv_reports AS
            SELECT org,start,{bucket} label,report_id,warned FROM temp.an_valid WHERE (?1 IS NULL OR start>=?1) AND (?2 IS NULL OR start<=?2)"),
            params![range.as_ref().map(|r|&r.0),range.as_ref().map(|r|r.1)]).map_err(err)?;
        c.execute_batch("CREATE INDEX temp.adv_label_org ON adv_reports(label,org);
            CREATE TEMP TABLE adv_coverage(label TEXT,org TEXT,warned INTEGER,PRIMARY KEY(label,org)) WITHOUT ROWID;").map_err(err)?;
        let expected = match self.kind.as_str() {
            "week" if s.report_type == "day" => "7",
            "month" if s.report_type == "day" => "julianday(label,'+1 month')-julianday(label)",
            "year" if s.report_type == "day" => "julianday(label,'+1 year')-julianday(label)",
            _ => "1",
        };
        c.execute_batch(&format!("INSERT INTO adv_coverage SELECT label,org,max(warned) FROM adv_reports GROUP BY label,org HAVING count(*)={expected};
            CREATE TEMP TABLE adv_facts(label TEXT,org TEXT,key TEXT,name TEXT,kind TEXT,count REAL,amount REAL,known_amount REAL,active INTEGER,PRIMARY KEY(label,org,key)) WITHOUT ROWID;")).map_err(err)?;
        c.execute("INSERT INTO adv_facts SELECT v.label,v.org,coalesce(g.key,'work:'||x.work),coalesce(g.name,x.work),
            CASE WHEN g.key IS NULL THEN 'work' ELSE 'group' END,sum(x.count),CASE WHEN COUNT(x.amount)=COUNT(*) THEN SUM(x.amount) END,SUM(x.amount),max(x.count<>0 OR x.amount IS NULL OR x.amount<>0)
            FROM adv_reports v JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org LEFT JOIN analysis_groups g ON g.work=x.work
            WHERE (?1 IS NULL OR EXISTS(SELECT 1 FROM analysis_filter_items f WHERE f.id=?1 AND f.item=coalesce(g.name,x.work)))
            GROUP BY v.label,v.org,coalesce(g.key,'work:'||x.work)",[&s.names]).map_err(err)?;
        validate_numbers(c, "SELECT count,known_amount FROM adv_facts")
    }
}

pub(super) fn prepare_changes(
    c: &Connection,
    s: &AnalysisScope,
    q: &ChangeQuery,
    grouping: Option<&str>,
) -> Result<(), String> {
    let calendar = Calendar::new(s, q, grouping)?;
    if grouping.is_none() || grouping == Some("day") {
        return fill_changes(c, s, q);
    }
    calendar.prepare_change_facts(c, s, q)?;
    c.execute_batch("CREATE TEMP TABLE an_currents(start TEXT PRIMARY KEY,base TEXT) WITHOUT ROWID;
        CREATE TEMP TABLE an_pairs(start TEXT,base TEXT,org TEXT,base_id TEXT,current_id TEXT,warned INTEGER,PRIMARY KEY(start,org)) WITHOUT ROWID;
        CREATE TEMP TABLE an_period_values(start TEXT,org TEXT,key TEXT,name TEXT,kind TEXT,count REAL,amount REAL,PRIMARY KEY(start,org,key)) WITHOUT ROWID;
        CREATE TEMP TABLE an_changes(start TEXT,base TEXT,org TEXT,key TEXT,name TEXT,kind TEXT,bc REAL,ba REAL,cc REAL,ca REAL,PRIMARY KEY(start,org,key)) WITHOUT ROWID;").map_err(err)?;
    calendar.fill_change_periods(c, q.method == "previous")?;
    c.execute("INSERT INTO an_pairs SELECT p.start,p.base,f.item,b.label,v.label,coalesce(b.warned,0) OR coalesce(v.warned,0)
        FROM an_currents p CROSS JOIN analysis_filter_items f LEFT JOIN adv_coverage b ON b.label=p.base AND b.org=f.item
        LEFT JOIN adv_coverage v ON v.label=p.start AND v.org=f.item WHERE f.id=?1",[&s.organizations]).map_err(err)?;
    c.execute_batch("INSERT INTO an_period_values SELECT label,org,key,name,kind,count,amount FROM adv_facts WHERE label IN (SELECT start FROM an_currents UNION SELECT base FROM an_currents)").map_err(err)?;
    finish_changes(c)
}

impl Analytics<'_> {
    pub(super) fn advanced_changes_page(
        &self,
        revision: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        grouping: Option<&str>,
        after: Option<&ChangeCursor>,
    ) -> Result<AdvancedChangesPage, String> {
        let mut conn = self.database.connect()?;
        let tx = conn.transaction().map_err(err)?;
        workspace(&tx, revision, s)?;
        prepare_changes(&tx, s, q, grouping)?;
        let summary = changes_summary(&tx, q)?;
        let mut stmt = tx
            .prepare(
                "SELECT start,base,org,key,name,kind,bc,ba,cc,ca FROM an_changes
            WHERE ?1 IS NULL OR (start,org,key)>(?1,?2,?3) ORDER BY start,org,key LIMIT 51",
            )
            .map_err(err)?;
        if let Some(a) = after {
            bounded(&a.org)?;
            key_bound(&a.key)?;
            date(&a.current_start)?;
        }
        let mut result = stmt
            .query(params![
                after.map(|a| &a.current_start),
                after.map(|a| &a.org),
                after.map(|a| &a.key)
            ])
            .map_err(err)?;
        let mut rows = Vec::new();
        while let Some(r) = result.next().map_err(err)? {
            rows.push(change_value(r, q)?);
        }
        let next = if rows.len() > 50 {
            rows.pop();
            rows.last().map(ChangeValue::cursor)
        } else {
            None
        };
        let (first_base, last_current) = tx
            .query_row("SELECT min(base),max(start) FROM an_currents", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(err)?;
        let mut pairs = tx
            .prepare(
                "SELECT start,base,org,base_id IS NULL,current_id IS NULL,warned<>0
            FROM an_pairs WHERE base_id IS NULL OR current_id IS NULL ORDER BY start,org LIMIT 5",
            )
            .map_err(err)?;
        let missing_examples = pairs
            .query_map([], |r| {
                Ok(ChangePair {
                    current_start: r.get(0)?,
                    base_start: r.get(1)?,
                    org: r.get(2)?,
                    missing_base: r.get(3)?,
                    missing_current: r.get(4)?,
                    import_warning: r.get(5)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(AdvancedChangesPage {
            missing_examples,
            rows,
            next,
            summary,
            first_base,
            last_current,
        })
    }

    pub(super) fn advanced_changes_csv(
        &self,
        revision: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        grouping: Option<&str>,
        path: &Path,
    ) -> Result<(), String> {
        let mut conn = self.database.connect()?;
        let tx = conn.transaction().map_err(err)?;
        workspace(&tx, revision, s)?;
        prepare_changes(&tx, s, q, grouping)?;
        let coverage = changes_summary(&tx, q)?;
        export(path, |w| {
            use Cell::{Number, Text};
            let source = if grouping.is_some() {
                "daily"
            } else {
                "imported"
            };
            for (label, value) in [
                ("source", source),
                ("displayPeriod", grouping.unwrap_or(&s.report_type)),
                ("reportType", s.report_type.as_str()),
                ("from", q.from.as_str()),
                ("to", q.to.as_str()),
                ("method", q.method.as_str()),
                (
                    "nameSelection",
                    if s.names.is_some() { "selected" } else { "all" },
                ),
                (
                    "countUnit",
                    "Количество / объём в единицах исходных отчётов",
                ),
                ("amountUnit", "Сумма в единицах исходных отчётов"),
                ("averageUnit", "Сумма / единица количества"),
            ] {
                csv_record(w, &[Text(label), Text(value)])?;
            }
            for (label, value) in [
                ("revision", revision),
                ("comparableRows", coverage.rows),
                ("missingPairs", coverage.missing_pairs),
                ("importWarningPairs", coverage.import_warning_pairs),
                ("invalidReports", coverage.invalid_reports),
                ("duplicateIssues", coverage.duplicate_issues),
                ("overlapIssues", coverage.overlap_issues),
            ] {
                csv_record(w, &[Text(label), Text(&value.to_string())])?;
            }
            for (label, value) in [
                ("percentThreshold", q.percent),
                ("minCount", q.min_count),
                ("minAmount", q.min_amount),
            ] {
                csv_record(w, &[Text(label), Number(Some(value))])?;
            }
            for (label, id) in [
                ("organization", Some(&s.organizations)),
                ("selectedName", s.names.as_ref()),
            ] {
                let mut stmt = tx
                    .prepare("SELECT item FROM analysis_filter_items WHERE id=?1 ORDER BY ordinal")
                    .map_err(err)?;
                let mut items = stmt.query([id]).map_err(err)?;
                while let Some(row) = items.next().map_err(err)? {
                    let value: String = row.get(0).map_err(err)?;
                    csv_record(w, &[Text(label), Text(&value)])?;
                }
            }
            let (base, last): (Option<String>, Option<String>) = tx
                .query_row("SELECT min(base),max(start) FROM an_currents", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .map_err(err)?;
            for (label, value) in [("firstBase", base), ("lastCurrent", last)] {
                csv_record(w, &[Text(label), Text(value.as_deref().unwrap_or(""))])?;
            }
            write_changes_rows(&tx, q, w)
        })
    }
}

impl Storage {
    pub fn advanced_changes_page(
        &self,
        revision: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        grouping: Option<&str>,
        after: Option<&ChangeCursor>,
    ) -> Result<AdvancedChangesPage, String> {
        self.analysis().advanced_changes_page(revision, s, q, grouping, after)
    }

    pub fn advanced_changes_csv(
        &self,
        revision: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        grouping: Option<&str>,
        path: &Path,
    ) -> Result<(), String> {
        self.analysis().advanced_changes_csv(revision, s, q, grouping, path)
    }
}

#[derive(Serialize)]
pub struct PatternPage {
    pub total_rows: i64,
    pub rows: Vec<Value>,
    pub next: Option<String>,
    pub used: i64,
    pub expected: i64,
    pub previous_used: i64,
    pub baseline_from: String,
    pub baseline_to: String,
    pub current_from: String,
    pub current_to: String,
    pub current_complete: bool,
    pub current_amount_incomplete: bool,
}

fn usual_metric(
    base: Option<f64>,
    current: Option<f64>,
    observed_current: Option<f64>,
    used: i64,
    percent: f64,
) -> Result<Value, String> {
    let change = average_change(base, current, percent)?;
    let status = match change.delta {
        Some(d) if used >= 2 => {
            if d != 0. && change.percent.is_none_or(|p| p.abs() >= percent) {
                if d > 0. {
                    "high"
                } else {
                    "low"
                }
            } else {
                "normal"
            }
        }
        _ => "insufficient",
    };
    Ok(
        json!({"base":base,"current":observed_current,"delta":change.delta,"percent":change.percent,"status":status}),
    )
}

fn regularity_row(
    tx: &Connection,
    key: &str,
    from: &str,
    to: &str,
    used: i64,
    expected: i64,
    previous: i64,
    gap: i64,
) -> Result<Value, String> {
    let (first,last,hits,old_hits,last_index):(Option<String>,Option<String>,i64,i64,Option<i64>)=tx.query_row(
        "SELECT min(f.label) FILTER(WHERE f.label<=?2),max(f.label) FILTER(WHERE f.label<=?2),count(CASE WHEN d.slot=1 AND d.complete THEN 1 END),
            count(CASE WHEN d.slot=0 AND d.complete THEN 1 END),min(t.distance)
         FROM adv_combined f LEFT JOIN adv_dates d ON d.label=f.label LEFT JOIN adv_tail t ON t.label=f.label
         WHERE f.key=?1 AND f.active AND (f.label<=?2 OR d.slot=0)",params![key,to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(err)?;
    let trailing = if last.is_some() {
        Some(match last_index {
            Some(n) => n,
            None => count_sql(tx, "SELECT count(*) FROM adv_tail")?,
        })
    } else {
        None
    };
    let frequency = (used > 0).then(|| hits as f64 / used as f64 * 100.);
    let old_frequency = (previous > 0).then(|| old_hits as f64 / previous as f64 * 100.);
    let mut statuses = Vec::new();
    if used >= 2 && frequency >= Some(75.) {
        statuses.push("frequent");
    }
    if used >= 4 && hits > 0 && frequency <= Some(25.) {
        statuses.push("rare");
    }
    if used >= 2
        && previous >= 2
        && old_hits >= 2
        && hits > 0
        && frequency <= old_frequency.map(|x| x / 2.)
    {
        statuses.push("rarer");
    }
    if hits == 0 && used == expected && old_hits > 0 {
        statuses.push("disappeared");
    }
    if hits > 0 && first.as_deref().is_some_and(|d| d >= from) {
        statuses.push("new");
    }
    if trailing.is_some_and(|n| n >= gap) {
        statuses.push("dormant");
    }
    if statuses.is_empty() {
        statuses.push(if used == 0 {
            "unknown"
        } else if hits > 0 {
            "occasional"
        } else if used < expected {
            "uncertain"
        } else {
            "absent"
        });
    }
    Ok(
        json!({"first":first,"last":last,"hits":hits,"previousHits":old_hits,"frequency":frequency,"previousFrequency":old_frequency,"trailing":trailing,"statuses":statuses}),
    )
}

impl Analytics<'_> {
    pub(super) fn advanced_patterns(
        &self,
        revision: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        grouping: Option<&str>,
        mode: &str,
        window: i32,
        after: Option<&str>,
    ) -> Result<PatternPage, String> {
        self.patterns_filtered(revision,s,q,grouping,mode,window,after,None,None,None)
    }

    pub(super) fn unified_pattern_page(
        &self, revision:i64, s:&AnalysisScope, q:&ChangeQuery, grouping:Option<&str>,
        mode:&str, window:i32, options:&UnifiedPatternOptions, after:Option<&str>,
    )->Result<PatternPage,String> {
        validate_pattern_options(mode, options)?;
        self.patterns_filtered(revision, s, q, grouping, mode, window, after, Some(options), None, None)
    }

    pub(super) fn export_pattern_rows(
        &self, revision: i64, s: &AnalysisScope, q: &ChangeQuery, grouping: Option<&str>,
        mode: &str, window: i32, options: &UnifiedPatternOptions, write: &PatternWriter<'_>,
    ) -> Result<(), String> {
        validate_pattern_options(mode, options)?;
        self.patterns_filtered(revision, s, q, grouping, mode, window, None, Some(options), Some(write), None)?;
        Ok(())
    }

    pub(super) fn unified_pattern_page_at_page(&self,revision:i64,s:&AnalysisScope,q:&ChangeQuery,grouping:Option<&str>,mode:&str,window:i32,options:&UnifiedPatternOptions,page_index:usize)->Result<PatternPage,String> {
        validate_pattern_options(mode,options)?;
        self.patterns_filtered(revision,s,q,grouping,mode,window,None,Some(options),None,Some(page_index))
    }
    fn patterns_filtered(
        &self, revision:i64, s:&AnalysisScope, q:&ChangeQuery, grouping:Option<&str>,
        mode:&str, window:i32, after:Option<&str>, options:Option<&UnifiedPatternOptions>, write:Option<&PatternWriter<'_>>, page_index:Option<usize>,
    )->Result<PatternPage,String> {
        if !matches!(mode, "usual" | "regularity") || !matches!(window, 4 | 8) {
            return Err("Invalid pattern mode or window".into());
        }
        if let Some(key) = after.filter(|_|options.is_none()) {
            key_bound(key)?;
        }
        let calendar = Calendar::new(s, q, grouping)?;
        let mut conn = self.database.connect()?;
        let tx = conn.transaction().map_err(err)?;
        workspace(&tx, revision, s)?;
        let fingerprint=if let Some(options)=options {
            analysis_presentation::presentation_functions(&tx)?;
            Some(analysis_presentation::fingerprint(&tx,revision,s,json!([q,grouping,mode,window,options]))?)
        }else{None};
        let cursor_offset=if let Some(fingerprint)=fingerprint.as_deref() {
            analysis_presentation::page_offset(after,fingerprint)?
        }else{0};
        calendar.facts(&tx, s, None)?;
        tx.execute_batch("CREATE TEMP TABLE adv_complete(label TEXT PRIMARY KEY) WITHOUT ROWID;
          CREATE TEMP TABLE adv_dates(label TEXT,slot INTEGER,batch INTEGER,complete INTEGER,PRIMARY KEY(label,slot)) WITHOUT ROWID;
          CREATE TEMP TABLE adv_tail(label TEXT PRIMARY KEY,distance INTEGER) WITHOUT ROWID;
          CREATE TEMP TABLE adv_combined(key TEXT,label TEXT,count REAL,amount REAL,known_amount REAL,active INTEGER,PRIMARY KEY(key,label)) WITHOUT ROWID;
          INSERT INTO adv_combined SELECT key,label,sum(count),CASE WHEN COUNT(amount)=COUNT(*) THEN SUM(amount) END,SUM(known_amount),max(active) FROM adv_facts GROUP BY key,label;
          CREATE TEMP TABLE adv_keys(key TEXT PRIMARY KEY,name TEXT,kind TEXT) WITHOUT ROWID;
          INSERT INTO adv_keys SELECT key,name,kind FROM adv_facts GROUP BY key;").map_err(err)?;
        tx.execute("INSERT INTO adv_complete SELECT label FROM adv_coverage GROUP BY label HAVING count(*)=(SELECT count(*) FROM analysis_filter_items WHERE id=?1)",[&s.organizations]).map_err(err)?;
        tx.execute("INSERT INTO adv_keys SELECT coalesce(g.key,'work:'||f.item),f.item,CASE WHEN g.key IS NULL THEN 'work' ELSE 'group' END FROM analysis_filter_items f
            LEFT JOIN analysis_group_names g ON g.name=f.item WHERE f.id=?1 AND NOT EXISTS(SELECT 1 FROM adv_keys k WHERE k.name=f.item)",[&s.names]).map_err(err)?;
        let usual = mode == "usual";
        let range_window = options.is_some_and(|o| o.range_window);
        let (first, last) = calendar.current_bounds(usual && !range_window);
        let mut count = 0;
        calendar.for_each_bucket(first, |d| {
            tx.execute("INSERT INTO adv_dates VALUES(?1,1,0,EXISTS(SELECT 1 FROM adv_complete WHERE label=?1))",[d.to_string()]).map_err(err)?;
            count += 1;
            Ok(())
        })?;
        let sample_length = if range_window { count } else { 1 };
        let (baseline_from, baseline_to, baseline_buckets) = if let Some(base) = options.and_then(|o| o.baseline.as_ref()) {
            let selected = ChangeQuery { from: base.from.clone(), to: base.to.clone(), ..q.clone() };
            let baseline = Calendar::new(s, &selected, None)?;
            let bounds = baseline.current_bounds(false);
            let buckets = if grouping.is_some() {
                calendar.whole_buckets(bounds.0, bounds.1)?
            } else {
                Some(bounds)
            };
            (bounds.0, bounds.1, buckets)
        } else {
            let base_count = if usual { window * sample_length } else { count };
            let bounds = (step(&calendar.kind, first, -base_count)?, step(&calendar.kind, first, -1)?);
            (bounds.0, bounds.1, Some(bounds))
        };
        if let Some((mut day, last)) = baseline_buckets {
            let mut index = 0;
            loop {
                tx.execute("INSERT INTO adv_dates VALUES(?1,0,?2,EXISTS(SELECT 1 FROM adv_complete WHERE label=?1))",
                    params![day.to_string(), index / sample_length]).map_err(err)?;
                if day == last { break; }
                day = step(&calendar.kind, day, 1)?;
                index += 1;
            }
        }
        // A missing day excludes its entire historical sample from the mean.
        // Other samples retain their original values, including observed zeros.
        if range_window {
            tx.execute_batch("UPDATE adv_dates SET complete=0 WHERE slot=0 AND batch IN
                (SELECT batch FROM adv_dates WHERE slot=0 GROUP BY batch HAVING min(complete)=0)").map_err(err)?;
        }
        let previous = count_sql(&tx,
            "SELECT count(DISTINCT batch) FROM adv_dates WHERE slot=0 AND complete")?;
        let used = count_sql(
            &tx,
            "SELECT count(*) FROM adv_dates WHERE slot=1 AND complete",
        )?;
        let current_complete = used == i64::from(count);
        let (current_observed, current_amount_incomplete) = if usual && options.is_some() {
            tx.query_row("SELECT
                EXISTS(SELECT 1 FROM adv_dates d JOIN adv_reports r USING(label) WHERE d.slot=1),
                EXISTS(SELECT 1 FROM adv_dates d JOIN adv_facts f USING(label) WHERE d.slot=1 AND f.amount IS NULL)",
                [], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?
        } else {
            (false, false)
        };
        if usual {
            tx.execute_batch("INSERT INTO adv_keys VALUES('','Итого по выбранным работам','total');
                INSERT INTO adv_combined SELECT '',label,sum(count),CASE WHEN COUNT(amount)=COUNT(*) THEN SUM(amount) END,SUM(known_amount),max(active) FROM adv_combined GROUP BY label;").map_err(err)?;
        } else {
            let mut d = last;
            let mut distance = 0;
            while tx
                .execute(
                    "INSERT INTO adv_tail SELECT label,?2 FROM adv_complete WHERE label=?1",
                    params![d.to_string(), distance],
                )
                .map_err(err)?
                != 0
            {
                distance += 1;
                let Ok(prev) = step(&calendar.kind, d, -1) else {
                    break;
                };
                d = prev;
            }
        }
        // The filtered path streams one computed row into a temporary SQL
        // table at a time. Filtering/sorting/summary/LIMIT then share one view;
        // neither historical reports nor all computed rows enter a JS array.
        if options.is_some() {tx.execute_batch("CREATE TEMP TABLE unified_patterns(key TEXT PRIMARY KEY,name TEXT,data TEXT) WITHOUT ROWID").map_err(err)?;}
        let sql=if options.is_some() {"SELECT key,name,kind FROM adv_keys WHERE ?1 IS NULL ORDER BY key"}else{"SELECT key,name,kind FROM adv_keys WHERE ?1 IS NULL OR key>?1 ORDER BY key LIMIT 51"};
        let mut stmt=tx.prepare(sql).map_err(err)?;
        let mut iterator=stmt.query([if options.is_some(){None}else{after}]).map_err(err)?;
        let mut keys=Vec::new();
        if options.is_none() {
          while let Some(r)=iterator.next().map_err(err)? {keys.push((r.get::<_,String>(0).map_err(err)?,r.get::<_,String>(1).map_err(err)?,r.get::<_,String>(2).map_err(err)?));}
        }
        let next = if keys.len() > 50 {
            keys.pop();
            keys.last().map(|k| k.0.clone())
        } else {
            None
        };
        let mut rows = Vec::new();
        let mut legacy_keys=keys.into_iter();
        loop {
            let next_key=if options.is_some() {
                iterator.next().map_err(err)?.map(|r|Ok::<_,String>((r.get::<_,String>(0).map_err(err)?,r.get::<_,String>(1).map_err(err)?,r.get::<_,String>(2).map_err(err)?))).transpose()?
            }else{legacy_keys.next()};
            let Some((key,name,kind))=next_key else {break};
            let mut row = if usual {
                let (bc,ba,ac,aa,known_amount):(f64,Option<f64>,f64,Option<f64>,Option<f64>)=tx.query_row("SELECT
                    coalesce(sum(count) FILTER(WHERE slot=0),0),CASE WHEN COUNT(*) FILTER(WHERE slot=0)=0 THEN 0 WHEN COUNT(amount) FILTER(WHERE slot=0)=COUNT(*) FILTER(WHERE slot=0) THEN SUM(amount) FILTER(WHERE slot=0) END,
                    coalesce(sum(count) FILTER(WHERE slot=1),0),CASE WHEN COUNT(*) FILTER(WHERE slot=1)=0 THEN 0 WHEN COUNT(amount) FILTER(WHERE slot=1)=COUNT(*) FILTER(WHERE slot=1) THEN SUM(amount) FILTER(WHERE slot=1) END,
                    SUM(known_amount) FILTER(WHERE slot=1)
                    FROM adv_combined JOIN adv_dates USING(label) WHERE key=?1 AND (slot=1 OR complete)",[&key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(err)?;
                let base = totals(bc, ba, previous > 0)?;
                let now = totals(ac, aa, current_complete)?;
                let shown = options
                    .map(|_| observed_totals(ac, aa, known_amount, current_observed))
                    .transpose()?;
                let shown = shown.as_ref().unwrap_or(&now);
                json!({"count":usual_metric(base.count.map(|v|v/previous as f64),now.count,shown.count,previous,q.percent)?,
                    "amount":usual_metric(base.amount.map(|v|v/previous as f64),now.amount,shown.amount,previous,q.percent)?,
                    "average":usual_metric(base.average,now.average,shown.average,previous,q.percent)?})
            } else {
                regularity_row(
                    &tx,
                    &key,
                    &first.to_string(),
                    &last.to_string(),
                    used,
                    i64::from(count),
                    previous,
                    i64::from(window),
                )?
            };
            row["key"] = if key.is_empty() {
                Value::Null
            } else {
                json!(key)
            };
            row["name"] = json!(name);
            row["kind"] = json!(kind);
            if options.is_some() {
                tx.execute("INSERT INTO unified_patterns VALUES(?1,?2,?3)",params![key,name,serde_json::to_string(&row).map_err(err)?]).map_err(err)?;
            }else {rows.push(row);}
        }
        let mut total_rows=count_sql(&tx,"SELECT count(*) FROM adv_keys")?;
        let mut next=next;
        if let Some(options)=options {
            let term=analysis_presentation::normalized(&options.search);
            tx.execute("DELETE FROM unified_patterns WHERE instr(classic_normalized(name),?1)=0 OR (?2<>'all' AND NOT EXISTS(SELECT 1 FROM json_each(json_extract(data,'$.statuses')) WHERE value=?2))",params![term,options.status]).map_err(err)?;
            total_rows=count_sql(&tx,"SELECT count(*) FROM unified_patterns")?;
            let offset=match page_index {Some(index)=>crate::storage::direct_page_offset(index,50,total_rows)?,None=>cursor_offset};
            let order=if term.is_empty(){"key"}else{"(instr(classic_normalized(name),?1)<>1),name COLLATE classic_ru,key"};
            if let Some(write) = write {
                write(&tx, PatternExport {
                    term: &term,
                    order,
                    used,
                    previous,
                    current_complete,
                    current_amount_incomplete,
                    base_kind: &calendar.kind,
                    base_from: &baseline_from.to_string(),
                    base_to: &baseline_to.to_string(),
                })?;
            } else {
            let mut query=tx.prepare(&format!("SELECT data FROM unified_patterns WHERE ?1 IS NOT NULL ORDER BY {order} LIMIT 51 OFFSET ?2")).map_err(err)?;
            let mut result=query.query(params![term,offset]).map_err(err)?;
            while let Some(r)=result.next().map_err(err)? {rows.push(serde_json::from_str(&r.get::<_,String>(0).map_err(err)?).map_err(err)?);}
            next=if rows.len()>50 {rows.pop();Some(analysis_presentation::next_cursor(fingerprint.as_deref().unwrap(),offset+50)?)}else{None};
            }
        }
        Ok(PatternPage {
            total_rows,
            rows,
            next,
            used: if usual { previous } else { used },
            expected: i64::from(if usual { window } else { count }),
            previous_used: previous,
            baseline_from: baseline_from.to_string(),
            baseline_to: baseline_to.to_string(),
            current_from: first.to_string(),
            current_to: last.to_string(),
            current_complete,
            current_amount_incomplete,
        })
    }
}

impl Storage {
    pub fn advanced_patterns(
        &self,
        revision: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        grouping: Option<&str>,
        mode: &str,
        window: i32,
        after: Option<&str>,
    ) -> Result<PatternPage, String> {
        self.analysis().advanced_patterns(revision, s, q, grouping, mode, window, after)
    }

    pub fn unified_pattern_page(
        &self, revision:i64, s:&AnalysisScope, q:&ChangeQuery, grouping:Option<&str>,
        mode:&str, window:i32, options:&UnifiedPatternOptions, after:Option<&str>,
    )->Result<PatternPage,String> {
        self.analysis().unified_pattern_page(revision, s, q, grouping, mode, window, options, after)
    }

    pub fn unified_pattern_page_at_page(&self,revision:i64,s:&AnalysisScope,q:&ChangeQuery,grouping:Option<&str>,mode:&str,window:i32,options:&UnifiedPatternOptions,page_index:usize)->Result<PatternPage,String> {
        self.analysis().unified_pattern_page_at_page(revision, s, q, grouping, mode, window, options, page_index)
    }
}

#[derive(Deserialize)]
pub struct ContributionCursor {
    magnitude: f64,
    key: String,
}

fn percent_of(value: f64, total: f64, valid: bool) -> Result<Option<f64>, String> {
    if valid && total != 0. {
        Ok(Some(finite(value / total * 100.)?))
    } else {
        Ok(None)
    }
}

pub(super) fn observed_contribution_rows(c: &Connection, metric: &str) -> Result<String, String> {
    let (before, after) = match metric {
        "count" => ("bc", "cc"),
        "amount" => ("coalesce(ba,b_known)", "coalesce(ca,a_known)"),
        _ => return Err("Select amount or count".into()),
    };
    let before = if observed_reports(c, 1, None)? { before } else { "NULL" };
    let after = if observed_reports(c, 2, None)? { after } else { "NULL" };
    Ok(format!("SELECT key,name,kind,{before} AS base,{after} AS current FROM an_comparison
        WHERE key NOT LIKE 'absent:%' AND ({before} IS NOT NULL OR {after} IS NOT NULL)"))
}

impl Analytics<'_> {
    pub(super) fn advanced_composition(
        &self,
        revision: i64,
        s: &AnalysisScope,
        selection: &ChangeQuery,
        grouping: Option<&str>,
        before: &RangeQuery,
        mode: &str,
        metric: &str,
        after: Option<&ContributionCursor>,
    ) -> Result<Value, String> {
        self.advanced_composition_limit(revision,s,selection,grouping,before,mode,metric,after,12)
    }

    pub(super) fn advanced_composition_limit(
        &self, revision:i64, s:&AnalysisScope, selection:&ChangeQuery, grouping:Option<&str>,
        before:&RangeQuery, mode:&str, metric:&str, after:Option<&ContributionCursor>, structure_limit:usize,
    )->Result<Value,String> {
        self.composition_window(revision,s,selection,grouping,before,mode,metric,after,structure_limit,None)
    }
    pub(super) fn advanced_composition_at_page(&self, revision:i64, s:&AnalysisScope, selection:&ChangeQuery, grouping:Option<&str>, before:&RangeQuery,mode:&str,metric:&str,structure_limit:usize,page_index:usize)->Result<Value,String> {
        if mode!="contributions" {return Err("Direct pages require contribution rows".into());}
        self.composition_window(revision,s,selection,grouping,before,mode,metric,None,structure_limit,Some(page_index))
    }
    fn composition_window(
        &self, revision:i64, s:&AnalysisScope, selection:&ChangeQuery, grouping:Option<&str>,
        before:&RangeQuery, mode:&str, metric:&str, after:Option<&ContributionCursor>, structure_limit:usize,page_index:Option<usize>,
    )->Result<Value,String> {
        if !matches!(structure_limit,10|12|20) {return Err("Structure limit must be 10 or 20".into());}
        let mut base = selection.clone();
        base.from = before.from.clone();
        base.to = before.to.clone();
        Calendar::new(s, &base, grouping)?;
        Calendar::new(s, selection, grouping)?;
        if !matches!(mode, "structure" | "contributions") {
            return Err("Unknown composition analysis".into());
        }
        let (b, a) = match metric {
            "count" => ("bc", "cc"),
            "amount" => ("ba", "ca"),
            _ => return Err("Select amount or count".into()),
        };
        let q = ComparisonQuery {
            before: before.clone(),
            after: RangeQuery {
                from: selection.from.clone(),
                to: selection.to.clone(),
            },
            min_amount: 0.,
            min_count: 0.,
            percent: 0.,
        };
        let mut conn = self.database.connect()?;
        let tx = conn.transaction().map_err(err)?;
        workspace(&tx, revision, s)?;
        let (base_complete, mut current_complete) = comparison(&tx, s, &q, None)?;
        let mut result = json!({"baseComplete":base_complete,"currentComplete":current_complete,"total_rows":0,"rows":[],"next":null});
        if mode == "contributions" && page_index.is_none() && !(current_complete && base_complete) {
            return Ok(result);
        }
        if mode == "structure" && !observed_reports(&tx,2,None)? {
            return Ok(result);
        }
        // Explicitly selected names absent in both ranges are added by the
        // generic comparison screen. Structure/contributions use observed keys.
        tx.execute_batch("DELETE FROM an_comparison WHERE key LIKE 'absent:%'")
            .map_err(err)?;
        let (bt, at): (Option<f64>, Option<f64>) = tx
            .query_row(
                &format!("SELECT CASE WHEN COUNT(*)=0 THEN 0 WHEN COUNT({b})=COUNT(*) THEN SUM({b}) END,CASE WHEN COUNT(*)=0 THEN 0 WHEN COUNT({a})=COUNT(*) THEN SUM({a}) END FROM an_comparison"),
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(err)?;
        current_complete &= at.is_some();
        result["currentComplete"] = json!(current_complete);
        let base_complete = base_complete && bt.is_some();
        result["baseComplete"] = json!(base_complete);
        if mode == "contributions" && !(base_complete && current_complete) {
            if let Some(index) = page_index {
                let source = observed_contribution_rows(&tx, metric)?;
                let total = count_sql(&tx, &format!("SELECT count(*) FROM ({source})"))?;
                let offset = crate::storage::direct_page_offset(index, 50, total)?;
                let mut stmt = tx.prepare(&format!("{source} ORDER BY key LIMIT 50 OFFSET ?1")).map_err(err)?;
                let mut data = stmt.query([offset]).map_err(err)?;
                let mut rows = Vec::new();
                while let Some(row) = data.next().map_err(err)? {
                    let base = row.get::<_,Option<f64>>(3).map_err(err)?.map(finite).transpose()?;
                    let current = row.get::<_,Option<f64>>(4).map_err(err)?.map(finite).transpose()?;
                    rows.push(json!({"key":row.get::<_,String>(0).map_err(err)?,
                        "name":row.get::<_,String>(1).map_err(err)?,"kind":row.get::<_,String>(2).map_err(err)?,
                        "base":base,"current":current,"delta":null,"percent":null,"contribution":null}));
                }
                result["rows"] = json!(rows);
                result["total_rows"] = json!(total);
            }
            return Ok(result);
        }
        // The structure can list observed values even where the full
        // denominator is unknown. Such rows never receive shares.
        let a = if mode == "structure" && metric == "amount" { "coalesce(ca,a_known)" } else { a };
        let current_total = if mode == "structure" && at.is_none() {
            range_summary(&tx,s,2)?.available_total.amount
        } else { at };
        let Some(at) = current_total else { return Ok(result); };
        if mode == "structure" {
            result["currentAvailable"] = json!(true);
        }
        let bt = bt.map(finite).transpose()?;
        finite(at)?;
        let mut rows = Vec::new();
        if mode == "structure" {
            let (bv,av):(bool,bool)=tx.query_row(&format!("SELECT NOT EXISTS(SELECT 1 FROM an_comparison WHERE {b}<0),NOT EXISTS(SELECT 1 FROM an_comparison WHERE {a}<0)"),[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(err)?;
            let valid = current_complete && av && at > 0.;
            let base_valid = bv && bt.is_some_and(|n| n > 0.) && base_complete;
            let locale: icu_provider::DataLocale = "ru".parse().map_err(err)?;
            let collator =
                icu_collator::Collator::try_new(&locale, Default::default()).map_err(err)?;
            tx.create_collation("classic_ru", move |a, b| collator.compare(a, b))
                .map_err(err)?;
            // The classic stable sort retains the first encounter when names
            // compare equal (including canonically equivalent spellings).
            // Current range precedes baseline; then selected org, period and row.
            tx.execute(
                "CREATE TEMP TABLE adv_encounter AS SELECT c.key,
                min(printf('%d/%020d/%s/%020d',2-p.slot,f.ordinal,p.start,x.row_index)) AS encounter
                FROM an_periods p JOIN an_valid v ON v.start=p.start
                JOIN analysis_filter_items f ON f.id=?1 AND f.item=v.org
                JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org
                LEFT JOIN analysis_groups g ON g.work=x.work
                JOIN an_comparison c ON c.key=coalesce(g.key,'work:'||x.work) GROUP BY c.key",
                [&s.organizations],
            )
            .map_err(err)?;
            let mut stmt=tx.prepare(&format!("WITH ranked AS (SELECT key,name,kind,{b} base,{a} current,row_number() OVER(ORDER BY abs({a}) DESC,name COLLATE classic_ru,encounter) n FROM an_comparison JOIN adv_encounter USING(key))
                SELECT key,name,kind,base,current,n,0 FROM ranked WHERE n<={structure_limit} UNION ALL
                SELECT NULL,'Остальные услуги ('||count(*)||')',NULL,CASE WHEN COUNT(base)=COUNT(*) THEN SUM(base) END,sum(current),{others},1 FROM ranked WHERE n>{structure_limit} HAVING count(*)>0 ORDER BY 6",others=structure_limit+1)).map_err(err)?;
            let mut data = stmt.query([]).map_err(err)?;
            let mut cumulative = 0.;
            while let Some(r) = data.next().map_err(err)? {
                let key: Option<String> = r.get(0).map_err(err)?;
                let name: String = r.get(1).map_err(err)?;
                let kind: Option<String> = r.get(2).map_err(err)?;
                let base: Option<f64> = r.get(3).map_err(err)?;
                let current: Option<f64> = r.get(4).map_err(err)?;
                if let Some(value) = current { cumulative = finite(cumulative + value)?; }
                let share = current.map(|value| percent_of(value,at,valid)).transpose()?.flatten();
                let base_share = base.zip(bt).map(|(base,bt)| percent_of(base,bt,base_valid)).transpose()?.flatten();
                let mut row = json!({"key":key,"name":name,"base":base.filter(|_|base_complete),"current":current,"share":share,"baseShare":base_share,"cumulative":percent_of(cumulative,at,valid)?,"shareChange":share.zip(base_share).map(|(a,b)|a-b)});
                if kind.is_some() {
                    row["kind"] = json!(kind);
                } else {
                    row["other"] = json!(true);
                }
                rows.push(row);
            }
            result["sharesValid"] = json!(valid);
            result["baseSharesValid"] = json!(base_valid);
            result["total"] = json!(at);
        } else {
            if let Some(c) = after {
                finite(c.magnitude)?;
                key_bound(&c.key)?;
            }
            result["total_rows"] = json!(count_sql(
                &tx,
                &format!("SELECT count(*) FROM an_comparison WHERE {a}<>{b}")
            )?);
            let offset=match page_index {Some(index)=>crate::storage::direct_page_offset(index,50,result["total_rows"].as_i64().ok_or("Invalid contribution count")?)?,None=>0};
            let bt = bt.ok_or("Missing baseline total")?;
            let delta = finite(at - bt)?;
            let sum: f64 = tx
                .query_row(
                    &format!("SELECT coalesce(sum({a}-{b}),0) FROM an_comparison"),
                    [],
                    |r| r.get(0),
                )
                .map_err(err)?;
            let residual = finite(delta - sum)?;
            let tolerance = (if metric == "amount" {
                0.01_f64
            } else {
                1e-6_f64
            })
            .max(f64::EPSILON * (bt.abs() + at.abs()) * 16.);
            result["check"] = json!({"delta":delta,"sum":sum,"residual":residual,"reconciled":residual.abs()<=tolerance});
            for (field, predicate, order) in [("increases", ">", "DESC"), ("decreases", "<", "ASC")]
            {
                let mut ranked=tx.prepare(&format!("SELECT name,{a}-{b} AS delta FROM an_comparison WHERE {a}{predicate}{b} ORDER BY delta {order},key LIMIT 3")).map_err(err)?;
                let top = ranked
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))
                    .map_err(err)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(err)?;
                result[field] = json!(top);
            }
            let mut stmt = tx
                .prepare(&format!(
                    "SELECT key,name,kind,{b},{a},abs({a}-{b}) m FROM an_comparison WHERE {a}<>{b}
                 AND (?1 IS NULL OR m<?1 OR (m=?1 AND key>?2)) ORDER BY m DESC,key LIMIT 51 OFFSET ?3"
                ))
                .map_err(err)?;
            let mut data = stmt
                .query(params![after.map(|c| c.magnitude), after.map(|c| &c.key),offset])
                .map_err(err)?;
            let mut cursor = Value::Null;
            while let Some(r) = data.next().map_err(err)? {
                if rows.len() == 50 {
                    result["next"] = cursor;
                    break;
                }
                let key: String = r.get(0).map_err(err)?;
                let name: String = r.get(1).map_err(err)?;
                let kind: String = r.get(2).map_err(err)?;
                let v = measure(r.get(3).map_err(err)?, r.get(4).map_err(err)?, 0., 0.)?;
                cursor = json!({"key":key,"magnitude":v.delta.abs()});
                rows.push(json!({"key":key,"name":name,"kind":kind,"base":v.base,"current":v.current,"delta":v.delta,"percent":v.percent,"contribution":percent_of(v.delta,delta,true)?}));
            }
        }
        if mode == "structure" {
            result["total_rows"] = json!(rows.len());
        }
        result["rows"] = json!(rows);
        Ok(result)
    }
}

impl Storage {
    pub fn advanced_composition(
        &self,
        revision: i64,
        s: &AnalysisScope,
        selection: &ChangeQuery,
        grouping: Option<&str>,
        before: &RangeQuery,
        mode: &str,
        metric: &str,
        after: Option<&ContributionCursor>,
    ) -> Result<Value, String> {
        self.analysis().advanced_composition(revision, s, selection, grouping, before, mode, metric, after)
    }

    pub fn advanced_composition_limit(
        &self, revision:i64, s:&AnalysisScope, selection:&ChangeQuery, grouping:Option<&str>,
        before:&RangeQuery, mode:&str, metric:&str, after:Option<&ContributionCursor>, structure_limit:usize,
    )->Result<Value,String> {
        self.analysis().advanced_composition_limit(revision, s, selection, grouping, before, mode, metric, after, structure_limit)
    }

    pub fn advanced_composition_at_page(&self, revision:i64, s:&AnalysisScope, selection:&ChangeQuery, grouping:Option<&str>, before:&RangeQuery,mode:&str,metric:&str,structure_limit:usize,page_index:usize)->Result<Value,String> {
        self.analysis().advanced_composition_at_page(revision, s, selection, grouping, before, mode, metric, structure_limit, page_index)
    }
}

fn validate_pattern_options(mode:&str,options:&UnifiedPatternOptions)->Result<(),String> {
    if options.search.len()>512 || !matches!(options.status.as_str(),"all"|"frequent"|"rare"|"rarer"|"disappeared"|"new"|"dormant")
        || (mode=="usual" && (options.status!="all" || options.baseline.is_some()))
        || (mode!="usual" && options.range_window) {return Err("Invalid pattern filters".into());}
    Ok(())
}
