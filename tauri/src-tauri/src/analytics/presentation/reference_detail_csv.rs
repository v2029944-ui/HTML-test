//! Filtered detail tables are calculated in SQLite and written one row at a time.
use super::*;
use Cell::{Number, Text};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DetailCsvOptions {
    section: String,
    metric: String,
    before: Option<RangeQuery>,
    base_org: Option<String>,
    limit: Option<usize>,
    window: Option<i32>,
    options: Option<UnifiedPatternOptions>,
}

fn headers(w: &mut impl Write, labels: &[&str]) -> Result<(), String> {
    csv_record(w, &labels.iter().map(|v| Text(v)).collect::<Vec<_>>())
}
fn measure_label(m: &str) -> Result<&'static str, String> {
    match m {
        "count" => Ok("Количество / объём"),
        "amount" => Ok("Сумма"),
        "average" => Ok("Средняя сумма на единицу"),
        _ => Err("Unknown metric".into()),
    }
}
fn work_label(name: &str, kind: &str) -> String {
    if kind == "group" {
        format!("{name} (группа)")
    } else {
        name.to_owned()
    }
}
fn value(t: &AnalysisTotals, m: &str) -> Option<f64> {
    match m {
        "count" => t.count,
        "amount" => t.amount,
        "average" => t.average,
        _ => None,
    }
}
fn period_header(w: &mut impl Write, label: &str, kind: &str, from: &str, last: &str) -> Result<(), String> {
    let start = date(last)?;
    let through = if kind == "year" {
        NaiveDate::from_ymd_opt(start.year(), 12, 31).ok_or("Invalid year")?
    } else {
        end(kind, start)?
    };
    headers(w, &[label, from, &through.to_string()])
}
fn context(
    w: &mut impl Write,
    tx: &Connection,
    s: &AnalysisScope,
    q: &ChangeQuery,
    table: &DetailCsvOptions,
) -> Result<(), String> {
    headers(w, &["Показатель", measure_label(&table.metric)?])?;
    headers(w, &["Тип исходных отчётов", &s.report_type])?;
    period_header(w, "Анализируемый период", &s.report_type, &q.from, &q.to)?;
    if let Some(base) = &table.before {
        period_header(w, "Базовый период", &s.report_type, &base.from, &base.to)?;
    }
    for (label, filter) in [
        ("Подразделения", Some(&s.organizations)),
        ("Работы / группы", s.names.as_ref()),
    ] {
        if let Some(id) = filter {
            let values:String=tx.query_row("SELECT coalesce(group_concat(item,', '),'') FROM (SELECT item FROM analysis_filter_items WHERE id=?1 ORDER BY ordinal)",[id],|r|r.get(0)).map_err(err)?;
            headers(w, &[label, &values])?;
        }
    }
    if s.names.is_none(){headers(w,&["Работы / группы","Все работы"])?;}
    Ok(())
}
fn status(code: &str) -> &str {
    match code {
        "normal" => "Обычный уровень",
        "high" => "Необычно выше",
        "low" => "Необычно ниже",
        "insufficient" => "Недостаточно данных",
        "frequent" => "Часто",
        "rare" => "Редко",
        "rarer" => "Заметно реже",
        "disappeared" => "Перестала встречаться",
        "new" => "Впервые в загруженной истории",
        "dormant" => "Долго без выполнений",
        "occasional" => "Встречается",
        "absent" => "Нет выполнений",
        "uncertain" => "Есть пропуски",
        "unknown" => "Нет сопоставимых отчётов",
        _ => code,
    }
}
// The pattern calculator has already validated the query, generated and sorted the records.
// Read directly from its temporary table, including rows beyond the first screen.
fn export_patterns(
    tx: &Connection,
    path: &Path,
    table: &DetailCsvOptions,
    s: &AnalysisScope,
    q: &ChangeQuery,
    rows: PatternExport<'_>,
) -> Result<(), String> {
    let PatternExport {
        term,
        order,
        used,
        previous,
        current_complete,
        current_amount_incomplete,
        base_kind,
        base_from,
        base_to,
    } = rows;
    export(path, |w| {
        context(w, tx, s, q, table)?;
        let opts = table.options.as_ref().ok_or("Missing pattern options")?;
        if let Some(base) = &opts.baseline {
            period_header(w, "Базовый период", &s.report_type, &base.from, &base.to)?;
        } else {
            period_header(w, "Базовый период", base_kind, base_from, base_to)?;
        }
        headers(w, &["Поиск", &opts.search])?;
        headers(w, &["Фильтр признака", &opts.status])?;
        let usual = table.section == "usual";
        if let Some(window) = table.window {
            if usual || opts.baseline.is_none() {
                headers(w, &["Запрошено исторических периодов", &window.to_string()])?;
            }
            if !usual {
                headers(w, &["Порог отсутствия, периодов", &window.to_string()])?;
            }
        }
        if usual {
            let observed_windows = previous.to_string();
            headers(w, &["Исторических окон с отчётами", &observed_windows])?;
            let mut notices = Vec::new();
            if !current_complete {
                notices.push("Текущий интервал неполон. Показаны доступные значения. Отклонение рассчитывается при достаточной истории по показателю.");
            }
            if current_amount_incomplete && table.metric != "count" {
                notices.push("В некоторых строках сумма не указана. Показана сумма заполненных ячеек.");
            }
            if !notices.is_empty() {
                headers(w, &["Качество данных", &notices.join(" ")])?;
            }
            headers(
                w,
                &[
                    "Услуга / группа",
                    "Среднее раньше",
                    "Сейчас",
                    "Отклонение",
                    "Отклонение, %",
                    "Оценка",
                ],
            )?;
        } else {
            let current_with_reports = used.to_string();
            let base_with_reports = previous.to_string();
            headers(w, &["Периодов с отчётами сейчас", &current_with_reports])?;
            headers(w, &["Периодов с отчётами раньше", &base_with_reports])?;
            if !current_complete {
                headers(w, &["Качество данных", "Текущий интервал неполон. Частота рассчитана по периодам с доступными отчётами."])?;
            }
            headers(
                w,
                &[
                    "Услуга / группа",
                    "Была в периодах",
                    "Частота сейчас, %",
                    "Была раньше",
                    "Частота раньше, %",
                    "Разница частоты, п. п.",
                    "Впервые в истории",
                    "Последний раз",
                    "Подряд без активности",
                    "Что это значит",
                ],
            )?;
        }
        let mut stmt = tx
            .prepare(&format!(
                "SELECT data FROM unified_patterns WHERE ?1 IS NOT NULL ORDER BY {order}"
            ))
            .map_err(err)?;
        let mut rows = stmt.query([term]).map_err(err)?;
        while let Some(row) = rows.next().map_err(err)? {
            let entry: Value =
                serde_json::from_str(&row.get::<_, String>(0).map_err(err)?).map_err(err)?;
            let name = work_label(
                entry["name"].as_str().ok_or("Missing pattern name")?,
                entry["kind"].as_str().unwrap_or(""),
            );
            if usual {
                let m = &entry[&table.metric];
                csv_record(
                    w,
                    &[
                        Text(&name),
                        Number(m["base"].as_f64()),
                        Number(m["current"].as_f64()),
                        Number(m["delta"].as_f64()),
                        Number(m["percent"].as_f64()),
                        Text(status(
                            m["status"].as_str().ok_or("Missing pattern status")?,
                        )),
                    ],
                )?;
            } else {
                let a = entry["frequency"].as_f64();
                let b = entry["previousFrequency"].as_f64();
                let statuses = entry["statuses"]
                    .as_array()
                    .ok_or("Missing frequency statuses")?
                    .iter()
                    .filter_map(Value::as_str)
                    .map(status)
                    .collect::<Vec<_>>()
                    .join("; ");
                csv_record(
                    w,
                    &[
                        Text(&name),
                        Number(entry["hits"].as_f64().filter(|_| used > 0)),
                        Number(a),
                        Number(entry["previousHits"].as_f64().filter(|_| previous > 0)),
                        Number(b),
                        Number(a.zip(b).map(|(a, b)| a - b)),
                        Text(entry["first"].as_str().unwrap_or("")),
                        Text(entry["last"].as_str().unwrap_or("")),
                        Number(entry["trailing"].as_f64()),
                        Text(&statuses),
                    ],
                )?;
            }
        }
        Ok(())
    })
}

