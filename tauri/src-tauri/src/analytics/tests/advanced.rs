use super::*;

#[test]
fn daily_week_changes_use_available_reports_and_ignore_imported_totals() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    let reports = data["reports"].as_array_mut().unwrap();
    reports.clear();
    for day in 1..=21 {
        if day == 17 {
            continue;
        }
        let start = format!("2024-01-{day:02}");
        let count = if day <= 7 { 0 } else { -1 };
        reports.push(json!({"id":start,"type":"day","start":start,"end":start,"organizations":["A"],"rows":[{"organization":"A","work":"item","count":count,"amount":day}]}));
    }
    reports.push(json!({"id":"imported","type":"week","start":"2024-01-08","end":"2024-01-14","organizations":["A"],"rows":[{"organization":"A","work":"item","count":99,"amount":999}]}));
    let rev = prepare(&store, &data);
    let s = scope(&store, rev, "day", &["A"], None);
    let q = ChangeQuery {
        method: "range".into(),
        from: "2024-01-01".into(),
        to: "2024-01-28".into(),
        percent: 20.,
        min_count: 1.,
        min_amount: 1.,
    };
    let page = store
        .advanced_changes_page(rev, &s, &q, Some("week"), None)
        .unwrap();
    assert_eq!(page.summary.missing_pairs, 1);
    assert_eq!(page.summary.partial_pairs, 1);
    assert_eq!(page.missing_examples[0].current_start, "2024-01-22");
    assert!(!page.missing_examples[0].missing_base);
    assert!(page.missing_examples[0].missing_current);
    assert_eq!(page.rows.len(), 2);
    let item = &page.rows[0];
    assert_eq!(
        (&item.base_start, &item.current_start),
        (&"2024-01-01".into(), &"2024-01-08".into())
    );
    assert_eq!(
        (item.count.base, item.count.current, item.amount.as_ref().unwrap().current),
        (0., -7., 77.)
    );
    assert_eq!(item.count.percent, None);
    let partial = &page.rows[1];
    assert_eq!(partial.current_start, "2024-01-15");
    assert_eq!((partial.count.base, partial.count.current), (-7., -6.));
    assert_eq!(partial.amount.as_ref().unwrap().current, 109.);

    let file = dir.path().join("changes.csv");
    let options = UnifiedChangeOptions {
        metrics: vec!["amount".into()],
        mode: "all".into(),
        search: String::new(),
        direction: "all".into(),
        sort: "name".into(),
    };
    store.unified_changes_csv(rev, &s, &q, Some("week"), &options, &file).unwrap();
    let csv = fs::read_to_string(file).unwrap();
    assert!(csv.contains("Пар с неполным покрытием отчётами: 1. Пар без отчётов за один или оба периода: 1."));
    assert!(csv.lines().any(|row| row.starts_with("\"work:item\"") && row.contains("\"109\"")));
    assert!(!csv.contains("2024-01-22"));
}

#[test]
fn advanced_frequency_and_usual_match_classic() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    data["advanced"] = json!(true);
    let rev = prepare(&store, &data);
    let q = ChangeQuery {
        method: "range".into(),
        from: "2024-02-28".into(),
        to: "2024-03-01".into(),
        percent: 20.,
        min_count: 1.,
        min_amount: 1.,
    };
    for orgs in [&["A"][..], &["A", "B"][..]] {
        let names = ["Group", "zero", "gone", "missing"];
        let old = golden(&data, "day", orgs, Some(&names), &[], &q);
        let scope = scope(&store, rev, "day", orgs, Some(&names));
        for mode in ["usual", "regularity"] {
            let page = store
                .advanced_patterns(rev, &scope, &q, None, mode, 4, None)
                .unwrap();
            assert_eq!(normalized(json!(page.used)), old[mode]["used"]);
            let mut actual = page.rows;
            let mut expected = old[mode]["rows"].as_array().unwrap().clone();
            for rows in [&mut actual, &mut expected] {
                rows.sort_by_key(|v| v["key"].to_string());
            }
            assert_eq!(normalized(json!(actual)), normalized(json!(expected)));
        }
    }
}

