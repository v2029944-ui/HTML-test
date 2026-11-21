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
        "average" => Ok("Сумма / количество (объём)"),
        _ => Err("Unknown metric".into()),
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
fn context(
    w: &mut impl Write,
    tx: &Connection,
    s: &AnalysisScope,
    q: &ChangeQuery,
    table: &DetailCsvOptions,
) -> Result<(), String> {
    headers(w, &["Показатель", measure_label(&table.metric)?])?;
    headers(w, &["Тип исходных отчётов", &s.report_type])?;
    headers(w, &["Анализируемый период", &q.from, &q.to])?;
    if let Some(base) = &table.before {
        headers(w, &["Базовый период", &base.from, &base.to])?;
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
pub(super) fn export_patterns(
    tx: &Connection,
    path: &Path,
    table: &DetailCsvOptions,
    s: &AnalysisScope,
    q: &ChangeQuery,
    term: &str,
    order: &str,
    used: i64,
    previous: i64,
    base_from: &str,
    base_to: &str,
) -> Result<(), String> {
    export(path, |w| {
        context(w, tx, s, q, table)?;
        headers(w, &["Базовый период", base_from, base_to])?;
        let opts = table.options.as_ref().ok_or("Missing pattern options")?;
        headers(w, &["Поиск", &opts.search])?;
        headers(w, &["Фильтр признака", &opts.status])?;
        let usual = table.section == "usual";
        if usual {
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
            let name = entry["name"].as_str().ok_or("Missing pattern name")?;
            if usual {
                let m = &entry[&table.metric];
                csv_record(
                    w,
                    &[
                        Text(name),
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
                        Text(name),
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
            self.unified_patterns_output(
                rev,
                s,
                q,
                grouping,
                &table.section,
                table.window.ok_or("Missing pattern window")?,
                opts,
                None,
                Some((path, table)),
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
        let mut connection = self.connect()?;
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
                if !b || !a {
                    return Err("Contribution export requires both complete intervals".into());
                }
                write_contributions(w, &tx, &table.metric)
            } else {
                write_comparison(w, &tx, s, &table.metric, b, a)
            }
        })
    }
}
fn write_structure(w: &mut impl Write, page: &Value) -> Result<(), String> {
    if page["currentComplete"] != true {
        return Err("Incomplete current interval".into());
    }
    let complete = page["baseComplete"] == true;
    let (head, keys): (&[&str], &[&str]) = if complete {
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
        let mut record = vec![Text(row["name"].as_str().ok_or("Missing structure name")?)];
        record.extend(keys.iter().map(|key| Number(row[*key].as_f64())));
        csv_record(w, &record)?;
    }
    Ok(())
}
fn write_contributions(w: &mut impl Write, tx: &Connection, metric: &str) -> Result<(), String> {
    let (before, after) = match metric {
        "count" => ("bc", "cc"),
        "amount" => ("ba", "ca"),
        _ => return Err("Select count or amount for contributions".into()),
    };
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
    let mut stmt=tx.prepare(&format!("SELECT name,{before},{after} FROM an_comparison WHERE {before}<>{after} ORDER BY abs({after}-{before}) DESC,key")).map_err(err)?;
    let mut rows = stmt.query([]).map_err(err)?;
    while let Some(r) = rows.next().map_err(err)? {
        let name: String = r.get(0).map_err(err)?;
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
    if !b && !a {
        return Err("Incomplete comparison intervals".into());
    }
    if b && a {
        headers(
            w,
            &[
                "Работа / группа",
                "Значение раньше",
                "Значение сейчас",
                "Разница",
                "Изменение, %",
            ],
        )?;
    } else {
        headers(
            w,
            &[
                "Работа / группа",
                if b {
                    "Значение раньше"
                } else {
                    "Значение сейчас"
                },
            ],
        )?;
    }
    let mut emit =
        |name: &str, base: &AnalysisTotals, current: &AnalysisTotals| -> Result<(), String> {
            let old = value(base, metric);
            let new = value(current, metric);
            if b && a {
                let m = average_change(old, new, 0.)?;
                csv_record(
                    w,
                    &[
                        Text(name),
                        Number(old),
                        Number(new),
                        Number(m.delta),
                        Number(m.percent),
                    ],
                )
            } else {
                csv_record(w, &[Text(name), Number(if b { old } else { new })])
            }
        };
    emit(
        "Итого по выбранным работам",
        &range_summary(tx, s, 1)?.total,
        &range_summary(tx, s, 2)?.total,
    )?;
    let mut stmt = tx
        .prepare("SELECT name,bc,ba,cc,ca FROM an_comparison ORDER BY key")
        .map_err(err)?;
    let mut rows = stmt.query([]).map_err(err)?;
    while let Some(r) = rows.next().map_err(err)? {
        let name: String = r.get(0).map_err(err)?;
        let before = totals(r.get(1).map_err(err)?, r.get(2).map_err(err)?, b)?;
        let after = totals(r.get(3).map_err(err)?, r.get(4).map_err(err)?, a)?;
        if value(&before, metric).is_some() || value(&after, metric).is_some() {
            emit(&name, &before, &after)?;
        }
    }
    Ok(())
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
    let mut stmt=tx.prepare("SELECT f.item,NOT EXISTS(SELECT 1 FROM an_missing m WHERE m.slot=1 AND m.org=f.item),coalesce(sum(v.count),0),coalesce(sum(v.amount),0) FROM analysis_filter_items f LEFT JOIN an_values v ON v.slot=1 AND v.org=f.item WHERE f.id=?1 GROUP BY f.ordinal,f.item ORDER BY f.ordinal").map_err(err)?;
    let mut rows = stmt.query([&s.organizations]).map_err(err)?;
    let mut orgs = Vec::new();
    let mut complete = Vec::new();
    let mut summary = Vec::new();
    while let Some(r) = rows.next().map_err(err)? {
        orgs.push(r.get::<_, String>(0).map_err(err)?);
        let ok = r.get(1).map_err(err)?;
        complete.push(ok);
        summary.push(value(
            &totals(r.get(2).map_err(err)?, r.get(3).map_err(err)?, ok)?,
            &table.metric,
        ));
    }
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
    let mut emit = |name: &str, values: &[Option<f64>]| -> Result<(), String> {
        let mut line = vec![Text(name)];
        for (i, n) in values.iter().enumerate() {
            line.push(Number(*n));
            if i != base_index {
                let m = average_change(values[base_index], *n, 0.)?;
                line.push(Number(m.delta));
                line.push(Number(m.percent));
            }
        }
        csv_record(w, &line)
    };
    emit("Итого по выбранным работам", &summary)?;
    unified_analytics::presentation_functions(tx)?;
    unified_analytics::create_name_catalog(tx)?;
    // The explicit subset keeps its selection order. All names use the same
    // catalog, collation and tie-break as the paginated organization table.
    let names_sql = if s.names.is_some() {
        "SELECT item FROM analysis_filter_items WHERE id=?1 ORDER BY ordinal"
    } else {
        "SELECT name FROM unified_names WHERE ?1 IS NULL ORDER BY name COLLATE classic_ru,key"
    };
    let mut names = tx.prepare(names_sql).map_err(err)?;
    let mut names = names.query([&s.names]).map_err(err)?;
    tx.execute(
        "CREATE INDEX an_values_name_org ON an_values(slot,name,org)",
        [],
    )
    .map_err(err)?;
    let mut amounts=tx.prepare("SELECT f.ordinal,coalesce(sum(v.count),0),coalesce(sum(v.amount),0) FROM analysis_filter_items f LEFT JOIN an_values v ON v.slot=1 AND v.org=f.item AND v.name=?2 WHERE f.id=?1 GROUP BY f.ordinal ORDER BY f.ordinal").map_err(err)?;
    while let Some(row) = names.next().map_err(err)? {
        let name: String = row.get(0).map_err(err)?;
        let mut cells = Vec::with_capacity(orgs.len());
        let mut values = amounts.query(params![s.organizations, name]).map_err(err)?;
        while let Some(r) = values.next().map_err(err)? {
            let ok = *complete.get(cells.len()).ok_or("Unexpected organization")?;
            cells.push(value(
                &totals(r.get(1).map_err(err)?, r.get(2).map_err(err)?, ok)?,
                &table.metric,
            ));
        }
        if cells.len() != orgs.len() {
            return Err("Organization count changed".into());
        }
        emit(&name, &cells)?;
    }
    Ok(())
}