impl Analytics<'_> {
    pub(super) fn reference_detail_csv(
        &self,
        rev: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        grouping: Option<&str>,
        table: &DetailCsvOptions,
        path: &Path,
    ) -> Result<(), String> {
        if !path.is_absolute()
            || path.as_os_str().len() > 32767
            || !path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("csv"))
        {
            return Err("Choose an absolute CSV file path".into());
        }
        measure_label(&table.metric)?;
        if matches!(table.section.as_str(), "usual" | "regularity") {
            let opts = table.options.as_ref().ok_or("Missing pattern options")?;
            self.export_pattern_rows(
                rev,
                s,
                q,
                grouping,
                &table.section,
                table.window.ok_or("Missing pattern window")?,
                opts,
                &|tx, rows| export_patterns(tx, path, table, s, q, rows),
            )?;
            return Ok(());
        }
        // Structure is bounded by the user-selected top 10/20 and a single remainder row.
        let structure = if table.section == "structure" {
            Some(self.advanced_composition_limit(
                rev,
                s,
                q,
                grouping,
                table.before.as_ref().ok_or("Missing comparison period")?,
                "structure",
                &table.metric,
                None,
                table.limit.ok_or("Missing structure limit")?,
            )?)
        } else {
            None
        };
        let mut connection = self.database.connect()?;
        let tx = connection.transaction().map_err(err)?;
        check_revision(&tx, rev)?;
        if let Some(page) = structure {
            return export(path, |w| {
                context(w, &tx, s, q, table)?;
                write_structure(w, &page)
            });
        }
        workspace(&tx, rev, s)?;
        let current = RangeQuery {
            from: q.from.clone(),
            to: q.to.clone(),
        };
        if table.section == "organizations" {
            fill_range(&tx, s, &current, 1)?;
            return export(path, |w| {
                context(w, &tx, s, q, table)?;
                write_organizations(w, &tx, s, table)
            });
        }
        if !matches!(table.section.as_str(), "comparison" | "contributions") {
            return Err("Unknown export table".into());
        }
        let comparison_query = ComparisonQuery {
            before: table.before.clone().ok_or("Missing comparison period")?,
            after: current,
            percent: q.percent,
            min_count: q.min_count,
            min_amount: q.min_amount,
        };
        let (b, a) = comparison(&tx, s, &comparison_query, None)?;
        export(path, |w| {
            context(w, &tx, s, q, table)?;
            if table.section == "contributions" {
                write_contributions(w, &tx, &table.metric, b && a)
            } else {
                write_comparison(w, &tx, s, &table.metric, b, a)
            }
        })
    }
}

