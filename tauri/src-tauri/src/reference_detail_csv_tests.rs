use super::*;

#[test]
fn detail_csv_streams_all_filtered_rows_and_preserves_the_selected_table_shape() {
    let (dir, store, revision, scope) = fixture();
    let mut q = query();
    q.to = "2024-01-02".into();
    q.from = q.to.clone();
    let before = json!({"from":"2024-01-01","to":"2024-01-01"});
    for section in [
        "comparison",
        "contributions",
        "organizations",
        "usual",
        "regularity",
        "structure",
    ] {
        let mut spec = json!({"section":section,"metric":"amount"});
        match section {
            "usual" | "regularity" => {
                spec["options"] = json!({"status":"all","search":""});
                spec["window"] = json!(4);
            }
            "organizations" => spec["baseOrg"] = json!("A"),
            _ => {
                spec["before"] = before.clone();
                if section == "structure" {
                    spec["limit"] = json!(10);
                }
            }
        }
        let table: DetailCsvOptions = serde_json::from_value(spec).unwrap();
        let path = dir.path().join(format!("{section}.csv"));
        store
            .reference_detail_csv(revision, &scope, &q, None, &table, &path)
            .unwrap();
        let data = std::fs::read_to_string(&path).unwrap();
        assert!(data.starts_with('\u{feff}'));
        assert!(data.contains("\"Подразделения\";\"A\""));
        let data_rows = data
            .lines()
            .filter(|line| line.starts_with("\"Работа ") && !line.starts_with("\"Работа /"))
            .count();
        if section == "structure" {
            assert!(data_rows <= 10);
            assert!(
                data.contains("Остальн") || data.contains("Прочие"),
                "{data}"
            );
        } else {
            assert!(data_rows > 50, "{section} exported {data_rows}");
            assert!(
                data.contains("\"ЯРКАЯ услуга\""),
                "missing final record in {section}"
            );
        }
        let original = std::fs::read(&path).unwrap();
        assert!(store
            .reference_detail_csv(revision, &scope, &q, None, &table, &path)
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}
#[test]
fn detail_csv_validates_scope_destination_and_formula_safe_filter_text() {
    let (dir, store, r, scope) = fixture();
    let q = query();
    let table:DetailCsvOptions=serde_json::from_value(json!({"section":"regularity","metric":"count","options":{"search":"=формула","status":"all"},"window":4})).unwrap();
    let path = dir.path().join("scope.csv");
    assert!(store
        .reference_detail_csv(r + 1, &scope, &q, None, &table, &path)
        .is_err());
    assert!(!path.exists());
    assert!(store
        .reference_detail_csv(r, &scope, &q, None, &table, Path::new("relative.csv"))
        .is_err());
    assert!(store
        .reference_detail_csv(
            r,
            &scope,
            &q,
            None,
            &table,
            &dir.path().join("report.sqlite3")
        )
        .is_err());
    store
        .reference_detail_csv(r, &scope, &q, None, &table, &path)
        .unwrap();
    let data = std::fs::read_to_string(&path).unwrap();
    assert!(data.contains("\"Поиск\";\"'=формула\""));
    assert!(!data.contains("ЯРКАЯ услуга"));
}

#[test]
fn usual_csv_exports_observed_values_with_coverage_notice_and_empty_deltas() {
    let (dir, store, revision, scope) = fixture();
    let q = ChangeQuery { from: "2024-01-02".into(), to: "2024-01-03".into(), ..query() };
    let table: DetailCsvOptions = serde_json::from_value(json!({
        "section":"usual","metric":"amount","window":4,
        "options":{"search":"яркая","status":"all","range_window":true}
    })).unwrap();
    let path = dir.path().join("observed-usual.csv");
    store.reference_detail_csv(revision, &scope, &q, None, &table, &path).unwrap();
    let data = std::fs::read_to_string(path).unwrap();
    assert!(data.contains("Текущий интервал неполон. Показаны доступные значения."));
    assert!(data.lines().any(|line| line == r#""ЯРКАЯ услуга";"";"94";"";"";"Недостаточно данных""#), "{data}");
}

#[test]
fn usual_csv_labels_partial_sums_even_with_complete_calendar_coverage() {
    let (dir, store, _, scope) = fixture();
    let mut data = store.load().unwrap();
    data["reports"][1]["rows"][74]["amount"] = Value::Null;
    store.save(data).unwrap();
    let revision = store.revision().unwrap();
    store.analysis_index_report("d0", revision).unwrap();
    store.analysis_index_report("d1", revision).unwrap();
    store.analysis_index_settings(revision).unwrap();
    let organizations = store.analysis_filter_create(revision, "organizations").unwrap();
    store.analysis_filter_append(revision, &organizations, &["A".into()]).unwrap();
    store.analysis_filter_seal(revision, &organizations).unwrap();
    let scope = AnalysisScope { organizations, ..scope };
    let q = ChangeQuery { from: "2024-01-01".into(), to: "2024-01-02".into(), ..query() };
    let options = json!({"search":"яркая","status":"all","range_window":true});
    let page = store.unified_pattern_page(revision, &scope, &q, None, "usual", 4, &serde_json::from_value(options.clone()).unwrap(), None).unwrap();
    assert!(page.current_complete);
    assert_eq!(page.rows[0]["amount"]["current"], 84.0);
    assert!(page.rows[0]["average"]["current"].is_null());
    let table: DetailCsvOptions = serde_json::from_value(json!({
        "section":"usual","metric":"amount","window":4,"options":options
    })).unwrap();
    let path = dir.path().join("usual-missing-amount.csv");
    store.reference_detail_csv(revision, &scope, &q, None, &table, &path).unwrap();
    let data = std::fs::read_to_string(path).unwrap();
    assert!(data.contains("В некоторых строках сумма не указана."));
    assert!(data.lines().any(|line| line == r#""ЯРКАЯ услуга";"";"84";"";"";"Недостаточно данных""#), "{data}");
}

#[test]
fn contributions_with_incomplete_coverage_keep_observed_rows_across_pages_and_csv() {
    let (dir, store, revision, scope) = fixture();
    let q = ChangeQuery { from: "2024-01-02".into(), to: "2024-01-03".into(), ..query() };
    let before = RangeQuery { from: "2024-01-01".into(), to: "2024-01-01".into() };
    let first = store.advanced_composition_at_page(revision, &scope, &q, None, &before, "contributions", "amount", 12, 0).unwrap();
    let last = store.advanced_composition_at_page(revision, &scope, &q, None, &before, "contributions", "amount", 12, 1).unwrap();
    assert_eq!(first["currentComplete"], false);
    assert_eq!(first["total_rows"], 75);
    assert_eq!(first["rows"].as_array().unwrap().len(), 50);
    assert!(first["check"].is_null());
    let row = last["rows"].as_array().unwrap().iter().find(|row| row["name"] == "ЯРКАЯ услуга").unwrap();
    assert_eq!((row["base"].as_f64(), row["current"].as_f64()), (Some(84.), Some(94.)));
    assert!(row["delta"].is_null() && row["percent"].is_null() && row["contribution"].is_null());
    let zero = &first["rows"][1];
    assert_eq!((zero["base"].as_f64(), zero["current"].as_f64()), (Some(0.), Some(0.)));
    let table: DetailCsvOptions = serde_json::from_value(json!({
        "section":"contributions","metric":"amount","before":before
    })).unwrap();
    let path = dir.path().join("observed-contributions.csv");
    store.reference_detail_csv(revision, &scope, &q, None, &table, &path).unwrap();
    let csv = std::fs::read_to_string(path).unwrap();
    let notice = csv.find("Показаны доступные значения.").unwrap();
    let header = csv.find(r#""Услуга / группа";"Значение раньше""#).unwrap();
    assert!(notice < header, "{csv}");
    assert!(csv.lines().any(|line| line == r#""ЯРКАЯ услуга";"84";"94";"";"";"""#), "{csv}");
    assert!(csv.lines().any(|line| line == r#""Работа 01";"0";"0";"";"";"""#), "{csv}");
}

#[test]
fn unavailable_interval_preserves_only_the_complete_sides_values_on_screen_and_in_csv() {
    let (dir, store, r, scope) = fixture();
    let mut selection = query();
    selection.from = "2024-03-01".into();
    selection.to = selection.from.clone();
    let before = RangeQuery {
        from: "2024-01-01".into(),
        to: "2024-01-01".into(),
    };
    let cmp = ComparisonQuery {
        before: before.clone(),
        after: RangeQuery {
            from: selection.from.clone(),
            to: selection.to.clone(),
        },
        percent: 0.,
        min_amount: 0.,
        min_count: 0.,
    };
    let page = store
        .analysis_comparison_page(r, &scope, &cmp, None, None, 50)
        .unwrap();
    let first = page.iter().find(|x| x.name == "Работа 00").unwrap();
    assert!(first.amount.is_none());
    let available = first.available.as_ref().unwrap();
    assert_eq!(available.amount, Some(-10.));
    assert_eq!(available.count, Some(-1.));
    assert_eq!(available.average, None);
    let zero = page.iter().find(|x| x.name == "Работа 01").unwrap();
    let available = zero.available.as_ref().unwrap();
    assert_eq!((available.amount, available.count, available.average), (Some(0.), Some(1.), Some(0.)));
    for (metric, negative, zero) in [
        ("amount", Some("-10"), "0"), ("count", Some("-1"), "1"), ("average", None, "0"),
    ] {
        let table: DetailCsvOptions = serde_json::from_value(json!({
            "section":"comparison","metric":metric,"before":before,
        })).unwrap();
        let path = dir.path().join(format!("partial-{metric}.csv"));
        store.reference_detail_csv(r, &scope, &selection, None, &table, &path).unwrap();
        let data = std::fs::read_to_string(path).unwrap();
        assert!(data.contains(r#""Работа / группа";"Значение раньше""#), "{data}");
        let negative = negative.map(|n| format!(r#""Работа 00";"{n}""#));
        assert_eq!(data.lines().find(|line| line.starts_with(r#""Работа 00";"#)), negative.as_deref(), "{data}");
        assert!(data.lines().any(|line| line == format!(r#""Работа 01";"{zero}""#)), "{data}");
        assert!(!data.contains("Разница"), "{data}");
    }
}

#[test]
fn imported_detail_csv_labels_the_inclusive_end_of_each_source_period() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().join("periods")).unwrap();
    let sources = [
        ("week","2026-11-04","2026-11-10","2026-11-11","2026-11-17"),
        ("month","2024-02-01","2024-02-29","2025-02-01","2025-02-28"),
    ];
    let mut reports = Vec::new();
    for (kind,previous,previous_end,current,current_end) in sources {
        for (start,end,amount) in [(previous,previous_end,10),(current,current_end,20)] {
            reports.push(json!({"id":start,"type":kind,"start":start,"end":end,"organizations":["A"],
                "rows":[{"organization":"A","work":"Work","count":1,"amount":amount}]}));
        }
    }
    store.save(json!({"reports":reports,"settings":{"orgNames":["A","B","C","D"],"groups":[],"catalog":[]}})).unwrap();
    let rev = store.revision().unwrap();
    for (_,previous,_,current,_) in sources {
        for id in [previous,current] { store.analysis_index_report(id,rev).unwrap(); }
    }
    store.analysis_index_settings(rev).unwrap();
    let orgs = store.analysis_filter_create(rev,"organizations").unwrap();
    store.analysis_filter_append(rev,&orgs,&["A".into()]).unwrap();
    store.analysis_filter_seal(rev,&orgs).unwrap();
    for (kind,previous,previous_end,current,current_end) in sources {
        let scope = AnalysisScope {report_type:kind.into(),organizations:orgs.clone(),names:None};
        let mut q = query();
        q.from=current.into();
        q.to=current.into();
        let table: DetailCsvOptions = serde_json::from_value(json!({"section":"contributions","metric":"amount",
            "before":{"from":previous,"to":previous}})).unwrap();
        let file=dir.path().join(format!("{kind}.csv"));
        store.reference_detail_csv(rev,&scope,&q,None,&table,&file).unwrap();
        let csv=std::fs::read_to_string(file).unwrap();
        assert!(csv.contains(&format!(r#""Анализируемый период";"{current}";"{current_end}""#)),"{csv}");
        assert!(csv.contains(&format!(r#""Базовый период";"{previous}";"{previous_end}""#)),"{csv}");
    }
}

#[test]
fn department_work_pages_and_detail_csvs_keep_group_and_work_namesakes_separate() {
    let dir = tempfile::tempdir().unwrap();
    let store = Storage::new(dir.path().join("store")).unwrap();
    let report = |day: &str, factor: f64| {
        let mut rows = Vec::new();
        for (org, group, work) in [("A",100.,40.),("B",200.,60.)] {
            for (name, amount) in [("Сварка",group),("Я",work)] {
                rows.push(json!({"organization":org,"work":name,"amount":amount*factor,"count":1}));
            }
        }
        // The matching pair straddles the 50-row page boundary.
        for i in 0..49 {
            rows.push(json!({"organization":"A","work":format!("А{i:02}"),"amount":0,"count":0}));
        }
        json!({"id":day,"type":"day","start":day,"end":day,"organizations":["A","B"],"rows":rows})
    };
    let first = "2025-03-01";
    let second = "2025-03-02";
    store.save(json!({"reports":[report(first,0.5),report(second,1.)],
        "settings":{"orgNames":["A","B","C","D"],"groups":[{"id":"g","name":"Я","works":["Сварка"]}],"catalog":[]}})).unwrap();
    let revision = store.revision().unwrap();
    for day in [first,second] {store.analysis_index_report(day,revision).unwrap();}
    store.analysis_index_settings(revision).unwrap();
    let make_scope = |names: &[&str]| {
        let orgs = store.analysis_filter_create(revision,"organizations").unwrap();
        let items:Vec<_> = names.iter().map(|name|(*name).to_owned()).collect();
        store.analysis_filter_append(revision,&orgs,&items).unwrap();
        store.analysis_filter_seal(revision,&orgs).unwrap();
        AnalysisScope { report_type:"day".into(), organizations:orgs, names:None }
    };
    let scope = make_scope(&["B","A","C"]);
    let range = RangeQuery {from:second.into(),to:second.into()};
    let mut q = query();
    q.from = second.into();
    q.to = second.into();
    for (index, label, key, b, a) in [(0,"Я (группа)","group:g",200.,100.),(1,"Я (работа)","work:Я",60.,40.)] {
        let page = store.organization_work_window(revision,&scope,&range,index,50).unwrap();
        assert_eq!(page.total_rows,51);
        let item = page.rows.last().unwrap();
        assert_eq!(item.entry.name,label);
        assert_eq!(item.entry.key,key);
        let cells:Vec<_> = item.rows.iter().map(|org|(org.org.as_str(),org.total.amount)).collect();
        assert_eq!(cells,vec![("B",Some(b)),("A",Some(a)),("C",None)]);
    }
    assert!(store.organization_work_window(revision,&scope,&range,2,50).is_err());
    let spec = serde_json::from_value(json!({"section":"organizations","metric":"amount","baseOrg":"B"})).unwrap();
    let path = dir.path().join("organization.csv");
    store.reference_detail_csv(revision,&scope,&q,None,&spec,&path).unwrap();
    let csv = std::fs::read_to_string(path).unwrap();
    for expected in [r#""Я (группа)";"200";"100";"-100";"-50";"";"";"""#,
        r#""Я (работа)";"60";"40";"-20";"-33,333333333333"#] {
        assert!(csv.contains(expected),"{csv}");
    }
    let covered = make_scope(&["A","B"]);
    for section in ["comparison","contributions","structure","usual","regularity"] {
        let mut options = json!({"section":section,"metric":"amount"});
        if matches!(section,"usual"|"regularity") {
            options["window"] = json!(4);
            options["options"] = json!({"search":"","status":"all"});
        } else {
            options["before"] = json!({"from":first,"to":first});
            if section == "structure" {options["limit"] = json!(10);}
        }
        let spec = serde_json::from_value(options).unwrap();
        let path = dir.path().join(format!("{section}.csv"));
        store.reference_detail_csv(revision,&covered,&q,None,&spec,&path).unwrap();
        let csv = std::fs::read_to_string(path).unwrap();
        assert_eq!(csv.lines().filter(|r|r.starts_with(r#""Я (группа)";"#)).count(),1,"{section}: {csv}");
        assert_eq!(csv.lines().filter(|r|r.starts_with(r#""Я";"#)).count(),1,"{section}: {csv}");
    }
}
