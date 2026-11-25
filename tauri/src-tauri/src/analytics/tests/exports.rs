use super::*;

#[test]
fn real_js_ranges_changes_full_pages_exports() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let data = fixture();
    let rev = prepare(&store, &data);
    let changes = ChangeQuery {
        method: "range".into(),
        from: "2024-02-28".into(),
        to: "2024-03-01".into(),
        percent: 20.,
        min_count: 2.,
        min_amount: 15.,
    };
    let ranges = [("2024-02-28", "2024-02-29"), ("2024-03-01", "2024-03-01")];
    for names in [None, Some(&["Group", "zero"][..])] {
        let scope = scope(&store, rev, "day", &["A", "B"], names);
        let old = golden(&data, "day", &["A", "B"], names, &ranges, &changes);
        for (i, (from, to)) in ranges.iter().enumerate() {
            let query = RangeQuery {
                from: from.to_string(),
                to: to.to_string(),
            };
            let summary = store.analysis_range_summary(rev, &scope, &query).unwrap();
            assert_eq!(json!(summary.total), old["ranges"][i]["total"]);
            assert_eq!(json!(summary.complete), old["ranges"][i]["complete"]);
            let mut values = vec![];
            let mut after = None;
            loop {
                let page = store
                    .analysis_range_page(rev, &scope, &query, None, after.as_deref(), 1)
                    .unwrap();
                if page.is_empty() {
                    break;
                }
                after = Some(page[0].key.clone());
                values.extend(page);
            }
            assert_eq!(json!(values), old["ranges"][i]["values"]);
            assert_eq!(
                json!(store
                    .analysis_missing_page(rev, &scope, &query, None, 500)
                    .unwrap()),
                old["ranges"][i]["missing"]
            );
        }
        let mut values = vec![];
        let mut after = None;
        loop {
            let page = store
                .analysis_changes_page(rev, &scope, &changes, after.as_ref(), 1)
                .unwrap();
            if page.is_empty() {
                break;
            }
            after = Some(page[0].cursor());
            values.extend(page);
        }
        assert_eq!(json!(values), old["changes"]);
        let summary = store
            .analysis_changes_summary(rev, &scope, &changes)
            .unwrap();
        assert_eq!(summary.rows, values.len() as i64);
        let file = dir.path().join(if names.is_some() {
            "filtered.csv"
        } else {
            "all.csv"
        });
        store
            .analysis_export_changes(rev, &scope, &changes, &file)
            .unwrap();
        let csv = fs::read_to_string(&file).unwrap();
        assert_eq!(csv.lines().count(), values.len() + 1);
        assert!(store
            .analysis_export_changes(rev, &scope, &changes, &file)
            .is_err());
    }
}

#[test]
fn real_js_csv_escaping_numbers_full_range_changes_and_comparison() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    for r in data["reports"].as_array_mut().unwrap() {
        for row in r["rows"].as_array_mut().unwrap() {
            if row["work"] == "zero" {
                row["work"] = json!("  =formula;\"\nline");
                row["amount"] = json!(1.25e-7);
            }
        }
    }
    let rev = prepare(&store, &data);
    let s = scope(&store, rev, "day", &["A", "B"], None);
    let q = changes("2024-02-28", "2024-03-01", "range");
    let ranges = [
        ("2024-02-28", "2024-02-28"),
        ("2024-02-29", "2024-02-29"),
        ("2024-03-01", "2024-03-01"),
    ];
    let old = golden(&data, "day", &["A", "B"], None, &ranges, &q);
    for (i, (a, b)) in ranges.iter().enumerate() {
        let file = dir.path().join(format!("range-{i}.csv"));
        store
            .analysis_export_range(rev, &s, &query(a, b), None, &file)
            .unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            old["rangeCsv"][i].as_str().unwrap()
        );
        assert!(store
            .analysis_export_range(rev, &s, &query(a, b), None, &file)
            .is_err());
    }
    let file = dir.path().join("changes.csv");
    store.analysis_export_changes(rev, &s, &q, &file).unwrap();
    assert_eq!(
        fs::read_to_string(file).unwrap(),
        old["changesCsv"].as_str().unwrap()
    );
    let cq = ComparisonQuery {
        before: query(ranges[0].0, ranges[0].1),
        after: query(ranges[1].0, ranges[1].1),
        percent: q.percent,
        min_count: q.min_count,
        min_amount: q.min_amount,
    };
    assert_eq!(
        json!(store
            .analysis_comparison_page(rev, &s, &cq, None, None, 500)
            .unwrap()),
        old["comparison"]
    );
    let sentinel = dir.path().join("sentinel");
    fs::write(&sentinel, "untouched").unwrap();
    assert!(store
        .analysis_export_range(rev, &s, &query(ranges[0].0, ranges[0].1), None, &sentinel)
        .is_err());
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), "untouched");
    #[cfg(unix)]
    {
        let link = dir.path().join("link.csv");
        std::os::unix::fs::symlink(&sentinel, &link).unwrap();
        assert!(store.analysis_export_changes(rev, &s, &q, &link).is_err());
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "untouched");
    }
}