impl Storage {
    pub fn reference_detail_csv(
        &self,
        rev: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        grouping: Option<&str>,
        table: &DetailCsvOptions,
        path: &Path,
    ) -> Result<(), String> {
        self.analysis().reference_detail_csv(rev, s, q, grouping, table, path)
    }
}
fn write_structure(w: &mut impl Write, page: &Value) -> Result<(), String> {
    if page["currentComplete"] != true && page["currentAvailable"] != true {
        return Err("Incomplete current interval".into());
    }
    let base_available = page["baseAvailable"] == true;
    let (head, keys): (&[&str], &[&str]) = if base_available {
        (
            &[
                "Услуга / группа",
                "Значение раньше",
                "Значение сейчас",
                "Доля раньше, %",
                "Доля сейчас, %",
                "Накопленная доля, %",
                "Изменение доли, п. п.",
            ],
            &[
                "base",
                "current",
                "baseShare",
                "share",
                "cumulative",
                "shareChange",
            ],
        )
    } else {
        (
            &[
                "Услуга / группа",
                "Значение сейчас",
                "Доля сейчас, %",
                "Накопленная доля, %",
            ],
            &["current", "share", "cumulative"],
        )
    };
    headers(w, head)?;
    for row in page["rows"].as_array().ok_or("Missing structure rows")? {
        if row["current"].as_f64().is_none_or(|n| n == 0.) {
            continue;
        }
        let name = work_label(
            row["name"].as_str().ok_or("Missing structure name")?,
            row["kind"].as_str().unwrap_or(""),
        );
        let mut record = vec![Text(&name)];
        record.extend(keys.iter().map(|key| Number(row[*key].as_f64())));
        csv_record(w, &record)?;
    }
    if page["currentComplete"] != true || (base_available && page["baseComplete"] != true) {
        let note = if page["sharesValid"] == true {
            "Частичные значения. Доли рассчитаны от доступного итога."
        } else {
            "Частичные значения. Доли недоступны."
        };
        csv_record(w, &[Text("Качество данных"), Text(note)])?;
    }
    Ok(())
}
fn write_contributions(w: &mut impl Write, tx: &Connection, metric: &str, complete: bool) -> Result<(), String> {
    use super::advanced_analysis::{observed_contribution_delta, observed_contribution_metrics, observed_contribution_rows};
    let (before, after) = match metric {
        "count" => ("bc", "cc"),
        "amount" => ("ba", "ca"),
        _ => return Err("Select count or amount for contributions".into()),
    };
    let incomplete_amount = metric == "amount" && tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM an_comparison WHERE ba IS NULL OR ca IS NULL)",
        [], |r| r.get::<_,bool>(0),
    ).map_err(err)?;
    let both_observed = observed_reports(tx, 1, None)? && observed_reports(tx, 2, None)?;
    if !complete || incomplete_amount {
        let note = if both_observed && incomplete_amount {
            "Показаны доступные значения. Вклады рассчитаны по работам с известной суммой в обоих интервалах."
        } else if both_observed {
            "Показаны доступные значения. Изменение и вклад рассчитаны по доступным отчётам."
        } else {
            "Показаны доступные значения. Для расчёта изменения нужны отчёты в обоих интервалах."
        };
        headers(w, &["Качество данных", note])?;
    }
    headers(
        w,
        &[
            "Услуга / группа",
            "Значение раньше",
            "Значение сейчас",
            "Изменение значения",
            "Изменение, %",
            "Вклад в общее изменение, %",
        ],
    )?;
    if !both_observed || incomplete_amount {
        let source = observed_contribution_rows(tx, metric)?;
        let compared_delta = observed_contribution_delta(tx, &source)?;
        let mut stmt = tx.prepare(&format!("{source} ORDER BY key")).map_err(err)?;
        let mut rows = stmt.query([]).map_err(err)?;
        let mut row = rows.next().map_err(err)?.ok_or("No available contribution values")?;
        loop {
            let name = work_label(&row.get::<_, String>(1).map_err(err)?, &row.get::<_, String>(2).map_err(err)?);
            let base = row.get(3).map_err(err)?;
            let delta = row.get::<_, Option<f64>>(5).map_err(err)?.map(finite).transpose()?;
            let (percent, contribution) = observed_contribution_metrics(base, delta, compared_delta)?;
            csv_record(w, &[Text(&name), Number(base), Number(row.get(4).map_err(err)?),
                Number(delta), Number(percent), Number(contribution)])?;
            let Some(next) = rows.next().map_err(err)? else { break };
            row = next;
        }
        return Ok(());
    }
    let totals = tx
        .query_row(
            &format!(
                "SELECT coalesce(sum({before}),0),coalesce(sum({after}),0) FROM an_comparison"
            ),
            [],
            |r| Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?)),
        )
        .map_err(err)?;
    let total_delta = finite(totals.1 - totals.0)?;
    let mut stmt=tx.prepare(&format!("SELECT name,{before},{after},kind FROM an_comparison WHERE {before}<>{after} ORDER BY abs({after}-{before}) DESC,key")).map_err(err)?;
    let mut rows = stmt.query([]).map_err(err)?;
    while let Some(r) = rows.next().map_err(err)? {
        let name = work_label(&r.get::<_, String>(0).map_err(err)?, &r.get::<_, String>(3).map_err(err)?);
        let change = measure(r.get(1).map_err(err)?, r.get(2).map_err(err)?, 0., 0.)?;
        let share = if total_delta == 0. {
            None
        } else {
            Some(finite(change.delta / total_delta * 100.)?)
        };
        csv_record(
            w,
            &[
                Text(&name),
                Number(Some(change.base)),
                Number(Some(change.current)),
                Number(Some(change.delta)),
                Number(change.percent),
                Number(share),
            ],
        )?;
    }
    Ok(())
}
fn write_comparison(
    w: &mut impl Write,
    tx: &Connection,
    s: &AnalysisScope,
    metric: &str,
    b: bool,
    a: bool,
) -> Result<(), String> {
    let before = range_summary(tx, s, 1)?;
    let after = range_summary(tx, s, 2)?;
    let before_observed = before.available_reports > 0;
    let after_observed = after.available_reports > 0;
    if !before_observed && !after_observed {
        return Err("Incomplete comparison intervals".into());
    }
    let both_observed = before_observed && after_observed;
    if both_observed {
        headers(w, &["Работа / группа", "Значение раньше", "Значение сейчас", "Разница", "Изменение, %"])?;
    } else {
        headers(w, &["Работа / группа", if before_observed {"Значение раньше"} else {"Значение сейчас"}])?;
    }
    let mut emit = |name: &str, base: &AnalysisTotals, current: &AnalysisTotals,
                    comparable_before: Option<f64>, comparable_after: Option<f64>| -> Result<(), String> {
        let old = value(base, metric);
        let new = value(current, metric);
        if both_observed {
            let m = average_change(comparable_before, comparable_after, 0.)?;
            csv_record(w, &[Text(name), Number(old), Number(new), Number(m.delta), Number(m.percent)])
        } else {
            csv_record(w, &[Text(name), Number(if before_observed {old} else {new})])
        }
    };
    if value(&before.available_total,metric).is_some() || value(&after.available_total,metric).is_some() {
        let comparable_pair = if metric == "average" {
            let same_work = before.source_work.as_ref().zip(after.source_work.as_ref()).is_some_and(|(b, a)| b == a);
            comparable_averages(&before.available_total, &after.available_total, same_work)
        } else {
            value(&before.available_total, metric).zip(value(&after.available_total, metric))
        };
        emit("Итого по выбранным работам", &before.available_total, &after.available_total,
            comparable_pair.map(|(base, _)| base), comparable_pair.map(|(_, current)| current))?;
    }
    let mut stmt = tx.prepare("SELECT name,bc,ba,cc,ca,kind,b_known,a_known,b_paired,a_paired,same_source_work FROM an_comparison ORDER BY key").map_err(err)?;
    let mut rows = stmt.query([]).map_err(err)?;
    while let Some(r) = rows.next().map_err(err)? {
        let name = work_label(&r.get::<_, String>(0).map_err(err)?, &r.get::<_, String>(5).map_err(err)?);
        let bc = r.get(1).map_err(err)?;
        let ba = r.get(2).map_err(err)?;
        let ac = r.get(3).map_err(err)?;
        let aa = r.get(4).map_err(err)?;
        let base = paired_observed_totals(bc,ba,r.get(6).map_err(err)?,r.get(8).map_err(err)?,before_observed)?;
        let current = paired_observed_totals(ac,aa,r.get(7).map_err(err)?,r.get(9).map_err(err)?,after_observed)?;
        if value(&base, metric).is_some() || value(&current, metric).is_some() {
            let same_work = r.get::<_, Option<bool>>(10).map_err(err)?.unwrap_or(false);
            let (old, new) = if metric == "average" && same_work {
                (base.average, current.average)
            } else if metric == "average" {
                (value(&totals(bc,ba,before_observed)?,metric),
                 value(&totals(ac,aa,after_observed)?,metric))
            } else {
                (value(&base, metric), value(&current, metric))
            };
            emit(&name, &base, &current, old, new)?;
        }
    }
    if (before_observed && (!b || before.amount_missing_rows)) || (after_observed && (!a || after.amount_missing_rows)) {
        csv_record(w, &[Text("Качество данных"), Text("Частичные значения и изменения по доступным отчётам. Пропуски не считаются нулём.")])?;
    }
    Ok(())
}
#[derive(Serialize)]
pub struct OrganizationWorkValues {
    pub entry: UnifiedNameOption,
    pub rows: Vec<OrganizationSummary>,
}