#[test]
fn structure_and_contributions_match_standalone_calculations() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    data["advanced"] = json!(true);
    let rev = prepare(&store, &data);
    let q = ChangeQuery {
        method: "range".into(),
        from: "2024-02-29".into(),
        to: "2024-02-29".into(),
        percent: 30.,
        min_count: 0.,
        min_amount: 0.,
    };
    let before = RangeQuery {
        from: "2024-02-28".into(),
        to: "2024-02-28".into(),
    };
    let s = scope(&store, rev, "day", &["A", "B"], None);
    let old = golden(
        &data,
        "day",
        &["A", "B"],
        None,
        &[(&before.from, &before.to), (&q.from, &q.to)],
        &q,
    );
    let structure = store
        .advanced_composition(rev, &s, &q, None, &before, "structure", "amount", None)
        .unwrap();
    let mut rows = structure["rows"].as_array().unwrap().clone();
    let mut expected = old["structure"]["rows"].as_array().unwrap().clone();
    for list in [&mut rows, &mut expected] {
        list.sort_by_key(|row| row["key"].to_string());
    }
    for (row, want) in rows.iter_mut().zip(expected.iter_mut()) {
        for field in [
            "base",
            "current",
            "baseShare",
            "share",
            "cumulative",
            "shareChange",
        ] {
            let a = row[field].as_f64().unwrap();
            let b = want[field].as_f64().unwrap();
            assert!((a - b).abs() < 1e-10, "{field}: {a} vs {b}");
            row.as_object_mut().unwrap().remove(field);
            want.as_object_mut().unwrap().remove(field);
        }
    }
    assert_eq!(rows, expected);
    let result = store
        .advanced_composition(rev, &s, &q, None, &before, "contributions", "amount", None)
        .unwrap();
    assert_eq!(
        normalized(result["check"].clone()),
        old["contributions"]["amount"]
    );
    for row in result["rows"].as_array().unwrap() {
        let original = old["contributions"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["key"] == row["key"])
            .unwrap();
        assert_eq!(
            normalized(row["delta"].clone()),
            original["amount"]["delta"]
        );
    }
}

#[test]
fn complete_daily_months_match_independent_classic_month_reports() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    data["advanced"] = json!(true);
    let reports = data["reports"].as_array_mut().unwrap();
    reports.clear();
    for (month, days, volume) in [(1, 31, 1), (2, 29, 3), (3, 31, 0)] {
        for day in 1..=days {
            let start = format!("2024-{month:02}-{day:02}");
            reports.push(json!({"id":start,"type":"day","start":start,"end":start,"organizations":["A"],"rows":[{"organization":"A","work":"one","count":volume,"amount":10}]}));
        }
        reports.push(json!({"id":format!("m{month}"),"type":"month","start":format!("2024-{month:02}-01"),"end":format!("2024-{month:02}-{days:02}"),"organizations":["A"],"rows":[{"organization":"A","work":"one","count":volume*days,"amount":10*days}]}));
    }
    let rev = prepare(&store, &data);
    let scope = scope(&store, rev, "day", &["A"], None);
    let mut q = ChangeQuery {
        method: "range".into(),
        from: "2024-01-01".into(),
        to: "2024-03-01".into(),
        percent: 30.,
        min_count: 0.,
        min_amount: 0.,
    };
    let expected = golden(&data, "month", &["A"], None, &[], &q);
    q.to = "2024-03-31".into();
    for mode in ["usual", "regularity"] {
        let page = store
            .advanced_patterns(rev, &scope, &q, Some("month"), mode, 4, None)
            .unwrap();
        assert_eq!(normalized(json!(page.rows)), expected[mode]["rows"]);
    }
    let page = store
        .advanced_changes_page(rev, &scope, &q, Some("month"), None)
        .unwrap();
    assert_eq!(normalized(json!(page.rows)), expected["changes"]);
    let file = dir.path().join("changes.csv");
    store
        .advanced_changes_csv(rev, &scope, &q, Some("month"), &file)
        .unwrap();
    let csv=fs::read_to_string(file).unwrap();
    assert!(csv.ends_with(expected["changesCsv"].as_str().unwrap().trim_start_matches('\u{feff}')));
    q.from = "2024-01-01".into();
    q.to = "2024-12-31".into();
    let year = store
        .advanced_patterns(rev, &scope, &q, Some("year"), "regularity", 8, None)
        .unwrap();
    assert_eq!((year.used, year.expected), (0, 1));
    assert_eq!(year.rows[0]["frequency"], Value::Null);
}

