//! Independent current/baseline intervals over real work × organization facts.
use super::*;
use std::{collections::BTreeSet, sync::atomic::AtomicBool};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReferenceChangesRequest {
    pub revision: i64,
    pub report_type: String,
    pub from: String,
    pub to: String,
    pub base_from: String,
    pub base_to: String,
    pub organizations: Vec<String>,
    pub metric: String,
    pub percent: f64,
    pub min_count: f64,
    pub min_amount: f64,
    pub kind: String,
    pub search: String,
    pub sort: String,
    pub page: u32,
}
#[derive(Debug, Serialize)]
pub struct ReferenceChangeRow {
    pub name: String,
    pub org: String,
    pub before: Option<f64>,
    pub after: Option<f64>,
    pub delta: Option<f64>,
    pub percent: Option<f64>,
}
#[derive(Debug, Serialize)]
pub struct ReferenceChangeCounts {
    pub up: i64,
    pub down: i64,
    pub new: i64,
    pub gone: i64,
}
#[derive(Debug, Serialize)]
pub struct ReferenceChangesPage {
    pub rows: Vec<ReferenceChangeRow>,
    pub total: i64,
    pub searched: i64,
    pub significant: i64,
    pub counts: ReferenceChangeCounts,
    pub leader: Option<ReferenceChangeRow>,
    pub comparable: Vec<String>,
    pub partial_fallback: bool,
}
impl ReferenceChangesRequest {
    fn validate(&self) -> Result<(), String> {
        if !(1..=100).contains(&self.organizations.len())
            || self.organizations.iter().collect::<BTreeSet<_>>().len() != self.organizations.len()
            || self
                .organizations
                .iter()
                .any(|v| v.is_empty() || v.len() > BOUND)
            || !matches!(self.metric.as_str(), "count" | "amount")
            || !matches!(
                self.kind.as_str(),
                "significant" | "all" | "up" | "down" | "new" | "gone"
            )
            || !matches!(self.sort.as_str(), "delta" | "percent" | "name")
            || self.search.len() > 512
            || i64::from(self.page) * 12 > i64::from(i32::MAX)
            || [self.percent, self.min_count, self.min_amount]
                .iter()
                .any(|n| !n.is_finite() || *n < 0.)
        {
            return Err("Invalid reference changes query".into());
        }
        for query in [self.current(), self.baseline()] {
            let (first, last) = range_bounds(&self.report_type, &query)?;
            if (last - first).num_days() > 3660 {
                return Err("Select up to ten years per interval".into());
            }
        }
        Ok(())
    }
    fn current(&self) -> RangeQuery {
        RangeQuery {
            from: self.from.clone(),
            to: self.to.clone(),
        }
    }
    fn baseline(&self) -> RangeQuery {
        RangeQuery {
            from: self.base_from.clone(),
            to: self.base_to.clone(),
        }
    }
}
fn periods(c: &Connection, s: &AnalysisScope, q: &RangeQuery, slot: i64) -> Result<(), String> {
    let (mut current, last) = range_bounds(&s.report_type, q)?;
    loop {
        c.execute(
            "INSERT INTO an_periods VALUES(?1,?2)",
            params![slot, current.to_string()],
        )
        .map_err(err)?;
        if current == last {
            break;
        }
        current = shift(&s.report_type, current, 1)?;
    }
    c.execute("INSERT INTO an_missing SELECT ?1,f.item,p.start FROM analysis_filter_items f CROSS JOIN an_periods p LEFT JOIN an_valid v ON v.org=f.item AND v.start=p.start WHERE f.id=?2 AND p.slot=?1 AND v.report_id IS NULL",params![slot,s.organizations]).map_err(err)?;
    Ok(())
}
fn prepare_reference_values(
    c: &Connection,
    q: &ReferenceChangesRequest,
    s: &AnalysisScope,
) -> Result<(), String> {
    workspace(c, q.revision, s)?;
    unified_analytics::presentation_functions(c)?;
    periods(c, s, &q.baseline(), 1)?;
    periods(c, s, &q.current(), 2)?;
    c.execute_batch("CREATE TEMP TABLE ref_orgs(org TEXT PRIMARY KEY) WITHOUT ROWID;
        CREATE TEMP TABLE ref_values(slot INTEGER,org TEXT,name TEXT,count REAL,amount REAL,known_amount REAL,PRIMARY KEY(slot,org,name)) WITHOUT ROWID;").map_err(err)?;
    c.execute("INSERT INTO ref_orgs SELECT f.item FROM analysis_filter_items f WHERE f.id=?1 AND NOT EXISTS(SELECT 1 FROM an_missing m WHERE m.org=f.item)",[&s.organizations]).map_err(err)?;
    c.execute_batch("INSERT INTO ref_values SELECT p.slot,v.org,x.work,sum(x.count),CASE WHEN COUNT(x.amount)=COUNT(*) THEN SUM(x.amount) END,SUM(x.amount)
        FROM an_valid v JOIN an_periods p ON p.start=v.start
        JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org GROUP BY p.slot,v.org,x.work;").map_err(err)?;
    validate_numbers(c, "SELECT count,amount FROM ref_values")?;
    validate_numbers(c, "SELECT count,known_amount FROM ref_values")
}
fn prepare_reference(c:&Connection,q:&ReferenceChangesRequest,s:&AnalysisScope)->Result<(),String> {
    prepare_reference_values(c,q,s)?;
    c.execute_batch("CREATE TEMP TABLE ref_changes(name TEXT,org TEXT,before REAL,after REAL,delta REAL,percent REAL,kind TEXT,PRIMARY KEY(name,org)) WITHOUT ROWID;").map_err(err)?;
    let mut statement=c.prepare("WITH keys AS (SELECT DISTINCT org,name FROM ref_values),
        presence AS (SELECT p.slot,v.org FROM an_valid v JOIN an_periods p ON v.start=p.start GROUP BY p.slot,v.org)
        SELECT k.name,k.org,coalesce(b.count,0.0),CASE WHEN b.name IS NULL THEN 0 ELSE b.amount END,
               coalesce(a.count,0.0),CASE WHEN a.name IS NULL THEN 0 ELSE a.amount END,
               b.known_amount,a.known_amount,bp.org IS NOT NULL,ap.org IS NOT NULL,o.org IS NOT NULL
        FROM keys k LEFT JOIN ref_values b ON b.org=k.org AND b.name=k.name AND b.slot=1
        LEFT JOIN ref_values a ON a.org=k.org AND a.name=k.name AND a.slot=2
        LEFT JOIN presence bp ON bp.org=k.org AND bp.slot=1
        LEFT JOIN presence ap ON ap.org=k.org AND ap.slot=2
        LEFT JOIN ref_orgs o ON o.org=k.org").map_err(err)?;
    let mut rows = statement.query([]).map_err(err)?;
    let mut put = c
        .prepare("INSERT INTO ref_changes VALUES(?1,?2,?3,?4,?5,?6,?7)")
        .map_err(err)?;
    while let Some(row) = rows.next().map_err(err)? {
        let name: String = row.get(0).map_err(err)?;
        let org: String = row.get(1).map_err(err)?;
        let bc: f64 = row.get(2).map_err(err)?;
        let ba: Option<f64> = row.get(3).map_err(err)?;
        let ac: f64 = row.get(4).map_err(err)?;
        let aa: Option<f64> = row.get(5).map_err(err)?;
        let before = observed_totals(bc,ba,row.get(6).map_err(err)?,row.get(8).map_err(err)?)?;
        let after = observed_totals(ac,aa,row.get(7).map_err(err)?,row.get(9).map_err(err)?)?;
        let complete: bool = row.get(10).map_err(err)?;
        let (old, new, strict_pair, minimum) = if q.metric == "count" {
            (before.count, after.count, Some((bc,ac)), q.min_count)
        } else {
            (before.amount, after.amount, ba.zip(aa), q.min_amount)
        };
        let Some((b, a)) = strict_pair.filter(|_| before.count.is_some() && after.count.is_some()) else {
            put.execute(params![name, org, old, new, None::<f64>, None::<f64>, ""])
                .map_err(err)?;
            continue;
        };
        let value = measure(b, a, minimum, q.percent)?;
        let kind = if value.delta == 0. {
            ""
        } else if complete && bc == 0. && ba == Some(0.) && (ac != 0. || aa.is_some_and(|n| n != 0.)) {
            "new"
        } else if complete && (bc != 0. || ba.is_some_and(|n| n != 0.)) && ac == 0. && aa == Some(0.) {
            "gone"
        } else if (b != 0. || !complete) && value.significant {
            if value.delta > 0. {
                "up"
            } else {
                "down"
            }
        } else {
            ""
        };
        put.execute(params![
            name,
            org,
            old,
            new,
            value.delta,
            value.percent,
            kind
        ])
        .map_err(err)?;
    }
    Ok(())
}
fn order(q: &ReferenceChangesRequest) -> &'static str {
    match q.sort.as_str() {
        "percent"=>"(percent IS NULL),abs(percent) DESC,abs(delta) DESC,name COLLATE classic_ru,org COLLATE classic_ru,name,org",
        "name"=>"name COLLATE classic_ru,org COLLATE classic_ru,name,org",
        _=>"abs(delta) DESC,name COLLATE classic_ru,org COLLATE classic_ru,name,org",
    }
}
const CHANGE_KIND_FILTER:&str="(?1='all' OR (?1='significant' AND (
    kind<>'' OR (delta IS NULL AND (before IS NOT NULL OR after IS NOT NULL))
)) OR kind=?1)";
fn change_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReferenceChangeRow> {
    Ok(ReferenceChangeRow {
        name: row.get(0)?,
        org: row.get(1)?,
        before: row.get(2)?,
        after: row.get(3)?,
        delta: row.get(4)?,
        percent: row.get(5)?,
    })
}
fn comparable_organizations(c: &Connection, scope: &AnalysisScope) -> Result<Vec<String>, String> {
    let mut stmt = c.prepare("SELECT f.item FROM analysis_filter_items f JOIN ref_orgs o ON o.org=f.item WHERE f.id=?1 ORDER BY f.ordinal").map_err(err)?;
    let rows = stmt.query_map([&scope.organizations], |row| row.get(0)).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

fn filtered_rows_sql(q: &ReferenceChangesRequest) -> String {
    format!("SELECT name,org,before,after,delta,percent FROM ref_changes WHERE {CHANGE_KIND_FILTER} AND instr(classic_normalized(name),?2)>0 ORDER BY {}", order(q))
}

fn comparison_csv_context(
    writer: &mut impl Write,
    c: &Connection,
    q: &ReferenceChangesRequest,
    s: &AnalysisScope,
    title: &str,
) -> Result<(), String> {
    use Cell::{Number, Text};
    let current_end=end(&q.report_type,date(&q.to)?)?.to_string();
    let baseline_end=end(&q.report_type,date(&q.base_to)?)?.to_string();
    csv_record(writer,&[Text(title),Text(&q.from),Text(&current_end)])?;
    csv_record(writer,&[Text("База сравнения"),Text(&q.base_from),Text(&baseline_end)])?;
    csv_record(writer,&[Text("Тип исходных отчётов"),Text(&q.report_type)])?;
    csv_record(writer,&[Text("Область работ"),Text("Все работы")])?;
    csv_record(writer,&[Text("Выбранные подразделения"),Text(&q.organizations.join(", "))])?;
    let members=c.prepare("SELECT f.item,
        NOT EXISTS(SELECT 1 FROM an_missing m WHERE m.org=f.item AND m.slot=1),
        NOT EXISTS(SELECT 1 FROM an_missing m WHERE m.org=f.item AND m.slot=2)
        FROM analysis_filter_items f WHERE f.id=?1 ORDER BY f.ordinal").map_err(err)?
        .query_map([&s.organizations],|row|Ok((row.get::<_,String>(0)?,row.get::<_,bool>(1)?,row.get::<_,bool>(2)?))).map_err(err)?
        .collect::<Result<Vec<_>,_>>().map_err(err)?;
    let included:Vec<_>=members.iter().filter(|(_,before,after)|*before&&*after).map(|(org,_,_)|org.as_str()).collect();
    csv_record(writer,&[Text("Сопоставимые подразделения"),Number(Some(included.len() as f64)),Number(Some(members.len() as f64)),Text(&included.join(", "))])?;
    csv_record(writer,&[Text("Выборка сравнения"),Text("Сводные итоги учитывают доступные отчёты выбранных подразделений. Пропуск не означает ноль.")])?;
    for (org,before,after) in &members {
        let reason=match (*before,*after) {(true,true)=>"включено",(false,false)=>"неполные базовый и анализируемый интервалы",(false,true)=>"неполный базовый интервал",(true,false)=>"неполный анализируемый интервал"};
        csv_record(writer,&[Text("Состав сравнения"),Text(org),Text(reason)])?;
    }
    let observed = |slot| -> Result<(AnalysisTotals, Option<f64>), String> {
        let (count, amount, known_amount) = c.query_row("SELECT coalesce(sum(count),0.0),
            CASE WHEN COUNT(*)=0 THEN 0 WHEN COUNT(amount)=COUNT(*) THEN SUM(amount) END,
            SUM(known_amount) FROM ref_values WHERE slot=?1", [slot], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(err)?;
        let available = observed_reports(c, slot, None)?;
        Ok((observed_totals(count, amount, known_amount, available)?, amount.filter(|_| available)))
    };
    let (before, before_amount) = observed(1)?;
    let (after, after_amount) = observed(2)?;
    csv_record(writer,&[Text("Сравнение по доступным отчётам · все работы")])?;
    csv_record(writer,&[Text("Показатель"),Text("Было"),Text("Стало"),Text("Изменение"),Text("Изменение, %")])?;
    for (label,before,after,pair) in [("Сумма",before.amount,after.amount,before_amount.zip(after_amount)),
        ("Количество / объём",before.count,after.count,before.count.zip(after.count)),
        ("Сумма / количество (объём)",before.average,after.average,before.average.zip(after.average))] {
        let change=pair.map(|(before,after)|measure(before,after,0.,0.)).transpose()?;
        csv_record(writer,&[Text(label),Number(before),Number(after),Number(change.as_ref().map(|value|value.delta)),Number(change.and_then(|value|value.percent))])?;
    }
    csv_record(writer,&[])
}
impl Storage {
    fn reference_scope<T>(
        &self,
        q: &ReferenceChangesRequest,
        run: impl FnOnce(&AnalysisScope) -> Result<T, String>,
    ) -> Result<T, String> {
        q.validate()?;
        let id = self.analysis_filter_create(q.revision, "organizations")?;
        let result = (|| {
            self.analysis_filter_append(q.revision, &id, &q.organizations)?;
            self.analysis_filter_seal(q.revision, &id)?;
            run(&AnalysisScope {
                report_type: q.report_type.clone(),
                organizations: id.clone(),
                names: None,
            })
        })();
        let cleanup = self.analysis_filter_delete(&id);
        match (result, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(e), _) | (_, Err(e)) => Err(e),
        }
    }
    pub fn reference_changes(
        &self,
        q: &ReferenceChangesRequest,
    ) -> Result<ReferenceChangesPage, String> {
        self.reference_scope(q,|scope| {
            let mut conn=self.connect()?;let tx=conn.transaction().map_err(err)?;
            prepare_reference(&tx,q,scope)?;
            let counts=tx.query_row("SELECT coalesce(sum(kind='up'),0),coalesce(sum(kind='down'),0),coalesce(sum(kind='new'),0),coalesce(sum(kind='gone'),0) FROM ref_changes",[],|r|Ok(ReferenceChangeCounts {up:r.get(0)?,down:r.get(1)?,new:r.get(2)?,gone:r.get(3)?})).map_err(err)?;
            let leader=tx.query_row("SELECT name,org,before,after,delta,percent FROM ref_changes WHERE delta IS NOT NULL ORDER BY abs(delta) DESC,name COLLATE classic_ru,org COLLATE classic_ru,name,org LIMIT 1",[],change_row).optional().map_err(err)?;
            let comparable=comparable_organizations(&tx,scope)?;
            let search=unified_analytics::normalized(&q.search);
            let (total,searched,significant)=tx.query_row(
                &format!("SELECT count(*) FILTER (WHERE {CHANGE_KIND_FILTER}),
                        count(*), count(*) FILTER (WHERE kind<>'')
                 FROM ref_changes WHERE instr(classic_normalized(name),?2)>0"),
                params![q.kind,search],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))
            ).map_err(err)?;
            let rows=tx.prepare(&(filtered_rows_sql(q)+" LIMIT 12 OFFSET ?3")).map_err(err)?.query_map(params![q.kind,search,i64::from(q.page)*12],change_row).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;
            Ok(ReferenceChangesPage {rows,total,searched,significant,counts,leader,comparable,partial_fallback:q.kind=="significant"&&total>significant})
        })
    }
    /// Print all matching rows from one snapshot without sending an unbounded IPC response.
    pub fn reference_changes_print(
        &self,
        q: &ReferenceChangesRequest,
        cancelled: Arc<AtomicBool>,
        mut send: impl FnMut(Vec<ReferenceChangeRow>) -> Result<(), String>,
    ) -> Result<Vec<String>, String> {
        const BATCH_SIZE: usize = 128;
        if cancelled.load(Ordering::Acquire) {
            return Err("Печать отменена.".into());
        }
        self.reference_scope(q, |scope| {
            let mut conn = self.connect()?;
            let stop = Arc::clone(&cancelled);
            conn.progress_handler(1000, Some(move || stop.load(Ordering::Acquire)));
            let tx = conn.transaction().map_err(err)?;
            prepare_reference(&tx, q, scope)?;
            let mut stmt = tx.prepare(&filtered_rows_sql(q)).map_err(err)?;
            let mut rows = stmt.query(params![q.kind, unified_analytics::normalized(&q.search)]).map_err(err)?;
            let mut batch = Vec::with_capacity(BATCH_SIZE);
            while let Some(row) = rows.next().map_err(err)? {
                if cancelled.load(Ordering::Acquire) {
                    return Err("Печать отменена.".into());
                }
                batch.push(change_row(row).map_err(err)?);
                if batch.len() == BATCH_SIZE {
                    send(std::mem::replace(&mut batch, Vec::with_capacity(BATCH_SIZE)))?;
                }
            }
            if !batch.is_empty() {
                send(batch)?;
            }
            comparable_organizations(&tx, scope)
        })
    }

    pub fn reference_changes_csv(
        &self,
        q: &ReferenceChangesRequest,
        destination: &Path,
    ) -> Result<(), String> {
        if !destination.is_absolute()
            || destination.as_os_str().len() > 32767
            || !destination
                .extension()
                .and_then(|v| v.to_str())
                .is_some_and(|v| v.eq_ignore_ascii_case("csv"))
        {
            return Err("Choose an absolute CSV file path".into());
        }
        self.reference_scope(q,|scope| {
            let mut conn=self.connect()?;let tx=conn.transaction().map_err(err)?;
            prepare_reference(&tx,q,scope)?;
            export(destination,|writer| {
                comparison_csv_context(writer,&tx,q,scope,"Анализируемый период")?;
                csv_record(writer,&[Cell::Text("Качество данных"),Cell::Text("Изменения в таблице рассчитываются по доступным отчётам, если данные есть за оба интервала. Пропуски не считаются нулём.")])?;
                csv_record(writer,&[Cell::Text("Показатель строк"),Cell::Text(if q.metric=="amount"{"Сумма"}else{"Количество / объём"})])?;
                csv_record(writer,&[Cell::Text("Фильтр позиций"),Cell::Text(&q.kind)])?;
                csv_record(writer,&[Cell::Text("Поиск работ"),Cell::Text(&q.search)])?;
                csv_record(writer,&[Cell::Text("Сортировка"),Cell::Text(&q.sort)])?;
                csv_record(writer,&[Cell::Text("Порог изменения, %"),Cell::Number(Some(q.percent))])?;
                csv_record(writer,&[Cell::Text("Минимальное количество / объём"),Cell::Number(Some(q.min_count))])?;
                csv_record(writer,&[Cell::Text("Минимальная сумма"),Cell::Number(Some(q.min_amount))])?;
                csv_record(writer,&[])?;
                csv_record(writer,&[Cell::Text("Работа"),Cell::Text("Подразделение"),Cell::Text("Было"),Cell::Text("Стало"),Cell::Text("Изменение"),Cell::Text("%")])?;
                let mut stmt=tx.prepare(&filtered_rows_sql(q)).map_err(err)?;
                let mut rows=stmt.query(params![q.kind,unified_analytics::normalized(&q.search)]).map_err(err)?;
                while let Some(row)=rows.next().map_err(err)? {
                    let value=change_row(row).map_err(err)?;
                    csv_record(writer,&[Cell::Text(&value.name),Cell::Text(&value.org),Cell::Number(value.before),Cell::Number(value.after),Cell::Number(value.delta),Cell::Number(value.percent)])?;
                }
                Ok(())
            })
        })
    }
    pub fn reference_summary_csv(
        &self,
        q: &ReferenceChangesRequest,
        destination: &Path,
    ) -> Result<(), String> {
        if !destination.is_absolute()
            || destination.as_os_str().len() > 32767
            || !destination
                .extension()
                .and_then(|v| v.to_str())
                .is_some_and(|v| v.eq_ignore_ascii_case("csv"))
        {
            return Err("Choose an absolute CSV file path".into());
        }
        self.reference_scope(q,|scope| {
            let mut conn=self.connect()?;let tx=conn.transaction().map_err(err)?;
            prepare_reference_values(&tx,q,scope)?;
            export(destination,|writer| {
                comparison_csv_context(writer,&tx,q,scope,"Сводка доступных данных")?;
                csv_record(writer,&[Cell::Text("Наблюдаемые итоги анализируемого интервала"),Cell::Text("Все выбранные подразделения")])?;
                csv_record(writer,&[Cell::Text("Подразделение"),Cell::Text("Сумма"),Cell::Text("Количество / объём"),Cell::Text("Доля суммы, %"),Cell::Text("Сумма / количество (объём)"),Cell::Text("Состояние")])?;
                let mut statement=tx.prepare("WITH observed AS (
                    SELECT v.org,count(DISTINCT v.report_id) reports,coalesce(sum(x.count),0.0) count,CASE WHEN COUNT(x.row_index)=0 THEN 0 WHEN COUNT(x.amount)=COUNT(x.row_index) THEN SUM(x.amount) END amount,SUM(x.amount) known_amount
                    FROM an_valid v JOIN an_periods p ON p.slot=2 AND p.start=v.start
                    LEFT JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org GROUP BY v.org)
                    SELECT f.item,coalesce(o.reports,0),o.count,o.amount,
                    NOT EXISTS(SELECT 1 FROM an_missing m WHERE m.slot=2 AND m.org=f.item),
                    (SELECT CASE WHEN COUNT(amount)=COUNT(*) THEN SUM(amount) END FROM observed),o.known_amount
                    FROM analysis_filter_items f LEFT JOIN observed o ON o.org=f.item WHERE f.id=?1
                    ORDER BY (coalesce(o.amount,o.known_amount) IS NULL),coalesce(o.amount,o.known_amount) DESC,f.item COLLATE classic_ru,f.item").map_err(err)?;
                let mut rows=statement.query([&scope.organizations]).map_err(err)?;
                while let Some(row)=rows.next().map_err(err)? {
                    let org:String=row.get(0).map_err(err)?;let available:i64=row.get(1).map_err(err)?;
                    let strict_amount:Option<f64>=row.get(3).map_err(err)?;
                    let totals=observed_totals(row.get::<_,Option<f64>>(2).map_err(err)?.unwrap_or(0.),
                        strict_amount,row.get(6).map_err(err)?,available>0)?;
                    let total=row.get::<_,Option<f64>>(5).map_err(err)?.map(finite).transpose()?;
                    let share=match (totals.amount,total) {(Some(a),Some(t)) if t>0.=>Some(finite(a/t*100.)?),_=>None};
                    let complete:bool=row.get(4).map_err(err)?;
                    let state=if available==0 {"нет данных"} else if strict_amount.is_none() {"частичные данные, есть строки без суммы"} else if complete {"полные периоды"} else {"частичные данные"};
                    csv_record(writer,&[Cell::Text(&org),Cell::Number(totals.amount),Cell::Number(totals.count),Cell::Number(share),Cell::Number(totals.average),Cell::Text(state)])?;
                }
                Ok(())
            })
        })
    }
}
#[cfg(test)]
#[path = "reference_changes_tests.rs"]
mod tests;