fn ordered_organization_totals(
    tx: &Connection,
    s: &AnalysisScope,
) -> Result<Vec<OrganizationSummary>, String> {
    let mut stmt = tx.prepare(
        "SELECT f.item,(SELECT count(*) FROM an_missing m WHERE m.slot=1 AND m.org=f.item),
                coalesce(sum(v.count),0),CASE WHEN COUNT(v.key)=0 THEN 0 WHEN COUNT(v.amount)=COUNT(v.key) THEN SUM(v.amount) END,SUM(v.known_amount),CASE WHEN COUNT(v.source_work)=COUNT(v.paired_count) AND MIN(v.source_work)=MAX(v.source_work) THEN SUM(v.paired_count) END,CASE WHEN COUNT(v.source_work)=COUNT(v.paired_count) AND MIN(v.source_work)=MAX(v.source_work) THEN MIN(v.source_work) END
         FROM analysis_filter_items f LEFT JOIN an_values v ON v.slot=1 AND v.org=f.item
         WHERE f.id=?1 GROUP BY f.ordinal,f.item ORDER BY f.ordinal"
    ).map_err(err)?;
    let mut rows = stmt.query([&s.organizations]).map_err(err)?;
    let periods = count_sql(tx,"SELECT COUNT(*) FROM temp.an_periods WHERE slot=1")?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(err)? {
        let missing = row.get(1).map_err(err)?;
        let complete = missing == 0;
        let amount: Option<f64> = row.get(3).map_err(err)?;
        result.push(OrganizationSummary {
            org: row.get(0).map_err(err)?,
            average_source_work: row.get(6).map_err(err)?,
            complete,
            missing,
            amount_missing_rows: amount.is_none(),
            total: totals(row.get(2).map_err(err)?, amount, complete)?,
            available_total: paired_observed_totals(row.get(2).map_err(err)?, amount,
                row.get(4).map_err(err)?, row.get(5).map_err(err)?, missing < periods)?,
        });
    }
    Ok(result)
}

