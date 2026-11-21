//! Bounded SQL presentation queries for the original interface.
use super::*;
use rusqlite::functions::FunctionFlags;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnifiedChangeOptions {
    pub metrics: Vec<String>,
    pub mode: String,
    pub search: String,
    pub direction: String,
    pub sort: String,
}
#[derive(Serialize)]
pub struct UnifiedChangeSummary {
    pub rows: i64,
    pub grown: i64,
    pub fallen: i64,
    pub new: i64,
    pub gone: i64,
    pub top_count: Option<ChangeValue>,
    pub top_amount: Option<ChangeValue>,
    pub totals: ChangesSummary,
}
#[derive(Serialize)]
pub struct UnifiedChangesPage {
    pub rows: Vec<ChangeValue>,
    pub next: Option<String>,
    pub total_rows: i64,
    pub summary: UnifiedChangeSummary,
    pub missing_examples: Vec<ChangePair>,
    pub first_base: Option<String>,
    pub last_current: Option<String>,
}
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
#[derive(Serialize)]
pub struct UnifiedNameOption {
    pub key: String,
    pub name: String,
    pub kind: String,
}
#[derive(Serialize)]
pub struct UnifiedNameOptions {
    pub rows: Vec<UnifiedNameOption>,
    pub next: Option<String>,
    pub total_rows: i64,
}
#[derive(Serialize)]
pub struct UnifiedPeriodOption {
    pub start: String,
    pub end: String,
}
#[derive(Serialize)]
pub struct UnifiedPeriodOptions {
    pub rows: Vec<UnifiedPeriodOption>,
    pub next: Option<String>,
    pub min: Option<String>,
    pub max: Option<String>,
    pub total_rows: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    fingerprint: String,
    offset: i64,
}

