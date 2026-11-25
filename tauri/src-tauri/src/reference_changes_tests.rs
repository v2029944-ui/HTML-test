use super::*;
use tempfile::tempdir;
fn request(revision: i64) -> ReferenceChangesRequest {
    ReferenceChangesRequest {
        revision,
        report_type: "day".into(),
        from: "2024-01-10".into(),
        to: "2024-01-11".into(),
        base_from: "2024-01-01".into(),
        base_to: "2024-01-01".into(),
        organizations: vec!["A".into(), "B".into(), "Empty".into()],
        metric: "amount".into(),
        percent: 30.,
        min_count: 1.,
        min_amount: 10.,
        kind: "all".into(),
        search: String::new(),
        sort: "name".into(),
        page: 0,
    }
}
fn prepare(store: &Storage, reports: Vec<Value>) -> i64 {
    store.save(json!({"reports":reports,"settings":{"orgNames":["A","B","Empty","D"],"groups":[{"id":"group","name":"Combined","works":["Work","Other"]}],"catalog":[]}})).unwrap();
    let revision = store.revision().unwrap();
    let c = store.connect().unwrap();
    let ids = c
        .prepare("SELECT id FROM reports")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for id in ids {
        store.analysis_index_report(&id, revision).unwrap();
    }
    store.analysis_index_settings(revision).unwrap();
    revision
}
fn report(id: &str, start: &str, rows: Value) -> Value {
    json!({"id":id,"type":"day","start":start,"end":start,"organizations":["A","Empty"],"rows":rows})
}
fn row(org: &str, work: &str, count: f64, amount: f64) -> Value {
    json!({"organization":org,"work":work,"count":count,"amount":amount})
}