// The screen seeks a bounded page. The CSV streams the same keyed rows in the
// same order, preserving zero on covered days and unknown on incomplete ones.
fn visit_all_organization_works(
    tx: &Connection,
    s: &AnalysisScope,
    organizations: &[OrganizationSummary],
    page: Option<(usize, usize)>,
    mut visit: impl FnMut(OrganizationWorkValues) -> Result<(), String>,
) -> Result<i64, String> {
    analysis_presentation::presentation_functions(tx)?;
    create_name_catalog(tx, true)?;
    let total = count_sql(tx, "SELECT count(*) FROM unified_names")?;
    let (limit, offset) = match page {
        Some((index, size)) => (size as i64, crate::storage::direct_page_offset(index, size, total)?),
        None => (-1, 0),
    };
    let mut names = tx.prepare(
        "SELECT key,label,kind FROM unified_names ORDER BY name COLLATE classic_ru,key LIMIT ?1 OFFSET ?2"
    ).map_err(err)?;
    let mut names = names.query(params![limit, offset]).map_err(err)?;
    let mut amounts = tx.prepare(
        "SELECT coalesce(v.count,0),CASE WHEN v.key IS NULL THEN 0 ELSE v.amount END,v.known_amount,CASE WHEN v.source_work IS NOT NULL THEN v.paired_count END,v.source_work
         FROM analysis_filter_items f LEFT JOIN an_values v ON v.slot=1 AND v.org=f.item AND v.key=?2
         WHERE f.id=?1 ORDER BY f.ordinal"
    ).map_err(err)?;
    while let Some(name) = names.next().map_err(err)? {
        let entry = UnifiedNameOption {
            key: name.get(0).map_err(err)?,
            name: name.get(1).map_err(err)?,
            kind: name.get(2).map_err(err)?,
            ancestor_keys: Vec::new(),
            has_children: false,
        };
        let mut values = amounts.query(params![s.organizations, entry.key]).map_err(err)?;
        let mut rows = Vec::with_capacity(organizations.len());
        for org in organizations {
            let row = values.next().map_err(err)?.ok_or("Missing selected organization")?;
            let amount: Option<f64> = row.get(1).map_err(err)?;
            rows.push(OrganizationSummary {
                org: org.org.clone(),
                average_source_work: row.get(4).map_err(err)?,
                complete: org.complete,
                missing: org.missing,
                amount_missing_rows: amount.is_none(),
                total: totals(row.get(0).map_err(err)?, amount, org.complete)?,
                available_total: paired_observed_totals(row.get(0).map_err(err)?, amount,
                    row.get(2).map_err(err)?, row.get(3).map_err(err)?, org.available_total.count.is_some())?,
            });
        }
        visit(OrganizationWorkValues { entry, rows })?;
    }
    Ok(total)
}