pub(super) fn normalized(value: &str) -> String {
    // Same order as the classic normalized(): trailing punctuation is removed
    // before whitespace is collapsed and trimmed.
    value
        .to_lowercase()
        .trim_end_matches(['.', ',', ';', ':'])
        .split(|c: char| c.is_whitespace() || c == '\u{feff}')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
pub(super) fn presentation_functions(c: &Connection) -> Result<(), String> {
    c.create_scalar_function(
        "classic_normalized",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| Ok(normalized(&ctx.get::<String>(0)?)),
    )
    .map_err(err)?;
    let locale: icu_provider::DataLocale = "ru".parse().map_err(err)?;
    let collator = icu_collator::Collator::try_new(&locale, Default::default()).map_err(err)?;
    c.create_collation("classic_ru", move |a, b| collator.compare(a, b))
        .map_err(err)
}
pub(super) fn fingerprint(
    c: &Connection,
    revision: i64,
    s: &AnalysisScope,
    value: Value,
) -> Result<String, String> {
    let mut hash = Sha256::new();
    hash.update(serde_json::to_vec(&(revision, &s.report_type, value)).map_err(err)?);
    for id in [Some(s.organizations.as_str()), s.names.as_deref()] {
        hash.update(if id.is_some() {
            b"selected".as_slice()
        } else {
            b"all".as_slice()
        });
        if let Some(id) = id {
            let mut stmt = c
                .prepare("SELECT item FROM analysis_filter_items WHERE id=?1 ORDER BY ordinal")
                .map_err(err)?;
            let mut rows = stmt.query([id]).map_err(err)?;
            while let Some(row) = rows.next().map_err(err)? {
                let item: String = row.get(0).map_err(err)?;
                hash.update((item.len() as u64).to_le_bytes());
                hash.update(item.as_bytes());
            }
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub(super) fn page_offset(after: Option<&str>, fingerprint: &str) -> Result<i64, String> {
    let Some(after) = after else { return Ok(0) };
    if after.len() > 512 {
        return Err("Invalid analysis cursor".into());
    }
    let cursor: Cursor =
        serde_json::from_str(after).map_err(|_| "Invalid analysis cursor".to_string())?;
    if cursor.fingerprint != fingerprint || cursor.offset < 0 || cursor.offset > i64::from(i32::MAX)
    {
        return Err("Analysis cursor belongs to another query or revision".into());
    }
    Ok(cursor.offset)
}
pub(super) fn next_cursor(fingerprint: &str, offset: i64) -> Result<String, String> {
    serde_json::to_string(&Cursor {
        fingerprint: fingerprint.into(),
        offset,
    })
    .map_err(err)
}
fn validate_options(o: &UnifiedChangeOptions) -> Result<(), String> {
    if o.metrics.is_empty()
        || o.metrics.len() > 3
        || o.metrics
            .iter()
            .any(|m| !matches!(m.as_str(), "count" | "amount" | "average"))
        || o.metrics
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != o.metrics.len()
        || !matches!(o.mode.as_str(), "strong" | "all")
        || !matches!(
            o.direction.as_str(),
            "all" | "up" | "down" | "new" | "gone" | "small" | "negative" | "unchanged"
        )
        || o.search.len() > 512
    {
        return Err("Invalid change filters".into());
    }
    if o.sort != "name" {
        let (kind, metric) = o.sort.split_once('-').ok_or("Invalid change sort")?;
        if !matches!(kind, "significance" | "absolute" | "percent" | "current")
            || !o.metrics.iter().any(|m| m == metric)
        {
            return Err("Invalid change sort".into());
        }
    }
    Ok(())
}
fn any(o: &UnifiedChangeOptions, expr: impl Fn(&str) -> String) -> String {
    format!(
        "({})",
        o.metrics
            .iter()
            .map(|m| expr(m))
            .collect::<Vec<_>>()
            .join(" OR ")
    )
}
fn all(o: &UnifiedChangeOptions, expr: impl Fn(&str) -> String) -> String {
    format!(
        "({})",
        o.metrics
            .iter()
            .map(|m| expr(m))
            .collect::<Vec<_>>()
            .join(" AND ")
    )
}
fn prepare_unified(
    c: &Connection,
    s: &AnalysisScope,
    q: &ChangeQuery,
    g: Option<&str>,
    o: &UnifiedChangeOptions,
) -> Result<String, String> {
    validate_options(o)?;
    presentation_functions(c)?;
    advanced_analysis::prepare_changes(c, s, q, g)?;
    // Every metric and flag is computed in SQL before filtering, sorting or
    // LIMIT. This never materializes report history or all change rows in Rust.
    let mut expressions = Vec::new();
    for (m, b, a, min) in [
        ("count", "bc", "cc", q.min_count),
        ("amount", "ba", "ca", q.min_amount),
        ("average", "bavg", "cavg", 0.),
    ] {
        expressions.push(format!("{b} AS {m}_base,{a} AS {m}_current,({a}-{b}) AS {m}_delta,CASE WHEN {b}<>0 THEN ({a}-{b})/abs({b})*100 END AS {m}_percent,({b}<0 OR {a}<0) AS {m}_negative,({b}=0 OR abs({b})<{min}) AS {m}_small,(({a}-{b})<>0 AND ({b}=0 OR abs(({a}-{b})/abs({b})*100)>={pct})) AS {m}_notable,(({a}-{b})<>0 AND abs({a}-{b})>={min} AND ({b}=0 OR abs(({a}-{b})/abs({b})*100)>={pct})) AS {m}_significant",pct=q.percent));
    }
    c.execute_batch(&format!("CREATE TEMP TABLE unified_values AS WITH averages AS (SELECT *,CASE WHEN bc>0 THEN ba/bc END bavg,CASE WHEN cc>0 THEN ca/cc END cavg FROM an_changes) SELECT *,classic_normalized(name) normalized_name,((bc=0 AND ba=0) AND (cc<>0 OR ca<>0)) is_new,((bc<>0 OR ba<>0) AND (cc=0 AND ca=0)) is_gone,{} FROM averages",expressions.join(","))).map_err(err)?;
    let direction = match o.direction.as_str() {
        "new" => "is_new".into(),
        "gone" => "is_gone".into(),
        "small" => any(o, |m| format!("{m}_small AND {m}_delta<>0")),
        "negative" => any(o, |m| format!("{m}_negative")),
        "unchanged" => all(o, |m| format!("{m}_delta=0")),
        "up" | "down" => any(o, |m| {
            format!(
                "NOT {m}_negative AND {m}_delta{}0",
                if o.direction == "up" { ">" } else { "<" }
            )
        }),
        _ => "1".into(),
    };
    c.execute(&format!("DELETE FROM unified_values WHERE NOT coalesce(({direction}),0) OR instr(normalized_name,?1)=0"),[normalized(&o.search)]).map_err(err)?;
    let normal = any(o, |m| format!("{m}_significant AND NOT {m}_small"));
    let small = any(o, |m| format!("{m}_small AND {m}_notable"));
    c.execute_batch(&format!("ALTER TABLE unified_values ADD COLUMN normal INTEGER; ALTER TABLE unified_values ADD COLUMN small INTEGER; UPDATE unified_values SET normal=coalesce({normal},0),small=NOT coalesce({normal},0) AND coalesce({small},0);
      DELETE FROM an_changes WHERE NOT EXISTS(SELECT 1 FROM unified_values u WHERE (u.start,u.org,u.key)=(an_changes.start,an_changes.org,an_changes.key));
      CREATE TEMP TABLE unified_visible AS SELECT * FROM unified_values WHERE {}",if o.mode=="strong" {"normal OR small"}else{"1"})).map_err(err)?;
    let order = if !normalized(&o.search).is_empty() {
        "(instr(normalized_name,?1)<>1),name COLLATE classic_ru,start,org COLLATE classic_ru,key"
            .into()
    } else {
        let section = format!(
            "(small OR (NOT normal AND {}))",
            all(o, |m| format!("coalesce({m}_small,0)"))
        );
        let main = if o.sort == "name" {
            "name COLLATE classic_ru,org COLLATE classic_ru".into()
        } else {
            let (kind, m) = o.sort.split_once('-').unwrap();
            let score = match kind {
                "percent" => format!("abs({m}_percent) DESC"),
                "current" => format!("coalesce({m}_current,0) DESC"),
                _ => format!("coalesce(abs({m}_delta),0) DESC"),
            };
            format!(
                "{}{score},name COLLATE classic_ru,start",
                if kind == "significance" {
                    format!("coalesce({m}_significant AND NOT {m}_small,0) DESC,")
                } else {
                    String::new()
                }
            )
        };
        format!("{section},{main},org COLLATE classic_ru,key,start")
    };
    Ok(order)
}
const COLS: &str = "start,base,org,key,name,kind,bc,ba,cc,ca";
fn top(c: &Connection, q: &ChangeQuery, m: &str) -> Result<Option<ChangeValue>, String> {
    let mut stmt=c.prepare(&format!("SELECT {COLS} FROM unified_values WHERE {m}_delta<>0 ORDER BY abs({m}_delta) DESC,start,org,key LIMIT 1")).map_err(err)?;
    let mut rows = stmt.query([]).map_err(err)?;
    rows.next()
        .map_err(err)?
        .map(|r| change_value(r, q))
        .transpose()
}
pub(super) fn create_name_catalog(tx: &Connection, all_series: bool) -> Result<(), String> {
    // Name-based selection must be unambiguous. The all-work chart uses identity
    // keys and can safely display both members of a matching work/group pair.
    let labels = if all_series {
        "SELECT key,name,kind,
                CASE WHEN count(*) OVER(PARTITION BY name)>1
                     THEN name || CASE kind WHEN 'group' THEN ' (группа)' ELSE ' (работа)' END
                     ELSE name END label FROM candidates WHERE name<>''"
    } else {
        "SELECT key,name,min(kind) kind,name label FROM candidates
         WHERE name<>'' AND length(CAST(name AS BLOB))<=512 GROUP BY name HAVING count(*)=1"
    };
    tx.execute_batch(&format!("CREATE TEMP TABLE unified_names AS WITH candidates AS (
            SELECT g.key,g.name,'group' kind FROM analysis_groups g GROUP BY g.key,g.name
            UNION ALL SELECT 'work:'||x.work,x.work,'work' FROM report_rows x
            WHERE NOT EXISTS(SELECT 1 FROM analysis_groups g WHERE g.work=x.work) GROUP BY x.work
        ) {labels}")).map_err(err)
}
impl Storage {
    /// Metadata only; each response is bounded even for very large history.
    pub fn unified_name_options(
        &self,
        revision: i64,
        search: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<UnifiedNameOptions, String> {
        self.name_options_window(revision, search, after, limit, None, None, false)
    }
    pub fn unified_name_options_at_page(&self, revision:i64, search:&str, page_index:usize, limit:usize) -> Result<UnifiedNameOptions,String> {
        self.name_options_window(revision,search,None,limit,Some(page_index),None,false)
    }
    pub fn unified_work_series_at_page(&self, revision:i64, page_index:usize, limit:usize) -> Result<UnifiedNameOptions,String> {
        self.name_options_window(revision,"",None,limit,Some(page_index),None,true)
    }
    /// One revision-consistent read returns the catalog total and the surviving selection.
    pub fn unified_selected_name_options(&self, revision: i64, keys: &[String]) -> Result<UnifiedNameOptions, String> {
        if keys.len() > 50 || keys.iter().any(String::is_empty) {
            return Err("Invalid selected name keys".into());
        }
        for key in keys { key_bound(key)?; }
        self.name_options_window(revision, "", None, 50, None, Some(keys), false)
    }
    fn name_options_window(&self, revision:i64, search:&str, after:Option<&str>, limit:usize, page_index:Option<usize>, selected_keys:Option<&[String]>, all_series:bool) -> Result<UnifiedNameOptions,String> {
        if !(1..=50).contains(&limit) || search.len() > 512 {
            return Err("Invalid name option query".into());
        }
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(err)?;
        check_revision(&tx, revision)?;
        check_metadata_bounds(&tx)?;
        presentation_functions(&tx)?;
        let indexed: bool = tx
            .query_row("SELECT EXISTS(SELECT 1 FROM analysis_settings)", [], |r| {
                r.get(0)
            })
            .map_err(err)?;
        if !indexed {
            return Err("Analysis groups are not indexed".into());
        }
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&json!(["names", revision, search, limit, all_series])).map_err(err)?
            )
        );
        let cursor_offset = page_offset(after, &fingerprint)?;
        create_name_catalog(&tx,all_series)?;
        let term = normalized(search);
        tx.execute(
            "DELETE FROM unified_names WHERE instr(classic_normalized(name),?1)=0",
            [&term],
        )
        .map_err(err)?;
        let total_rows = count_sql(&tx, "SELECT count(*) FROM unified_names")?;
        if let Some(keys) = selected_keys {
            tx.execute("DELETE FROM unified_names WHERE key NOT IN (SELECT value FROM json_each(?1))",
                [serde_json::to_string(keys).map_err(err)?]).map_err(err)?;
        }
        let offset=match page_index {Some(index)=>crate::storage::direct_page_offset(index,limit,total_rows)?,None=>cursor_offset};
        let mut stmt=tx.prepare("SELECT key,label,kind FROM unified_names ORDER BY (instr(classic_normalized(name),?1)<>1),name COLLATE classic_ru,key LIMIT ?2 OFFSET ?3").map_err(err)?;
        let mut result = stmt
            .query(params![term, (limit + 1) as i64, offset])
            .map_err(err)?;
        let mut rows = Vec::new();
        while let Some(r) = result.next().map_err(err)? {
            rows.push(UnifiedNameOption {
                key: r.get(0).map_err(err)?,
                name: r.get(1).map_err(err)?,
                kind: r.get(2).map_err(err)?,
            })
        }
        let next = if rows.len() > limit {
            rows.pop();
            Some(next_cursor(&fingerprint, offset + limit as i64)?)
        } else {
            None
        };
        Ok(UnifiedNameOptions {
            rows,
            next,
            total_rows,
        })
    }
    /// Genuine report-period options (no generated zero-valued report facts).
    /// Daily sources can expose calendar week/month/year labels. Coverage is
    /// still verified separately by the calculation API, never inferred here.
    pub fn unified_period_options(
        &self,
        revision: i64,
        report_type: &str,
        grouping: Option<&str>,
        after: Option<&str>,
        limit: usize,
        week_calendar_month: Option<&str>,
    ) -> Result<UnifiedPeriodOptions, String> {
        if !matches!(report_type, "day" | "week" | "month")
            || !(1..=500).contains(&limit)
            || week_calendar_month.is_some_and(|day| report_type != "week" || !valid_date(day) || !day.ends_with("-01"))
            || grouping.is_some_and(|g| {
                report_type != "day" || !matches!(g, "day" | "week" | "month" | "year")
            })
        {
            return Err("Invalid period option query".into());
        }
        let kind = grouping.unwrap_or(report_type);
        let start=match kind {
            "week" if report_type=="day"=>"date(r.period_start,'-'||((cast(strftime('%w',r.period_start) as integer)+6)%7)||' days')",
            "month"=>"substr(r.period_start,1,7)||'-01'",
            "year"=>"substr(r.period_start,1,4)||'-01-01'",
            _=>"r.period_start"
        };
        let ending = match kind {
            "week" => "date(start,'+6 days')",
            "month" => "date(start,'+1 month','-1 day')",
            "year" => "date(start,'+1 year','-1 day')",
            _ => "start",
        };
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(err)?;
        check_revision(&tx, revision)?;
        check_metadata_bounds(&tx)?;
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&json!(["periods", revision, report_type, grouping, limit, week_calendar_month]))
                    .map_err(err)?
            )
        );
        let offset = page_offset(after, &fingerprint)?;
        // For a calendar month, index validation and lookup share the reports_by_period window.
        // At most 37 different starts can touch the month.
        let window = if week_calendar_month.is_some() {
            " AND r.period_start BETWEEN date(?2,'-6 days') AND date(?2,'+1 month','-1 day')"
        } else {
            " AND ?2 IS NULL"
        };
        let incomplete: bool = tx.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM reports r LEFT JOIN analysis_reports a ON a.report_id=r.id WHERE r.report_type=?1 AND a.report_id IS NULL{window})"),
            params![report_type,week_calendar_month], |r|r.get(0)).map_err(err)?;
        if incomplete {
            return Err("Report analysis index is incomplete".into());
        }
        tx.execute(&format!("CREATE TEMP TABLE unified_periods AS SELECT DISTINCT {start} start FROM reports r JOIN analysis_reports a ON a.report_id=r.id WHERE a.valid=1 AND r.report_type=?1{window}"),
            params![report_type,week_calendar_month]).map_err(err)?;
        let (min, max, total_rows) = tx
            .query_row(
                "SELECT min(start),max(start),count(*) FROM unified_periods",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(err)?;
        let mut stmt = tx
            .prepare(&format!(
                "SELECT start,{ending} FROM unified_periods ORDER BY start LIMIT ?1 OFFSET ?2"
            ))
            .map_err(err)?;
        let mut result = stmt
            .query(params![(limit + 1) as i64, offset])
            .map_err(err)?;
        let mut rows = Vec::new();
        while let Some(r) = result.next().map_err(err)? {
            rows.push(UnifiedPeriodOption {
                start: r.get(0).map_err(err)?,
                end: r.get(1).map_err(err)?,
            })
        }
        let next = if rows.len() > limit {
            rows.pop();
            Some(next_cursor(&fingerprint, offset + limit as i64)?)
        } else {
            None
        };
        Ok(UnifiedPeriodOptions {
            rows,
            next,
            min,
            max,
            total_rows,
        })
    }
    pub fn unified_changes_page(
        &self,
        revision: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        grouping: Option<&str>,
        options: &UnifiedChangeOptions,
        after: Option<&str>,
    ) -> Result<UnifiedChangesPage, String> {
        self.changes_window(revision,s,q,grouping,options,after,None)
    }
    pub fn unified_changes_page_at_page(&self,revision:i64,s:&AnalysisScope,q:&ChangeQuery,grouping:Option<&str>,options:&UnifiedChangeOptions,page_index:usize)->Result<UnifiedChangesPage,String> {
        self.changes_window(revision,s,q,grouping,options,None,Some(page_index))
    }
    fn changes_window(&self,revision:i64,s:&AnalysisScope,q:&ChangeQuery,grouping:Option<&str>,options:&UnifiedChangeOptions,after:Option<&str>,page_index:Option<usize>)->Result<UnifiedChangesPage,String> {
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(err)?;
        workspace(&tx, revision, s)?;
        let order = prepare_unified(&tx, s, q, grouping, options)?;
        let fingerprint = fingerprint(&tx, revision, s, json!([q, grouping, options]))?;
        let cursor_offset = page_offset(after, &fingerprint)?;
        let totals = changes_summary(&tx, q)?;
        let unique = |pred: &str| {
            count_sql(&tx,&format!("SELECT count(*) FROM (SELECT org,key FROM unified_values WHERE {pred} GROUP BY org,key)"))
        };
        let meaningful = |sign: &str| {
            any(options, |m| {
                format!(
                    "{m}_significant AND NOT {m}_small AND NOT {m}_negative AND {m}_delta{sign}0"
                )
            })
        };
        let summary = UnifiedChangeSummary {
            rows: totals.rows,
            grown: unique(&meaningful(">"))?,
            fallen: unique(&meaningful("<"))?,
            new: unique("is_new")?,
            gone: unique("is_gone")?,
            top_count: top(&tx, q, "count")?,
            top_amount: top(&tx, q, "amount")?,
            totals,
        };
        let total_rows = count_sql(&tx, "SELECT count(*) FROM unified_visible")?;
        let offset=match page_index {Some(index)=>crate::storage::direct_page_offset(index,50,total_rows)?,None=>cursor_offset};
        if offset % 50 != 0 || (offset > 0 && offset >= total_rows) {
            return Err("Invalid change page cursor".into());
        }
        let mut stmt = tx
            .prepare(&format!(
                "SELECT {COLS} FROM unified_visible ORDER BY {order} LIMIT 51 OFFSET ?2"
            ))
            .map_err(err)?;
        let mut result = stmt
            .query(params![normalized(&options.search), offset])
            .map_err(err)?;
        let mut rows = Vec::new();
        while let Some(r) = result.next().map_err(err)? {
            rows.push(change_value(r, q)?)
        }
        let next = if rows.len() > 50 {
            rows.pop();
            Some(next_cursor(&fingerprint, offset + 50)?)
        } else {
            None
        };
        let (first_base, last_current) = tx
            .query_row("SELECT min(base),max(start) FROM an_currents", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(err)?;
        let mut stmt=tx.prepare("SELECT start,base,org,base_id IS NULL,current_id IS NULL,warned<>0 FROM an_pairs WHERE base_id IS NULL OR current_id IS NULL ORDER BY start,org LIMIT 5").map_err(err)?;
        let missing_examples = stmt
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
        Ok(UnifiedChangesPage {
            rows,
            next,
            total_rows,
            summary,
            missing_examples,
            first_base,
            last_current,
        })
    }
    pub fn unified_changes_csv(
        &self,
        revision: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        grouping: Option<&str>,
        options: &UnifiedChangeOptions,
        destination: &Path,
    ) -> Result<(), String> {
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(err)?;
        workspace(&tx, revision, s)?;
        let order = prepare_unified(&tx, s, q, grouping, options)?;
        export(destination, |w| {
            use Cell::*;
            csv_record(
                w,
                &[
                    Text("filters"),
                    Text(&serde_json::to_string(options).map_err(err)?),
                ],
            )?;
            // Preserve existing guarded CSV encoding/formula protection.
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
            let mut stmt = tx
                .prepare(&format!(
                    "SELECT {COLS} FROM unified_visible ORDER BY {order}"
                ))
                .map_err(err)?;
            // ?1 is present only in the search ordering.
            let mut rows = if order.contains("?1") {
                stmt.query([normalized(&options.search)])
            } else {
                stmt.query([])
            }
            .map_err(err)?;
            while let Some(r) = rows.next().map_err(err)? {
                change_csv(w, &change_value(r, q)?)?
            }
            Ok(())
        })
    }
}

#[cfg(test)]
#[path = "unified_analytics_tests.rs"]
mod tests;