#[test]
fn partial_date_ranges_compare_loaded_reports_in_tables_and_csv() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let revision = prepare(&store, vec![
        report("before", "2023-01-01", json!([row("A", "Work", 2., 20.)])),
        report("after", "2024-01-01", json!([row("A", "Work", 3., 30.)])),
    ]);
    let mut q = request(revision);
    q.organizations = vec!["A".into()];
    q.base_from = "2023-01-01".into();
    q.from = "2024-01-01".into();
    for (label, before_end, after_end) in [
        ("custom", "2023-01-05", "2024-01-05"),
        ("week", "2023-01-07", "2024-01-07"),
        ("month", "2023-01-31", "2024-01-31"),
        ("year", "2023-12-31", "2024-12-31"),
    ] {
        q.base_to = before_end.into();
        q.to = after_end.into();
        let changes = store.reference_changes(&q).unwrap();
        assert!(changes.comparable.is_empty(), "{label}: neither interval has full coverage");
        let work = &changes.rows[0];
        assert_eq!((work.before, work.after, work.delta, work.percent), (Some(20.), Some(30.), Some(10.), Some(50.)), "{label}");
        let query = ComparisonQuery { before:q.baseline(), after:q.current(), percent:0., min_count:0., min_amount:0. };
        store.reference_scope(&q, |scope| {
            let dashboard = store.analysis_comparison_dashboard(revision, scope, &query)?;
            assert_eq!(dashboard.summary.amount.as_ref().map(|value| value.delta), Some(10.), "{label}");
            assert_eq!(dashboard.top_amount[0].amount.as_ref().map(|value| value.delta), Some(10.), "{label}");
            let window = store.analysis_comparison_window(revision, scope, &query, None, 0, 50)?;
            assert_eq!(window.rows[0].amount.as_ref().map(|value| value.delta), Some(10.), "{label}");
            let options = serde_json::from_value(json!({"section":"comparison","metric":"amount","before":{"from":q.base_from,"to":q.base_to}})).unwrap();
            let request = ChangeQuery { method:"range".into(), from:q.from.clone(), to:q.to.clone(), percent:0., min_count:0., min_amount:0. };
            let path = dir.path().join(format!("{label}.csv"));
            store.reference_detail_csv(revision, scope, &request, None, &options, &path)?;
            let csv = fs::read_to_string(path).unwrap();
            assert!(csv.contains(r#""Combined (группа)";"20";"30";"10";"50""#), "{csv}");
            assert!(csv.contains(r#""Итого по выбранным работам";"20";"30";"10";"50""#), "{csv}");
            assert!(csv.contains("Качество данных"), "{csv}");
            Ok(())
        }).unwrap();
    }
    q.from = "2024-01-02".into();
    q.to = q.from.clone();
    q.base_to = q.base_from.clone();
    let missing = store.reference_changes(&q).unwrap();
    assert_eq!((missing.rows[0].before, missing.rows[0].after, missing.rows[0].delta), (Some(20.), None, None));
}
#[test]
fn missing_amount_keeps_the_work_in_both_metric_views() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut partial = row("A", "Work", 7., 0.);
    partial["amount"] = Value::Null;
    let revision = prepare(&store, vec![
        report("base", "2024-01-01", json!([row("A", "Work", 2., 20.)])),
        report("current", "2024-01-10", json!([partial])),
    ]);
    let mut q = request(revision);
    q.to = q.from.clone();
    let amount = store.reference_changes(&q).unwrap();
    let work = &amount.rows[0];
    assert_eq!((work.before, work.after, work.delta), (Some(20.), None, None));
    assert!(amount.leader.is_none());
    assert_eq!(amount.significant, 0);

    q.metric = "count".into();
    let count = store.reference_changes(&q).unwrap();
    let work = &count.rows[0];
    assert_eq!((work.before, work.after, work.delta), (Some(2.), Some(7.), Some(5.)));
    assert_eq!(count.counts.up, 1);
}

#[test]
fn significant_filter_counts_partial_changes_and_keeps_confirmed_zero() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let revision = prepare(&store, vec![
        json!({"id":"base","type":"day","start":"2024-01-01","end":"2024-01-01","organizations":["A","B"],"rows":[row("A","Work",1.,10.),row("B","Work",2.,20.)]}),
        json!({"id":"a1","type":"day","start":"2024-01-10","end":"2024-01-10","organizations":["A","B"],"rows":[row("A","Work",3.,30.),row("B","Work",0.,0.)]}),
        report("a2", "2024-01-11", json!([row("A", "Work", 1., 10.)])),
    ]);
    let mut q = request(revision);
    q.kind = "significant".into();
    q.organizations = vec!["A".into(), "B".into()];
    let result = store.reference_changes(&q).unwrap();
    assert_eq!((result.total, result.significant, result.counts.up, result.counts.down), (2, 2, 1, 1));
    assert!(!result.partial_fallback);
    let partial = result.rows.iter().find(|row| row.org == "B").unwrap();
    assert_eq!((partial.before, partial.after, partial.delta, partial.percent), (Some(20.), Some(0.), Some(-20.), Some(-100.)));
    let destination = dir.path().join("mixed.csv");
    store.reference_changes_csv(&q, &destination).unwrap();
    let data = fs::read_to_string(destination).unwrap();
    assert!(data.contains(r#""Work";"B";"20";"0";"-20";"-100""#), "{data}");

    q.kind = "up".into();
    let result = store.reference_changes(&q).unwrap();
    assert_eq!(result.rows.iter().map(|row| row.org.as_str()).collect::<Vec<_>>(), ["A"]);
}

#[test]
fn independent_unequal_intervals_keep_partial_changes_and_real_works() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let revision = prepare(
        &store,
        vec![
            report(
                "base",
                "2024-01-01",
                json!([
                    row("A", "Work", 10., 100.),
                    row("A", "Gone", 1., 20.),
                    row("A", "Other", 1., 10.),
                    row("B", "Partial", 1., 5.)
                ]),
            ),
            report(
                "now1",
                "2024-01-10",
                json!([
                    row("A", "Work", 4., 50.),
                    row("A", "Other", 1., 10.),
                    row("A", "New", 2., 40.),
                    row("B", "Partial", 2., 1000.)
                ]),
            ),
            report("now2", "2024-01-11", json!([row("A", "Work", 8., 100.)])),
        ],
    );
    let q = request(revision);
    let result = store.reference_changes(&q).unwrap();
    let (current, baseline) = store
        .reference_scope(&q, |scope| {
            Ok((
                store.analysis_range_summary(revision, scope, &q.current())?,
                store.analysis_range_summary(revision, scope, &q.baseline())?,
            ))
        })
        .unwrap();
    let gap = current.single_missing.unwrap();
    assert_eq!((gap.org.as_str(), gap.start.as_str()), ("B", "2024-01-11"));
    assert!(baseline.single_missing.is_none());
    assert_eq!(result.comparable, ["A", "Empty"]);
    assert_eq!(
        (result.total, result.searched, result.significant),
        (5, 5, 4)
    );
    let partial = result.rows.iter().find(|r| r.org == "B").unwrap();
    assert!(partial.after.is_some());
    assert_eq!((partial.delta,partial.percent),(Some(995.),Some(19900.)));
    assert!(result.rows.iter().all(|r| r.name != "Combined"));
    let work = result.rows.iter().find(|r| r.name == "Work").unwrap();
    assert_eq!(
        (work.before, work.after, work.delta, work.percent),
        (Some(100.), Some(150.), Some(50.), Some(50.))
    );
    let new = result.rows.iter().find(|r| r.name == "New").unwrap();
    assert_eq!((new.before, new.after, new.percent), (Some(0.), Some(40.), None));
    assert_eq!(
        (
            result.counts.up,
            result.counts.down,
            result.counts.new,
            result.counts.gone
        ),
        (2, 0, 1, 1)
    );
    assert_eq!(result.leader.unwrap().name, "Partial");
    let mut filtered = q.clone();
    filtered.kind = "significant".into();
    filtered.search = "new".into();
    let found = store.reference_changes(&filtered).unwrap();
    assert_eq!((found.total, found.searched, found.significant), (1, 1, 1));
    filtered.kind = "down".into();
    filtered.search = "o".into();
    let result = store.reference_changes(&filtered).unwrap();
    assert_eq!(
        (result.total, result.searched, result.significant),
        (0, 3, 2)
    );
    assert_eq!(found.rows[0].name, "New");
    assert_eq!(found.counts.up, 2);
    assert_eq!(found.leader.unwrap().name, "Partial");
    assert_eq!(
        store
            .connect()
            .unwrap()
            .query_row("SELECT count(*) FROM analysis_filters", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn row_pages_sorting_complete_csv_and_formula_protection_are_consistent() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut base = Vec::new();
    let mut current = Vec::new();
    for i in 0..27 {
        let name = if i == 26 {
            "=SUM(A1)".into()
        } else {
            format!("Work{i:02}")
        };
        base.push(row("A", &name, 1., 10.));
        current.push(row("A", &name, 2., 20. + i as f64));
    }
    let revision = prepare(
        &store,
        vec![
            report("base", "2024-01-01", json!(base)),
            report("now1", "2024-01-10", json!(current)),
            report("now2", "2024-01-11", json!([])),
        ],
    );
    let mut q = request(revision);
    q.sort = "delta".into();
    let first = store.reference_changes(&q).unwrap();
    assert_eq!(first.rows.len(), 12);
    assert_eq!(first.total, 27);
    assert_eq!(first.rows[0].name, "=SUM(A1)");
    q.page = 1;
    let second = store.reference_changes(&q).unwrap();
    assert_eq!(second.rows.len(), 12);
    assert!(first
        .rows
        .iter()
        .all(|r| second.rows.iter().all(|s| s.name != r.name)));
    q.page = 2;
    assert_eq!(store.reference_changes(&q).unwrap().rows.len(), 3);
    let path = dir.path().join("full.csv");
    store.reference_changes_csv(&q, &path).unwrap();
    let csv = fs::read_to_string(&path).unwrap();
    assert!(csv.starts_with('\u{feff}'));
    assert_eq!(csv.lines().skip_while(|line|!line.starts_with("\"Работа\";")).skip(1).count(), 27);
    assert!(csv.contains("'=SUM(A1)"));
    assert!(store.reference_changes_csv(&q, &path).is_err());
    q.metric = "count".into();
    q.sort = "percent".into();
    assert_eq!(store.reference_changes(&q).unwrap().total, 27);
}
#[test]
fn missing_periods_are_not_zero_filled_and_negative_baselines_keep_percent() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let revision = prepare(
        &store,
        vec![
            report(
                "base",
                "2024-01-01",
                json!([row("A", "Negative", -2., -100.)]),
            ),
            report(
                "now1",
                "2024-01-10",
                json!([row("A", "Negative", -1., -50.)]),
            ),
        ],
    );
    let mut q = request(revision);
    let missing = store.reference_changes(&q).unwrap();
    assert!(missing.comparable.is_empty());
    assert_eq!(missing.total, 1);
    let observed = &missing.rows[0];
    assert_eq!((observed.before,observed.after,observed.delta,observed.percent),(Some(-100.),Some(-50.),Some(50.),Some(50.)));
    assert!(missing.leader.is_some());
    q.to = q.from.clone();
    let found = store.reference_changes(&q).unwrap();
    assert_eq!(found.rows[0].percent, Some(50.));
    assert_eq!(found.counts.up, 1);
    q.revision -= 1;
    assert!(store.reference_changes(&q).is_err());
}
#[test]
fn filters_thresholds_limits_and_duplicate_reports_reject_or_exclude_safely() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let revision = prepare(
        &store,
        vec![
            report("base", "2024-01-01", json!([row("A", "Work", 1., 1.)])),
            report("now1", "2024-01-10", json!([row("A", "Work", 2., 2.)])),
            report("dup", "2024-01-10", json!([row("A", "Work", 3., 3.)])),
            report("now2", "2024-01-11", json!([])),
        ],
    );
    let q = request(revision);
    let result = store.reference_changes(&q).unwrap();
    assert!(result.comparable.is_empty());
    for change in [0, 1, 2, 3, 4] {
        let mut bad = q.clone();
        match change {
            0 => bad.metric = "average".into(),
            1 => bad.percent = -1.,
            2 => bad.kind = "invalid".into(),
            3 => bad.sort = "invalid".into(),
            _ => bad.page = u32::MAX,
        };
        assert!(store.reference_changes(&bad).is_err());
    }
}
#[test]
fn failures_clean_up_filters_and_never_publish_csv_or_change_revision() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let revision = prepare(
        &store,
        vec![report(
            "base",
            "2024-01-01",
            json!([row("A", "Work", 1., 1.)]),
        )],
    );
    let q = request(revision);
    let c = store.connect().unwrap();
    c.execute("DELETE FROM analysis_reports", []).unwrap();
    assert!(store.reference_changes(&q).is_err());
    let path = dir.path().join("failed.csv");
    assert!(store.reference_changes_csv(&q, &path).is_err());
    assert!(!path.exists());
    assert_eq!(store.revision().unwrap(), revision);
    assert_eq!(
        c.query_row("SELECT count(*) FROM analysis_filters", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM analysis_filter_items", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    for mutate in [0, 1, 2, 3] {
        let mut bad = q.clone();
        match mutate {
            0 => bad.organizations.push("A".into()),
            1 => bad.base_to = "2035-01-01".into(),
            2 => bad.base_from = "2024-02-30".into(),
            _ => bad.min_amount = f64::INFINITY,
        };
        assert!(store.reference_changes(&bad).is_err());
    }
    assert!(store
        .reference_changes_csv(&q, Path::new("relative.csv"))
        .is_err());
    assert!(store
        .reference_changes_csv(&q, &dir.path().join("not.csv.txt"))
        .is_err());
}

#[test]
fn summary_csv_exports_observed_partial_values_and_leaves_unknown_cells_empty() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let revision = prepare(
        &store,
        vec![
            report("base", "2024-01-01", json!([])),
            report("now1", "2024-01-10", json!([row("A", "Work", 2., 20.)])),
        ],
    );
    let mut q = request(revision);
    q.organizations = vec!["Missing".into(), "Empty".into(), "A".into()];
    let path = dir.path().join("partial-summary.csv");
    store.reference_summary_csv(&q, &path).unwrap();
    let csv = fs::read_to_string(&path).unwrap();
    assert!(csv.starts_with('\u{feff}'));
    assert!(csv.contains("2024-01-10"));
    assert!(csv.contains("2024-01-01"));
    assert!(csv.contains("Доля суммы, %"));
    assert!(csv.contains("\"A\";\"20\";\"2\";\"100\";\"10\";\"частичные данные\""));
    assert!(csv.contains("\"Empty\";\"0\";\"0\";\"0\";\"\";\"частичные данные\""));
    assert!(csv.contains("\"Missing\";\"\";\"\";\"\";\"\";\"нет данных\""));
    let observed=csv.split("\"Подразделение\";\"Сумма\";").nth(1).unwrap();
    assert!(observed.find("\"A\";").unwrap()<observed.find("\"Empty\";").unwrap());
    assert!(observed.find("\"Empty\";").unwrap()<observed.find("\"Missing\";").unwrap());
    assert!(store.reference_summary_csv(&q, &path).is_err());
    q.to = q.from.clone();
    let full = dir.path().join("full-summary.csv");
    store.reference_summary_csv(&q, &full).unwrap();
    assert!(fs::read_to_string(full)
        .unwrap()
        .contains("\"A\";\"20\";\"2\";\"100\";\"10\";\"полные периоды\""));
    q.organizations = vec!["Empty".into()];
    let zero = dir.path().join("zero-summary.csv");
    store.reference_summary_csv(&q, &zero).unwrap();
    assert!(fs::read_to_string(zero)
        .unwrap()
        .contains("\"Empty\";\"0\";\"0\";\"\";\"\";\"полные периоды\""));
    q.organizations.clear();
    let unset = dir.path().join("unset-summary.csv");
    assert!(store.reference_summary_csv(&q, &unset).is_err());
    assert!(!unset.exists());
    assert_eq!(store.revision().unwrap(), revision);
    assert_eq!(
        store
            .connect()
            .unwrap()
            .query_row("SELECT count(*) FROM analysis_filters", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn comparable_sample_and_totals_match_overview_changes_comparison_and_exports() {
    for (missing_base, missing_current, zero_base) in [(false,true,false),(true,false,false),(true,true,false),(false,true,true)] {
        let dir=tempdir().unwrap();let store=Storage::new(dir.path().into()).unwrap();
        let organizations=["A","B","Empty","D"];
        let mut reports=Vec::new();
        for (id,start,baseline,last,value) in [("b1","2024-01-01",true,false,10.),("b2","2024-01-02",true,true,20.),("a1","2024-01-10",false,false,30.),("a2","2024-01-11",false,true,40.)] {
            let present:Vec<_>=organizations.iter().copied().filter(|org|*org!="D"||!last||!(if baseline{missing_base}else{missing_current})).collect();
            let rows:Vec<_>=present.iter().map(|org| {let amount=if *org=="D"{10000.}else if baseline&&zero_base{0.}else{value};row(org,"Work",amount/10.,amount)}).collect();
            reports.push(json!({"id":id,"type":"day","start":start,"end":start,"organizations":present,"rows":rows,"warnings":["Сохранённое контрольное замечание"]}));
        }
        let revision=prepare(&store,reports);let mut q=request(revision);q.base_to="2024-01-02".into();q.organizations=organizations.iter().map(|org|org.to_string()).collect();
        let changes=store.reference_changes(&q).unwrap();assert_eq!(changes.comparable,["A","B","Empty"]);assert_eq!(changes.rows.len(),4);
        let incomplete=changes.rows.iter().find(|row|row.org=="D").unwrap();
        let before = if missing_base { 10000. } else { 20000. };
        let after = if missing_current { 10000. } else { 20000. };
        assert_eq!((incomplete.before,incomplete.after,incomplete.delta),(Some(before),Some(after),Some(after-before)));
        let query=ComparisonQuery{before:q.baseline(),after:q.current(),percent:0.,min_count:0.,min_amount:0.};
        store.reference_scope(&q,|scope| {
            let overview=store.analysis_comparison_dashboard(revision,scope,&query)?;
            let before=store.analysis_organization_page(revision,scope,&q.baseline(),None,50)?;
            let after=store.analysis_organization_page(revision,scope,&q.current(),None,50)?;
            let mut included:Vec<_>=before.iter().filter(|row|row.complete&&after.iter().any(|other|other.org==row.org&&other.complete)).map(|row|row.org.clone()).collect();included.sort();
            let mut declared=changes.comparable.clone();declared.sort();assert_eq!(included,declared);
            let mut divisions:Vec<_>=overview.divisions.iter().filter(|row|row.before.amount.is_some()&&row.after.amount.is_some()).map(|row|row.organization.clone()).collect();divisions.sort();assert_eq!(divisions,declared);
            assert_eq!(overview.summary.before.complete,!missing_base);assert_eq!(overview.summary.after.complete,!missing_current);
            assert!(overview.summary.before.import_warning_reports>0);assert!(overview.summary.after.import_warning_reports>0);
            Ok(())
        }).unwrap();
        let mut sample=q.clone();sample.organizations=changes.comparable.clone();
        store.reference_scope(&sample,|scope| {
            let comparison=store.analysis_comparison_dashboard(revision,scope,&query)?;
            let window=store.analysis_comparison_window(revision,scope,&query,None,0,50)?;
            let before=if zero_base{0.}else{90.};
            assert_eq!(comparison.summary.before.total.amount,Some(before));assert_eq!(comparison.summary.after.total.amount,Some(210.));
            let measure=comparison.summary.amount.as_ref().unwrap();assert_eq!(measure.delta,210.-before);assert_eq!(measure.percent.is_none(),zero_base);
            assert_eq!(window.rows.len(),1);assert_eq!(window.rows[0].amount.as_ref().unwrap().base,before);assert_eq!(window.rows[0].amount.as_ref().unwrap().current,210.);
            assert_eq!(changes.rows.iter().filter(|row|row.org!="D").map(|row|row.before.unwrap()).sum::<f64>(),before);assert_eq!(changes.rows.iter().filter(|row|row.org!="D").map(|row|row.after.unwrap()).sum::<f64>(),210.);
            Ok(())
        }).unwrap();
        let summary=dir.path().join("summary.csv");let change_csv=dir.path().join("changes.csv");store.reference_summary_csv(&q,&summary).unwrap();store.reference_changes_csv(&q,&change_csv).unwrap();
        for path in [summary,change_csv] {
            let csv=fs::read_to_string(path).unwrap();
            assert!(csv.contains("\"Сопоставимые подразделения\";\"3\";\"4\";\"A, B, Empty\""),"CSV must declare the exact common sample: {csv}");
            let reason=if missing_base&&missing_current{"неполные базовый и анализируемый интервалы"}else if missing_base{"неполный базовый интервал"}else{"неполный анализируемый интервал"};
            assert!(csv.contains(&format!("\"Состав сравнения\";\"D\";\"{reason}\"")));
            let total_before = before + if zero_base { 0. } else { 90. };
            let total_after = after + 210.;
            assert!(csv.contains(&format!("\"Сумма\";\"{total_before}\";\"{total_after}\";\"{}\";",total_after-total_before)), "{csv}");
        }
        assert_eq!(store.revision().unwrap(),revision);
    }
}

#[test]
fn disjoint_organizations_keep_aggregate_comparison_without_per_organization_deltas() {
    let dir=tempdir().unwrap();let store=Storage::new(dir.path().into()).unwrap();
    let revision=prepare(&store,vec![json!({"id":"base","type":"day","start":"2024-01-01","end":"2024-01-01","organizations":["A","B"],"rows":[row("A","Work",1.,10.),row("B","Work",2.,20.)]}),json!({"id":"current","type":"day","start":"2024-01-10","end":"2024-01-10","organizations":["Empty","D"],"rows":[row("Empty","Work",3.,30.),row("D","Work",4.,40.)]})]);
    let mut q=request(revision);q.to=q.from.clone();q.organizations=vec!["A".into(),"B".into(),"Empty".into(),"D".into()];
    let result=store.reference_changes(&q).unwrap();assert!(result.comparable.is_empty());assert_eq!(result.rows.len(),4);
    assert!(result.rows.iter().all(|row| row.delta.is_none() && row.before.is_some()!=row.after.is_some()));
    for (summary,file) in [(true,"summary.csv"),(false,"changes.csv")] {
        let path=dir.path().join(file);if summary{store.reference_summary_csv(&q,&path).unwrap();}else{store.reference_changes_csv(&q,&path).unwrap();}
        let csv=fs::read_to_string(path).unwrap();assert!(csv.contains("\"Сопоставимые подразделения\";\"0\";\"4\";\"\""));assert!(csv.contains("\"Сумма\";\"30\";\"70\";\"40\";"));
    }
}

#[test]
fn all_work_summary_and_changes_are_explicitly_separate_from_selected_detail_group_totals() {
    let dir=tempdir().unwrap();let store=Storage::new(dir.path().into()).unwrap();
    let revision=prepare(&store,vec![report("base","2024-01-01",json!([row("A","Work",1.,10.),row("A","Other",9.,90.),row("A","Independent",100.,1000.)])),report("now","2024-01-10",json!([row("A","Work",2.,20.),row("A","Other",18.,180.),row("A","Independent",200.,2000.)]))]);
    let mut q=request(revision);q.to=q.from.clone();q.organizations=vec!["A".into()];
    let changes=store.reference_changes(&q).unwrap();assert_eq!(changes.rows.iter().map(|row|row.before.unwrap()).sum::<f64>(),1100.);assert_eq!(changes.rows.iter().map(|row|row.after.unwrap()).sum::<f64>(),2200.);
    store.reference_scope(&q,|scope| {
        let filter=store.analysis_filter_create(revision,"names")?;store.analysis_filter_append(revision,&filter,&["Combined".into()])?;store.analysis_filter_seal(revision,&filter)?;
        let selected=AnalysisScope{report_type:scope.report_type.clone(),organizations:scope.organizations.clone(),names:Some(filter.clone())};
        let query=ComparisonQuery{before:q.baseline(),after:q.current(),percent:0.,min_count:0.,min_amount:0.};
        let overview=store.analysis_comparison_dashboard(revision,scope,&query)?;let detail=store.analysis_comparison_dashboard(revision,&selected,&query)?;
        assert_eq!(overview.summary.before.total.amount,Some(1100.));assert_eq!(overview.summary.after.total.amount,Some(2200.));assert_eq!(detail.summary.before.total.amount,Some(100.));assert_eq!(detail.summary.after.total.amount,Some(200.));store.analysis_filter_delete(&filter)
    }).unwrap();
    for (summary,name) in [(true,"summary.csv"),(false,"changes.csv")] {
        let destination=dir.path().join(name);if summary{store.reference_summary_csv(&q,&destination).unwrap();}else{store.reference_changes_csv(&q,&destination).unwrap();}
        let csv=fs::read_to_string(destination).unwrap();assert!(csv.contains("\"Область работ\";\"Все работы\""));assert!(csv.contains("\"Сумма\";\"1100\";\"2200\";\"1100\";\"100\""));
    }
}

#[test]
fn print_streams_the_complete_sorted_filter_from_one_snapshot_in_bounded_batches() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut base = Vec::new();
    let mut current = Vec::new();
    for i in 0..1201 {
        let name = format!("Позиция {i:04}");
        base.push(row("A", &name, 1., 10.));
        current.push(row("A", &name, 2., 100. + i as f64));
    }
    base.push(row("A", "Позиция исключена по категории", 1., 1000.));
    current.push(row("A", "Позиция исключена по категории", 1., 100.));
    base.push(row("A", "Не отвечает поиску", 1., 10.));
    current.push(row("A", "Не отвечает поиску", 5., 4000.));
    let revision = prepare(
        &store,
        vec![
            report("base", "2024-01-01", json!(base)),
            report("now", "2024-01-10", json!(current)),
        ],
    );
    let mut q = request(revision);
    q.to = q.from.clone();
    q.page = 99;
    q.kind = "up".into();
    q.sort = "delta".into();
    q.search = "позиц".into();

    let cancelled = Arc::new(AtomicBool::new(false));
    let mut names = Vec::new();
    let mut sizes = Vec::new();
    let sample = store.reference_changes_print(&q, Arc::clone(&cancelled), |rows| {
        sizes.push(rows.len());
        names.extend(rows.into_iter().map(|r| r.name));
        Ok(())
    }).unwrap();
    assert_eq!(sample, ["A", "Empty"]);
    assert_eq!(sizes, [128, 128, 128, 128, 128, 128, 128, 128, 128, 49]);
    assert_eq!(names, (0..1201).rev().map(|i| format!("Позиция {i:04}")).collect::<Vec<_>>());

    let error = store.reference_changes_print(&q, Arc::clone(&cancelled), |_| Err("Channel closed".into())).unwrap_err();
    assert_eq!(error, "Channel closed");
    let c = store.connect().unwrap();
    assert_eq!(c.query_row("SELECT count(*) FROM analysis_filters", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(store.revision().unwrap(), revision);
    let mut received = 0;
    assert!(store.reference_changes_print(&q, Arc::clone(&cancelled), |rows| {
        received += rows.len();
        cancelled.store(true, Ordering::Release);
        Ok(())
    }).is_err());
    assert_eq!(received, 128);
    cancelled.store(false, Ordering::Release);
    q.revision -= 1;
    assert!(store.reference_changes_print(&q, Arc::clone(&cancelled), |_| panic!("stale revision emitted rows")).is_err());
}

#[test]
fn reference_changes_show_observed_values_without_fabricating_deltas() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut unknown = row("A", "Work", 4., 0.);
    unknown["amount"] = Value::Null;
    let revision = prepare(&store, vec![
        report("base", "2024-01-01", json!([row("A", "Work", 2., 20.)])),
        report("now", "2024-01-10", json!([row("A", "Work", 3., 30.),unknown])),
        json!({"id":"b","type":"day","start":"2024-01-10","end":"2024-01-10",
            "organizations":["B"],"rows":[row("B","Work",5.,40.)]})
    ]);
    let mut q = request(revision);
    q.kind = "significant".into();
    let result = store.reference_changes(&q).unwrap();
    assert!(result.comparable.is_empty());
    assert_eq!(result.significant, 0);
    assert_eq!(result.rows.len(), 2);
    assert_eq!((result.rows[0].before, result.rows[0].after, result.rows[0].delta), (Some(20.),Some(30.),None));
    assert_eq!((result.rows[1].before, result.rows[1].after, result.rows[1].delta), (None,Some(40.),None));
    let path = dir.path().join("changes.csv");
    store.reference_changes_csv(&q,&path).unwrap();
    let csv = fs::read_to_string(path).unwrap();
    assert!(csv.lines().any(|line| line == r#""Work";"B";"";"40";"";"""#));
}