impl Analytics<'_> {
    pub(super) fn organization_work_window(
        &self,
        revision: i64,
        scope: &AnalysisScope,
        query: &RangeQuery,
        index: usize,
        size: usize,
    ) -> Result<SqlPage<OrganizationWorkValues>, String> {
        if scope.names.is_some() || !(1..=50).contains(&size) {
            return Err("Select a page of 1..=50 rows from all works".into());
        }
        let mut conn = self.database.connect()?;
        let tx = conn.transaction().map_err(err)?;
        workspace(&tx, revision, scope)?;
        fill_range(&tx, scope, query, 1)?;
        let organizations = ordered_organization_totals(&tx, scope)?;
        let mut rows = Vec::new();
        let total_rows = visit_all_organization_works(&tx, scope, &organizations, Some((index, size)), |row| {
            rows.push(row);
            Ok(())
        })?;
        Ok(SqlPage { rows, total_rows })
    }
}

impl Storage {
    pub fn organization_work_window(
        &self,
        revision: i64,
        scope: &AnalysisScope,
        query: &RangeQuery,
        index: usize,
        size: usize,
    ) -> Result<SqlPage<OrganizationWorkValues>, String> {
        self.analysis().organization_work_window(revision, scope, query, index, size)
    }
}

fn write_organizations(
    w: &mut impl Write,
    tx: &Connection,
    s: &AnalysisScope,
    table: &DetailCsvOptions,
) -> Result<(), String> {
    let base = table
        .base_org
        .as_deref()
        .ok_or("Missing baseline organization")?;
    let organizations = ordered_organization_totals(tx, s)?;
    let orgs: Vec<_> = organizations.iter().map(|org| org.org.clone()).collect();
    let base_index = orgs
        .iter()
        .position(|o| o == base)
        .ok_or("Unknown baseline organization")?;
    let mut head = vec!["Работа / группа".to_owned()];
    for org in &orgs {
        head.push(org.clone());
        if org != base {
            head.push(format!("{org} · разница"));
            head.push(format!("{org} · %"));
        }
    }
    headers(w, &head.iter().map(String::as_str).collect::<Vec<_>>())?;
    let mut emit = |name: &str, values: &[OrganizationSummary]| -> Result<(), String> {
        let baseline = &values[base_index];
        let base = &baseline.available_total;
        let mut line = vec![Text(name)];
        for (i, row) in values.iter().enumerate() {
            line.push(Number(value(&row.available_total, &table.metric)));
            if i != base_index {
                let current = &row.available_total;
                let pair = if table.metric == "average" {
                    let same_work = baseline.average_source_work.as_ref()
                        .zip(row.average_source_work.as_ref()).is_some_and(|(b, a)| b == a);
                    comparable_averages(base, current, same_work)
                } else {
                    value(base, &table.metric).zip(value(current, &table.metric))
                };
                let (b, a) = pair.map_or((None, None), |(b, a)| (Some(b), Some(a)));
                let m = average_change(b, a, 0.)?;
                line.push(Number(m.delta));
                line.push(Number(m.percent));
            }
        }
        csv_record(w, &line)
    };
    emit("Итого по выбранным работам", &organizations)?;
    if s.names.is_none() {
        visit_all_organization_works(tx, s, &organizations, None, |item| emit(&item.entry.name, &item.rows))?;
    } else {
        let hierarchy = super::workspace::tree_indexed(tx)?;
        let mut names = tx.prepare(if hierarchy {
            "SELECT name,kind FROM an_scope_selections ORDER BY ordinal"
        } else {
            "SELECT f.item, CASE WHEN g.key IS NULL THEN 'work' ELSE 'group' END
             FROM analysis_filter_items f LEFT JOIN analysis_group_names g ON g.name=f.item
             WHERE f.id=?1 ORDER BY f.ordinal"
        }).map_err(err)?;
        let filter = [&s.names];
        let mut names = names.query(rusqlite::params_from_iter(if hierarchy { &filter[..0] } else { &filter }))
            .map_err(err)?;
        tx.execute(
            "CREATE INDEX an_values_name_org ON an_values(slot,name,org)",
            [],
        )
        .map_err(err)?;
        let mut amounts=tx.prepare("SELECT f.ordinal,coalesce(sum(v.count),0),CASE WHEN COUNT(v.key)=0 THEN 0 WHEN COUNT(v.amount)=COUNT(v.key) THEN SUM(v.amount) END,SUM(v.known_amount),CASE WHEN COUNT(v.source_work)=COUNT(v.paired_count) AND MIN(v.source_work)=MAX(v.source_work) THEN SUM(v.paired_count) END,CASE WHEN COUNT(v.source_work)=COUNT(v.paired_count) AND MIN(v.source_work)=MAX(v.source_work) THEN MIN(v.source_work) END FROM analysis_filter_items f LEFT JOIN an_values v ON v.slot=1 AND v.org=f.item AND v.name=?2 WHERE f.id=?1 GROUP BY f.ordinal ORDER BY f.ordinal").map_err(err)?;
        while let Some(row) = names.next().map_err(err)? {
            let name: String = row.get(0).map_err(err)?;
            let mut cells = Vec::with_capacity(orgs.len());
            let mut values = amounts.query(params![s.organizations, name]).map_err(err)?;
            while let Some(r) = values.next().map_err(err)? {
                let org = organizations.get(cells.len()).ok_or("Unexpected organization")?;
                let count = r.get(1).map_err(err)?;
                let amount: Option<f64> = r.get(2).map_err(err)?;
                cells.push(OrganizationSummary {
                    org: org.org.clone(), complete: org.complete, missing: org.missing,
                    average_source_work: r.get(5).map_err(err)?,
                    amount_missing_rows: amount.is_none(),
                    total: totals(count, amount, org.complete)?,
                    available_total: paired_observed_totals(count, amount, r.get(3).map_err(err)?, r.get(4).map_err(err)?, org.available_total.count.is_some())?,
                });
            }
            if cells.len() != orgs.len() {
                return Err("Organization count changed".into());
            }
            let kind: String = row.get(1).map_err(err)?;
            emit(&work_label(&name, &kind), &cells)?;
        }
    }
    for org in &organizations {
        let mut status = Vec::new();
        if org.missing > 0 {
            status.push(format!("Пропусков: {}", org.missing));
        }
        if org.amount_missing_rows {
            status.push("Есть строки без суммы".into());
        }
        if !status.is_empty() {
            csv_record(w, &[Text("Качество данных"), Text(&org.org), Text(&status.join(". "))])?;
        }
    }
    Ok(())
}
