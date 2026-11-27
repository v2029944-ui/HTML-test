//! Bounded SQL presentation queries for the original interface.
use super::*;
use super::analysis_presentation::{fingerprint, next_cursor, normalized, page_offset, presentation_functions};
use super::changes::change_pair_coverage;
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
    pub top_average: Option<ChangeValue>,
    pub rows_with_missing_amount: i64,
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
#[derive(Serialize)]
pub struct UnifiedNameOption {
    pub key: String,
    pub name: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ancestor_keys: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub has_children: bool,
}
#[derive(Serialize)]
pub struct UnifiedWorkCounts {
    pub selected: i64,
    pub total: i64,
}
#[derive(Serialize)]
pub struct UnifiedNameOptions {
    pub has_omitted_names: bool,
    pub rows: Vec<UnifiedNameOption>,
    pub next: Option<String>,
    pub total_rows: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_counts: Option<UnifiedWorkCounts>,
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
            "all" | "up" | "down" | "significant-up" | "significant-down" | "new" | "gone" | "small" | "negative" | "unchanged"
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
fn significant_direction(options: &UnifiedChangeOptions, sign: &str) -> String {
    any(options, |m| format!("{m}_significant AND NOT {m}_small AND NOT {m}_negative AND {m}_delta{sign}0"))
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
        ("amount", "bamount", "camount", q.min_amount),
        ("average", "bavg", "cavg", 0.),
    ] {
        let diff = if m == "average" {
            format!("(CASE WHEN average_comparable THEN {a}-{b} END)")
        } else {
            format!("({a}-{b})")
        };
        expressions.push(format!("{b} AS {m}_base,{a} AS {m}_current,{diff} AS {m}_delta,CASE WHEN {b}<>0 THEN {diff}/abs({b})*100 END AS {m}_percent,({b}<0 OR {a}<0) AS {m}_negative,({b}=0 OR abs({b})<{min}) AS {m}_small,({diff}<>0 AND ({b}=0 OR abs({diff}/abs({b})*100)>={pct})) AS {m}_notable,({diff}<>0 AND abs({diff})>={min} AND ({b}=0 OR abs({diff}/abs({b})*100)>={pct})) AS {m}_significant",pct=q.percent));
    }
    c.execute_batch(&format!("CREATE TEMP TABLE unified_values AS WITH averages AS (
        SELECT *, coalesce(ba,b_known) bamount,coalesce(ca,a_known) camount,
        CASE WHEN ba IS NOT NULL AND bc>0 THEN ba/bc WHEN ba IS NULL AND b_paired>0 THEN b_known/b_paired END bavg,
        CASE WHEN ca IS NOT NULL AND cc>0 THEN ca/cc WHEN ca IS NULL AND a_paired>0 THEN a_known/a_paired END cavg,
        (ba IS NOT NULL AND ca IS NOT NULL) OR same_source_work average_comparable FROM an_changes
    ) SELECT *,classic_normalized(name) normalized_name,(base_observed AND current_observed AND (bc=0 AND ba=0) AND (cc<>0 OR camount<>0)) is_new,(base_observed AND current_observed AND (bc<>0 OR bamount<>0) AND (cc=0 AND ca=0)) is_gone,{} FROM averages",expressions.join(","))).map_err(err)?;
    let direction = match o.direction.as_str() {
        "significant-up" => significant_direction(o, ">"),
        "significant-down" => significant_direction(o, "<"),
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
    // The correlated lookup must not scan the filtered rows for every pair.
    c.execute_batch(&format!("CREATE INDEX temp.unified_values_pair_key ON unified_values(start,org,key);
      ALTER TABLE unified_values ADD COLUMN normal INTEGER; ALTER TABLE unified_values ADD COLUMN small INTEGER; UPDATE unified_values SET normal=coalesce({normal},0),small=NOT coalesce({normal},0) AND coalesce({small},0);
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
const COLS: &str = "start,base,org,key,name,kind,bc,ba,cc,ca,bamount,camount,bavg,cavg,average_comparable,base_observed,current_observed";

fn unified_change_value(row: &rusqlite::Row<'_>, q: &ChangeQuery) -> Result<ChangeValue, String> {
    let mut value = change_value(row, q)?;
    let number = |column| row.get::<_, Option<f64>>(column).map_err(err)?.map(finite).transpose();
    let before = number(10)?;
    let after = number(11)?;
    if row.get::<_, bool>("base_observed").map_err(err)?
        && row.get::<_, bool>("current_observed").map_err(err)? {
        value.status = status_with_observed_amounts(
            value.count.base, number(7)?, value.count.current, number(9)?, before, after,
        ).into();
    }
    value.amount = before.zip(after).map(|(b, a)| measure(b, a, q.min_amount, q.percent)).transpose()?;
    if value.amount.is_none() {
        value.observed_amount_before = before;
        value.observed_amount_after = after;
    }
    let (base, current) = (number(12)?, number(13)?);
    value.average = if row.get(14).map_err(err)? {
        average_change(base, current, q.percent)?
    } else {
        AverageChange { base, current, delta: None, percent: None, flags: None }
    };
    Ok(value)
}
fn top(c: &Connection, q: &ChangeQuery, m: &str) -> Result<Option<ChangeValue>, String> {
    let mut stmt=c.prepare(&format!("SELECT {COLS} FROM unified_values WHERE {m}_delta<>0 ORDER BY abs({m}_delta) DESC,start,org,key LIMIT 1")).map_err(err)?;
    let mut rows = stmt.query([]).map_err(err)?;
    rows.next()
        .map_err(err)?
        .map(|r| unified_change_value(r, q))
        .transpose()
}
impl Analytics<'_> {
    pub(super) fn unified_name_options(
        &self,
        revision: i64,
        search: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<UnifiedNameOptions, String> {
        self.name_options_window(revision, search, after, limit, None, None, false, None)
    }
    pub(super) fn unified_name_options_at_page(&self, revision:i64, search:&str, page_index:usize, limit:usize) -> Result<UnifiedNameOptions,String> {
        self.name_options_window(revision,search,None,limit,Some(page_index),None,false,None)
    }
    pub(super) fn unified_work_series_at_page(&self, revision:i64, page_index:usize, limit:usize) -> Result<UnifiedNameOptions,String> {
        self.name_options_window(revision,"",None,limit,Some(page_index),None,true,None)
    }
    pub(super) fn unified_selected_name_options(&self, revision: i64, keys: &[String]) -> Result<UnifiedNameOptions, String> {
        if keys.len() > 50 || keys.iter().any(String::is_empty) {
            return Err("Invalid selected name keys".into());
        }
        for key in keys { key_bound(key)?; }
        self.name_options_window(revision, "", None, 50, None, Some(keys), false, None)
    }
    fn name_options_window(&self, revision:i64, search:&str, after:Option<&str>, limit:usize, page_index:Option<usize>, selected_keys:Option<&[String]>, all_series:bool, parent:Option<&str>) -> Result<UnifiedNameOptions,String> {
        if !(1..=50).contains(&limit) || search.len() > 512 {
            return Err("Invalid name option query".into());
        }
        let mut conn = self.database.connect()?;
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
                serde_json::to_vec(&json!(["names", revision, search, limit, all_series, parent])).map_err(err)?
            )
        );
        let cursor_offset = page_offset(after, &fingerprint)?;
        let hierarchy = super::workspace::tree_indexed(&tx)?;
        let has_omitted_names = if let Some(parent) = parent {
            key_bound(parent)?;
            if !hierarchy || !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM analysis_group_nodes WHERE key=?1)",
                [parent], |row| row.get::<_, bool>(0),
            ).map_err(err)? {
                return Err("Group for this detail page is unavailable".into());
            }
            tx.execute("CREATE TEMP TABLE unified_names AS
                SELECT key,name,'group' kind,name label FROM analysis_group_nodes WHERE parent_key=?1
                UNION ALL SELECT 'work:'||work,work,'work',work FROM analysis_groups WHERE key=?1",
                [parent]).map_err(err)?;
            false
        } else {
            create_name_catalog(&tx,all_series)?
        };
        let work_counts = if hierarchy && !all_series && parent.is_none() {
            let selected = serde_json::to_string(&selected_keys.unwrap_or_default()).map_err(err)?;
            Some(tx.query_row("WITH selected AS (
                SELECT n.key,n.kind FROM json_each(?1) f JOIN unified_names n ON n.key=f.value
            ) SELECT COUNT(*) FILTER (WHERE EXISTS(
                SELECT 1 FROM selected s WHERE s.key=w.key OR (s.kind='group' AND EXISTS(
                    SELECT 1 FROM analysis_group_ancestors a
                    WHERE a.descendant_key=w.parent_key AND a.ancestor_key=s.key
                ))
            )),COUNT(*) FROM an_tree_catalog w WHERE w.kind='work'", [selected],
                |row| Ok(UnifiedWorkCounts { selected: row.get(0)?, total: row.get(1)? })
            ).map_err(err)?)
        } else {
            None
        };
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
        let metadata = if hierarchy {
            ", (SELECT json_group_array(a.ancestor_key) FROM analysis_group_ancestors a
                WHERE a.descendant_key=CASE n.kind WHEN 'group' THEN n.key
                    ELSE (SELECT key FROM analysis_groups WHERE work=n.name) END
                    AND a.ancestor_key<>n.key),
               (EXISTS(SELECT 1 FROM analysis_group_nodes child WHERE child.parent_key=n.key)
                OR EXISTS(SELECT 1 FROM analysis_groups member WHERE member.key=n.key))
             FROM unified_names n"
        } else {
            ", '[]', 0 FROM unified_names n"
        };
        let mut stmt=tx.prepare(&format!("SELECT n.key,n.label,n.kind {metadata}
            ORDER BY (instr(classic_normalized(n.name),?1)<>1),n.name COLLATE classic_ru,n.key LIMIT ?2 OFFSET ?3")).map_err(err)?;
        let mut result = stmt
            .query(params![term, (limit + 1) as i64, offset])
            .map_err(err)?;
        let mut rows = Vec::new();
        while let Some(r) = result.next().map_err(err)? {
            rows.push(UnifiedNameOption {
                key: r.get(0).map_err(err)?,
                name: r.get(1).map_err(err)?,
                kind: r.get(2).map_err(err)?,
                ancestor_keys: serde_json::from_str(&r.get::<_, String>(3).map_err(err)?).map_err(err)?,
                has_children: r.get(4).map_err(err)?,
            })
        }
        let next = if rows.len() > limit {
            rows.pop();
            Some(next_cursor(&fingerprint, offset + limit as i64)?)
        } else {
            None
        };
        Ok(UnifiedNameOptions {
            has_omitted_names,
            rows,
            next,
            total_rows,
            work_counts,
        })
    }
    pub(super) fn unified_period_options(
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
        let mut conn = self.database.connect()?;
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
    pub(super) fn unified_changes_page(
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
    pub(super) fn unified_changes_page_at_page(&self,revision:i64,s:&AnalysisScope,q:&ChangeQuery,grouping:Option<&str>,options:&UnifiedChangeOptions,page_index:usize)->Result<UnifiedChangesPage,String> {
        self.changes_window(revision,s,q,grouping,options,None,Some(page_index))
    }
    fn changes_window(&self,revision:i64,s:&AnalysisScope,q:&ChangeQuery,grouping:Option<&str>,options:&UnifiedChangeOptions,after:Option<&str>,page_index:Option<usize>)->Result<UnifiedChangesPage,String> {
        let mut conn = self.database.connect()?;
        let tx = conn.transaction().map_err(err)?;
        workspace(&tx, revision, s)?;
        let order = prepare_unified(&tx, s, q, grouping, options)?;
        let fingerprint = fingerprint(&tx, revision, s, json!([q, grouping, options]))?;
        let cursor_offset = page_offset(after, &fingerprint)?;
        let totals = changes_summary(&tx, q)?;
        let unique = |pred: &str| {
            count_sql(&tx,&format!("SELECT count(*) FROM (SELECT org,key FROM unified_values WHERE {pred} GROUP BY org,key)"))
        };
        let summary = UnifiedChangeSummary {
            rows: totals.rows,
            grown: unique(&significant_direction(options, ">"))?,
            fallen: unique(&significant_direction(options, "<"))?,
            new: unique("is_new")?,
            gone: unique("is_gone")?,
            top_count: top(&tx, q, "count")?,
            top_amount: top(&tx, q, "amount")?,
            top_average: if options.metrics.iter().any(|metric| metric == "average") {
                top(&tx, q, "average")?
            } else {
                None
            },
            rows_with_missing_amount: count_sql(&tx, "SELECT COUNT(*) FROM unified_visible WHERE ba IS NULL OR ca IS NULL")?,
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
            rows.push(unified_change_value(r, q)?)
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
    pub(super) fn unified_changes_csv(
        &self,
        revision: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        grouping: Option<&str>,
        options: &UnifiedChangeOptions,
        destination: &Path,
    ) -> Result<(), String> {
        let mut conn = self.database.connect()?;
        let tx = conn.transaction().map_err(err)?;
        workspace(&tx, revision, s)?;
        let order = prepare_unified(&tx, s, q, grouping, options)?;
        let (missing_pairs, partial_pairs) = change_pair_coverage(&tx)?;
        export(destination, |w| {
            use Cell::*;
            csv_record(
                w,
                &[
                    Text("filters"),
                    Text(&serde_json::to_string(options).map_err(err)?),
                ],
            )?;
            if missing_pairs > 0 || partial_pairs > 0 {
                let coverage = format!(
                    "Пар с неполным покрытием отчётами: {partial_pairs}. Пар без отчётов за один или оба периода: {missing_pairs}."
                );
                csv_record(w, &[Text("Качество данных"), Text(&coverage)])?;
            }
            let missing_amount_rows = count_sql(&tx, "SELECT COUNT(*) FROM unified_visible WHERE ba IS NULL OR ca IS NULL")?;
            if missing_amount_rows > 0 {
                let coverage = format!("Строк сравнения с незаполненной суммой: {missing_amount_rows}. Сумма и среднее учитывают заполненные ячейки.");
                csv_record(w, &[Text("Качество данных"), Text(&coverage)])?;
            }
            let metrics: Vec<_> = options.metrics.iter().map(String::as_str).collect();
            changes_csv_header(w, &metrics)?;
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
                change_csv(w, &unified_change_value(r, q)?, &metrics)?
            }
            Ok(())
        })
    }
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
        self.analysis().unified_name_options(revision, search, after, limit)
    }

    pub fn unified_group_children_at_page(&self, revision: i64, parent: &str, index: usize, limit: usize) -> Result<UnifiedNameOptions, String> {
        self.analysis().name_options_window(revision, "", None, limit, Some(index), None, false, Some(parent))
    }

    pub fn unified_name_options_at_page(&self, revision:i64, search:&str, page_index:usize, limit:usize) -> Result<UnifiedNameOptions,String> {
        self.analysis().unified_name_options_at_page(revision, search, page_index, limit)
    }

    pub fn unified_work_series_at_page(&self, revision:i64, page_index:usize, limit:usize) -> Result<UnifiedNameOptions,String> {
        self.analysis().unified_work_series_at_page(revision, page_index, limit)
    }

    /// One revision-consistent read returns the catalog total and the surviving selection.
    pub fn unified_selected_name_options(&self, revision: i64, keys: &[String]) -> Result<UnifiedNameOptions, String> {
        self.analysis().unified_selected_name_options(revision, keys)
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
        self.analysis().unified_period_options(revision, report_type, grouping, after, limit, week_calendar_month)
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
        self.analysis().unified_changes_page(revision, s, q, grouping, options, after)
    }

    pub fn unified_changes_page_at_page(&self,revision:i64,s:&AnalysisScope,q:&ChangeQuery,grouping:Option<&str>,options:&UnifiedChangeOptions,page_index:usize)->Result<UnifiedChangesPage,String> {
        self.analysis().unified_changes_page_at_page(revision, s, q, grouping, options, page_index)
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
        self.analysis().unified_changes_csv(revision, s, q, grouping, options, destination)
    }
}

#[cfg(test)]
#[path = "../tests/unified_analytics_tests.rs"]
mod tests;