#[test]
fn advanced_keyset_pages_and_csv_cover_every_selected_descriptor() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    for (i, r) in data["reports"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        r["rows"]=json!((0..63).map(|k|json!({"organization":"A","work":format!("work-{k:02}"),"count":1,"amount":(i+1)*k})).collect::<Vec<_>>());
    }
    let rev = prepare(&store, &data);
    let s = scope(&store, rev, "day", &["A"], None);
    let q = ChangeQuery {
        method: "previous".into(),
        from: "2024-02-29".into(),
        to: "2024-02-29".into(),
        percent: 30.,
        min_count: 0.,
        min_amount: 0.,
    };
    let first = store
        .advanced_changes_page(rev, &s, &q, None, None)
        .unwrap();
    let next = store
        .advanced_changes_page(rev, &s, &q, None, first.next.as_ref())
        .unwrap();
    assert_eq!((first.rows.len(), next.rows.len()), (50, 13));
    assert!(next.next.is_none());
    assert!(first.rows.last().unwrap().key < next.rows[0].key);
    let file = dir.path().join("all.csv");
    store
        .advanced_changes_csv(rev, &s, &q, None, &file)
        .unwrap();
    assert_eq!(fs::read_to_string(file).unwrap().lines().skip_while(|line| !line.starts_with("\"key\";")).count(), 64);
    let p = store
        .advanced_patterns(rev, &s, &q, None, "usual", 8, None)
        .unwrap();
    let n = store
        .advanced_patterns(rev, &s, &q, None, "usual", 8, p.next.as_deref())
        .unwrap();
    assert_eq!((p.rows.len(), n.rows.len()), (50, 14));
    assert_eq!((p.total_rows,n.total_rows),(64,64));
    assert!(n.next.is_none());
    let before = RangeQuery {
        from: "2024-02-28".into(),
        to: "2024-02-28".into(),
    };
    let first = store
        .advanced_composition(rev, &s, &q, None, &before, "contributions", "amount", None)
        .unwrap();
    let cursor: ContributionCursor = serde_json::from_value(first["next"].clone()).unwrap();
    let last = store
        .advanced_composition(
            rev,
            &s,
            &q,
            None,
            &before,
            "contributions",
            "amount",
            Some(&cursor),
        )
        .unwrap();
    assert_eq!(first["rows"].as_array().unwrap().len(), 50);
    assert_eq!(last["rows"].as_array().unwrap().len(), 12);
    assert_eq!(last["total_rows"],62);
    assert!(last["next"].is_null());
}

#[test]
fn russian_collation_selects_classic_structure_remainder_at_cutoff() {
    for (left, right) in [("Ёж", "Ева"), ("Ёж", "Е\u{308}ж")] {
        let dir = tempdir().unwrap();
        let store = Storage::new(dir.path().into()).unwrap();
        let mut data = fixture();
        data["advanced"] = json!(true);
        let reports = data["reports"].as_array_mut().unwrap();
        reports.truncate(2);
        for (i, report) in reports.iter_mut().enumerate() {
            let mut rows:Vec<_>=(0..11).map(|k|json!({"organization":"A","work":format!("Крупная {k}"),"count":1,"amount":100+k})).collect();
            for (name, base, current) in [(left, 90, 1), (right, 10, -1)] {
                rows.push(json!({"organization":"A","work":name,"count":1,"amount":if i==0 {base}else{current}}));
            }
            report["rows"] = json!(rows);
        }
        let rev = prepare(&store, &data);
        let scope = scope(&store, rev, "day", &["A"], None);
        let q = ChangeQuery {
            method: "previous".into(),
            from: "2024-02-29".into(),
            to: "2024-02-29".into(),
            percent: 0.,
            min_count: 0.,
            min_amount: 0.,
        };
        let before = RangeQuery {
            from: "2024-02-28".into(),
            to: "2024-02-28".into(),
        };
        let old = golden(
            &data,
            "day",
            &["A"],
            None,
            &[(&before.from, &before.to), (&q.from, &q.to)],
            &q,
        );
        let actual = store
            .advanced_composition(rev, &scope, &q, None, &before, "structure", "amount", None)
            .unwrap();
        assert_eq!(
            actual["rows"][11]["name"],
            old["structure"]["rows"][11]["name"]
        );
        for field in ["base", "current", "baseShare", "share", "shareChange"] {
            assert_eq!(
                normalized(actual["rows"][12][field].clone()),
                old["structure"]["rows"][12][field]
            );
        }
    }
}

#[test]
fn grouped_changes_do_not_infer_work_absence_from_missing_daily_reports() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    data["reports"] = json!([
        {"id":"before","type":"day","start":"2024-01-01","end":"2024-01-01",
         "organizations":["A"],"rows":[{"organization":"A","work":"Old","count":2,"amount":20}]},
        {"id":"after","type":"day","start":"2024-01-08","end":"2024-01-08",
         "organizations":["A"],"rows":[{"organization":"A","work":"New","count":3,"amount":30}]}
    ]);
    let rev = prepare(&store, &data);
    let scope = scope(&store, rev, "day", &["A"], None);
    let q = changes("2024-01-01", "2024-01-14", "range");
    let mut options = UnifiedChangeOptions {
        metrics: vec!["amount".into()], mode: "all".into(), search: String::new(),
        direction: "all".into(), sort: "name".into(),
    };
    let page = store.unified_changes_page(rev, &scope, &q, Some("week"), &options, None).unwrap();
    assert_eq!((page.summary.totals.missing_pairs, page.summary.totals.partial_pairs), (0, 1));
    assert_eq!((page.summary.new, page.summary.gone), (0, 0));
    assert!(page.rows.iter().all(|row| row.status == "existing"));
    assert_eq!(page.rows.len(), 2);
    for category in ["new", "gone"] {
        options.direction = category.into();
        let page = store.unified_changes_page(rev, &scope, &q, Some("week"), &options, None).unwrap();
        assert!(page.rows.is_empty(), "{category}");
    }
}