#[test]
fn bounded_generated_keys_and_atomic_export_destination_race() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    let work = "z".repeat(65536);
    data["reports"][0]["rows"][0]["work"] = json!(work);
    let rev = prepare(&store, &data);
    let s = scope(&store, rev, "day", &["A"], None);
    let q = query("2024-02-28", "2024-02-28");
    let page = store
        .analysis_range_page(rev, &s, &q, None, None, 500)
        .unwrap();
    let last = page.last().unwrap();
    assert!(last.key.len() > 65536);
    assert!(store
        .analysis_range_page(rev, &s, &q, None, Some(&last.key), 1)
        .unwrap()
        .is_empty());
    let path = dir.path().join("race.csv");
    assert!(store
        .analysis_export_range_before_publish(rev, &s, &q, None, &path, || {
            fs::write(&path, "won race").unwrap();
        })
        .is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "won race");
    assert_eq!(
        fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|f| f
                .file_name()
                .to_string_lossy()
                .starts_with(".analiz-export-"))
            .count(),
        0
    );
}

#[test]
fn old_js_csv_unicode_formula_whitespace_and_exponent_boundaries() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    for (work, amount) in [
        ("\u{feff}=bom", 1e21),
        ("\u{85}=nel", 1e-6),
        ("@quoted;\"", -1e21),
    ] {
        data["reports"][0]["rows"]
            .as_array_mut()
            .unwrap()
            .push(json!({"organization":"A","work":work,"count":1,"amount":amount}));
    }
    let rev = prepare(&store, &data);
    let s = scope(&store, rev, "day", &["A"], None);
    let ranges = [("2024-02-28", "2024-02-28")];
    let old = golden(
        &data,
        "day",
        &["A"],
        None,
        &ranges,
        &changes(ranges[0].0, ranges[0].1, "previous"),
    );
    let file = dir.path().join("unicode.csv");
    store
        .analysis_export_range(rev, &s, &query(ranges[0].0, ranges[0].1), None, &file)
        .unwrap();
    assert_eq!(
        fs::read_to_string(file).unwrap(),
        old["rangeCsv"][0].as_str().unwrap()
    );
}

#[test]
fn export_uses_its_revision_snapshot_and_numeric_overflow_never_publishes() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let data = fixture();
    let rev = prepare(&store, &data);
    let s = scope(&store, rev, "day", &["A"], None);
    let q = query("2024-02-28", "2024-02-28");
    let file = dir.path().join("snapshot.csv");
    let c = store.connect().unwrap();
    store
        .analysis_export_range_before_publish(rev, &s, &q, None, &file, || {
            c.execute("UPDATE reports SET json=json WHERE id='a'", [])
                .unwrap();
        })
        .unwrap();
    assert!(fs::read_to_string(file).unwrap().contains("\"20\""));
    assert!(store
        .analysis_range_summary(rev, &s, &q)
        .unwrap_err()
        .contains("Stale"));
    let mut data = fixture();
    for row in data["reports"][0]["rows"].as_array_mut().unwrap() {
        row["count"] = json!(1e308);
        row["amount"] = json!(1e308);
    }
    let rev = prepare(&store, &data);
    let s = scope(&store, rev, "day", &["A"], None);
    assert!(store
        .analysis_range_summary(rev, &s, &q)
        .unwrap_err()
        .contains("overflow"));
    let path = dir.path().join("overflow.csv");
    assert!(store
        .analysis_export_range(rev, &s, &q, None, &path)
        .is_err());
    assert!(!path.exists());
}

#[test]
fn missing_changes_export_captures_scope_and_source_even_without_data_rows() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let rev = prepare(&store, &fixture());
    for (source, kind, grouping, to) in [
        ("daily", "day", Some("month"), "2024-05-31"),
        ("imported", "month", None, "2024-05-01"),
    ] {
        let s = scope(
            &store,
            rev,
            kind,
            &["A", "B"],
            Some(&["=untrusted", "zero"]),
        );
        let q = ChangeQuery {
            method: "previous".into(),
            from: "2024-05-01".into(),
            to: to.into(),
            percent: 37.,
            min_count: 2.,
            min_amount: 15.,
        };
        let file = dir.path().join(format!("{source}.csv"));
        store
            .advanced_changes_csv(rev, &s, &q, grouping, &file)
            .unwrap();
        let text = fs::read_to_string(&file).unwrap();
        for cell in [
            format!("\"source\";\"{source}\""),
            "\"displayPeriod\";\"month\"".into(),
            "\"method\";\"previous\"".into(),
            "\"percentThreshold\";\"37\"".into(),
            "\"minCount\";\"2\"".into(),
            "\"minAmount\";\"15\"".into(),
            "\"organization\";\"B\"".into(),
            "\"selectedName\";\"'=untrusted\"".into(),
            "\"missingPairs\";\"2\"".into(),
            "\"comparableRows\";\"0\"".into(),
            format!("\"to\";\"{to}\""),
        ] {
            assert!(text.contains(&cell), "{cell}");
        }
        assert!(store
            .advanced_changes_csv(rev, &s, &q, grouping, &file)
            .is_err());
    }
}
