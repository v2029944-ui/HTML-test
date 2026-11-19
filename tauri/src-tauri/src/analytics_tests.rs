use super::*;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::tempdir;
fn fixture() -> Value {
    let row = |org: &str, work: &str, count: f64, amount: f64| json!({"organization":org,"work":work,"count":count,"amount":amount});
    let reports = vec![
        json!({"id":"a","type":"day","start":"2024-02-28","end":"2024-02-28","organizations":["A","B"],"rows":[row("A","one",2.,20.),row("A","two",-3.,30.),row("A","zero",0.,9.),row("A","gone",1.,10.)]}),
        json!({"id":"b","type":"day","start":"2024-02-29","end":"2024-02-29","organizations":["A","B"],"rows":[row("A","one",4.,80.),row("A","two",-2.,40.),row("A","zero",0.,0.),row("A","new",2.,50.)]}),
        json!({"id":"c","type":"day","start":"2024-03-01","end":"2024-03-01","organizations":["A"],"rows":[row("A","one",1.,10.)]}),
    ];
    json!({"reports":reports,"settings":{"orgNames":["A","B","C","D"],"catalog":[],"groups":[{"id":"g","name":"Group","works":["one"]},{"id":"later","name":"Ignored","works":["one","two"]}]}})
}
fn normalized(v: Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.into_iter().map(normalized).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, normalized(v))).collect()),
        v => v,
    }
}
fn golden(
    data: &Value,
    kind: &str,
    orgs: &[&str],
    names: Option<&[&str]>,
    ranges: &[(&str, &str)],
    changes: &ChangeQuery,
) -> Value {
    let input = json!({"reports":data["reports"],"settings":data["settings"],"scope":{"type":kind,"organizations":orgs,"names":names},"ranges":ranges.iter().map(|(from,to)|json!({"from":from,"to":to})).collect::<Vec<_>>(),"changes":{"method":changes.method,"from":changes.from,"to":changes.to,"percent":changes.percent,"minCount":changes.min_count,"minAmount":changes.min_amount}});
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/native_analytics_golden.cjs");
    let mut child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    normalized(serde_json::from_slice(&result.stdout).unwrap())
}
fn scope(
    store: &Storage,
    rev: i64,
    kind: &str,
    orgs: &[&str],
    names: Option<&[&str]>,
) -> AnalysisScope {
    fn filter(s: &Storage, r: i64, kind: &str, items: &[&str]) -> String {
        let id = s.analysis_filter_create(r, kind).unwrap();
        s.analysis_filter_append(
            r,
            &id,
            &items.iter().map(|x| x.to_string()).collect::<Vec<_>>(),
        )
        .unwrap();
        s.analysis_filter_seal(r, &id).unwrap();
        id
    }
    AnalysisScope {
        report_type: kind.into(),
        organizations: filter(store, rev, "organizations", orgs),
        names: names.map(|n| filter(store, rev, "names", n)),
    }
}
fn prepare(store: &Storage, data: &Value) -> i64 {
    store.save(data.clone()).unwrap();
    let rev = store.revision().unwrap();
    for report in data["reports"].as_array().unwrap() {
        store
            .analysis_index_report(report["id"].as_str().unwrap(), rev)
            .unwrap();
    }
    store.analysis_index_settings(rev).unwrap();
    rev
}
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
fn query(from: &str, to: &str) -> RangeQuery {
    RangeQuery {
        from: from.into(),
        to: to.into(),
    }
}
fn changes(from: &str, to: &str, method: &str) -> ChangeQuery {
    ChangeQuery {
        method: method.into(),
        from: from.into(),
        to: to.into(),
        percent: 25.,
        min_count: 3.,
        min_amount: 20.,
    }
}
fn check_old(
    store: &Storage,
    rev: i64,
    data: &Value,
    kind: &str,
    orgs: &[&str],
    ranges: &[(&str, &str)],
    q: &ChangeQuery,
) {
    let s = scope(store, rev, kind, orgs, None);
    let old = golden(data, kind, orgs, None, ranges, q);
    for (i, (a, b)) in ranges.iter().enumerate() {
        let q = query(a, b);
        let sum = store.analysis_range_summary(rev, &s, &q).unwrap();
        assert_eq!(json!(sum.total), old["ranges"][i]["total"]);
        assert_eq!(json!(sum.complete), old["ranges"][i]["complete"]);
        assert_eq!(
            json!(store
                .analysis_range_page(rev, &s, &q, None, None, 500)
                .unwrap()),
            old["ranges"][i]["values"]
        );
    }
    let mut kinds = store
        .analysis_issue_page(rev, &s, None, 500)
        .unwrap()
        .into_iter()
        .map(|i| i.kind)
        .collect::<Vec<_>>();
    kinds.sort();
    assert_eq!(json!(kinds), old["issues"]);
    assert_eq!(
        json!(store.analysis_changes_page(rev, &s, q, None, 500).unwrap()),
        old["changes"]
    );
}
#[test]
fn old_js_overlap_both_sides_disjoint_orgs_and_invalid_legacy() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let report = |id: &str, start: &str, stop: &str, org: &str| json!({"id":id,"type":"week","start":start,"end":stop,"organizations":[org],"rows":[{"organization":org,"work":"W","count":1,"amount":10}]});
    let mut data = json!({"reports":[report("outside","2026-11-02","2026-11-08","A"),report("shifted","2026-11-08","2026-11-14","A"),report("disjoint","2026-11-08","2026-11-14","B"),report("duplicate1","2026-11-16","2026-11-22","A"),report("duplicate2","2026-11-16","2026-11-22","A"),report("invalidbound","2026-11-23","2026-11-28","A"),report("invalidrow","2026-11-30","2026-12-06","A")],"settings":fixture()["settings"]});
    data["reports"][6]["rows"][0]["count"] = json!("bad");
    // Legacy writers can store invalid rows, although normal save() rejects them.
    let rev0 = store.revision().unwrap();
    let c = store.connect().unwrap();
    for r in data["reports"].as_array().unwrap() {
        c.execute("INSERT INTO reports(id,report_type,period_start,period_end,json) VALUES(?1,?2,?3,?4,?5)",params![r["id"].as_str(),r["type"].as_str(),r["start"].as_str(),r["end"].as_str(),r.to_string()]).unwrap();
    }
    c.execute(
        "INSERT INTO kv VALUES('settings',?1)",
        [data["settings"].to_string()],
    )
    .unwrap();
    drop(c);
    let rev = store.revision().unwrap();
    assert!(rev > rev0);
    for r in data["reports"].as_array().unwrap() {
        store
            .analysis_index_report(r["id"].as_str().unwrap(), rev)
            .unwrap();
    }
    store.analysis_index_settings(rev).unwrap();
    check_old(
        &store,
        rev,
        &data,
        "week",
        &["A", "B"],
        &[
            ("2026-11-08", "2026-11-08"),
            ("2026-11-16", "2026-11-16"),
            ("2026-11-30", "2026-11-30"),
        ],
        &changes("2026-11-02", "2026-12-07", "range"),
    );
    let s = scope(&store, rev, "week", &["B"], None);
    let t = store
        .analysis_range_summary(rev, &s, &query("2026-11-08", "2026-11-08"))
        .unwrap();
    assert!(t.complete);
    assert_eq!(t.total.amount, Some(10.));
}
#[test]
fn old_js_month_leap_week_shifted_and_previous_negative_average() {
    for kind in ["day", "week", "month"] {
        let dir = tempdir().unwrap();
        let store = Storage::new(dir.path().into()).unwrap();
        let mut data = fixture();
        let starts = match kind {
            "month" => ["2024-01-01", "2024-02-01", "2024-03-01"],
            "week" => ["2024-02-19", "2024-02-26", "2024-03-04"],
            _ => ["2024-02-28", "2024-02-29", "2024-03-01"],
        };
        let ends = match kind {
            "month" => ["2024-01-31", "2024-02-29", "2024-03-31"],
            "week" => ["2024-02-25", "2024-03-03", "2024-03-10"],
            _ => starts,
        };
        for (i, r) in data["reports"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            r["type"] = json!(kind);
            r["start"] = json!(starts[i]);
            r["end"] = json!(ends[i]);
        }
        let rev = prepare(&store, &data);
        check_old(
            &store,
            rev,
            &data,
            kind,
            &["A", "B"],
            &[(starts[0], starts[1]), (starts[0], starts[2])],
            &changes(starts[1], starts[1], "previous"),
        );
        let s = scope(&store, rev, kind, &["A"], None);
        for (from, to) in [("2024-02-30", "2024-03-01"), ("2024-03-01", "2024-02-01")] {
            assert!(store
                .analysis_range_summary(rev, &s, &query(from, to))
                .is_err());
        }
        if kind == "week" {
            assert!(store
                .analysis_range_summary(rev, &s, &query("2024-02-19", "2024-02-27"))
                .unwrap_err()
                .contains("sequence"));
        }
        if kind == "month" {
            assert!(store
                .analysis_range_summary(rev, &s, &query("2024-02-02", "2024-03-01"))
                .is_err());
        }
        let t = store
            .analysis_range_page(rev, &s, &query(starts[0], starts[0]), None, None, 500)
            .unwrap();
        assert_eq!(
            t.iter().find(|x| x.key == "group:later").unwrap().average,
            None
        );
        assert_eq!(
            t.iter().find(|x| x.key == "work:zero").unwrap().average,
            None
        );
    }
}
#[test]
fn revisions_text_blob_corruption_and_concurrent_publication() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let data = fixture();
    let rev = prepare(&store, &data);
    let s = scope(&store, rev, "day", &["A", "B"], None);
    let c = store.connect().unwrap();
    c.execute(
        "UPDATE reports SET json=CAST(json AS BLOB) WHERE id='a'",
        [],
    )
    .unwrap();
    let new = store.revision().unwrap();
    assert!(store
        .analysis_range_summary(rev, &s, &query("2024-02-28", "2024-02-29"))
        .unwrap_err()
        .contains("Stale"));
    let s = scope(&store, new, "day", &["A", "B"], None);
    assert!(store
        .analysis_range_summary(new, &s, &query("2024-02-28", "2024-02-29"))
        .unwrap_err()
        .contains("unindexed"));
    store.analysis_index_report("a", new).unwrap();
    assert!(
        store
            .analysis_range_summary(new, &s, &query("2024-02-28", "2024-02-29"))
            .unwrap()
            .complete
    );
    c.execute("UPDATE reports SET json=json WHERE id='b'", [])
        .unwrap();
    let rev = store.revision().unwrap();
    let result = store.analysis_index_report_before_publish("b", rev, || {
        c.execute("UPDATE reports SET json=json WHERE id='c'", [])
            .unwrap();
    });
    assert!(result.is_err());
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM analysis_reports WHERE report_id='b'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    let rev = store.revision().unwrap();
    store.analysis_index_report("b", rev).unwrap();
    store.analysis_index_report("c", rev).unwrap();
    c.execute("UPDATE kv SET json=json WHERE name='settings'", [])
        .unwrap();
    let rev = store.revision().unwrap();
    let s = scope(&store, rev, "day", &["A"], None);
    assert!(store
        .analysis_range_summary(rev, &s, &query("2024-02-28", "2024-02-29"))
        .unwrap_err()
        .contains("settings unindexed"));
    store.analysis_index_settings(rev).unwrap();
    c.execute("UPDATE reports SET json=?1", ["{"]).unwrap();
    let rev = store.revision().unwrap();
    assert!(store.analysis_index_report("a", rev).is_err());
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM analysis_reports", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn disk_filters_limits_empty_missing_and_large_batches() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let rev = prepare(&store, &fixture());
    let org = store.analysis_filter_create(rev, "organizations").unwrap();
    let q = query("2024-02-28", "2024-02-29");
    let s = AnalysisScope {
        report_type: "day".into(),
        organizations: org.clone(),
        names: None,
    };
    assert!(store.analysis_range_summary(rev, &s, &q).is_err());
    store.analysis_filter_seal(rev, &org).unwrap();
    assert!(!store.analysis_range_summary(rev, &s, &q).unwrap().complete);
    assert!(store
        .analysis_filter_append(rev, &org, &["A".into()])
        .is_err());
    let id = store.analysis_filter_create(rev, "names").unwrap();
    assert!(store
        .analysis_filter_append(rev, &id, &vec!["x".into(); 501])
        .is_err());
    assert!(store
        .analysis_filter_append(rev, &id, &["x".repeat(65537)])
        .is_err());
    for i in 0..5 {
        store
            .analysis_filter_append(
                rev,
                &id,
                &(0..500)
                    .map(|n| format!("unused-{i}-{n}"))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    store
        .analysis_filter_append(rev, &id, &["Group".into()])
        .unwrap();
    store.analysis_filter_seal(rev, &id).unwrap();
    let mut s = scope(&store, rev, "day", &["A", "B", "Absent"], None);
    s.names = Some(id);
    let summary = store.analysis_range_summary(rev, &s, &q).unwrap();
    assert_eq!(summary.missing, 2);
    assert_eq!(summary.values, 1);
    assert_eq!(summary.total.amount, None);
    assert!(store
        .analysis_range_page(rev, &s, &q, None, None, 0)
        .is_err());
    assert!(store
        .analysis_range_page(rev, &s, &q, None, None, 501)
        .is_err());
    assert!(store
        .analysis_range_page(rev, &s, &q, Some("Outside"), None, 1)
        .is_err());
    let orgs = store
        .analysis_organization_page(rev, &s, &q, None, 500)
        .unwrap();
    assert_eq!(
        orgs.iter().find(|o| o.org == "B").unwrap().total.count,
        Some(0.)
    );
    assert!(!orgs.iter().find(|o| o.org == "Absent").unwrap().complete);
}
#[test]
fn comparisons_full_totals_org_scope_and_incomplete_nulls() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let rev = prepare(&store, &fixture());
    let s = scope(&store, rev, "day", &["A", "B"], None);
    let mut q = ComparisonQuery {
        before: query("2024-02-28", "2024-02-28"),
        after: query("2024-02-29", "2024-02-29"),
        percent: 0.,
        min_count: 0.,
        min_amount: 0.,
    };
    let sum = store.analysis_comparison_summary(rev, &s, &q).unwrap();
    assert_eq!(sum.values, 5);
    assert_eq!(sum.count.as_ref().unwrap().delta, 4.);
    assert_eq!(sum.amount.as_ref().unwrap().delta, 101.);
    let first = store
        .analysis_comparison_page(rev, &s, &q, None, None, 1)
        .unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(
        store
            .analysis_comparison_page(rev, &s, &q, None, Some(&first[0].key), 500)
            .unwrap()
            .len(),
        4
    );
    let org = store
        .analysis_comparison_page(rev, &s, &q, Some("B"), None, 500)
        .unwrap();
    assert!(org.is_empty());
    let file = dir.path().join("compare.csv");
    store
        .analysis_export_comparison(rev, &s, &q, None, &file)
        .unwrap();
    assert_eq!(fs::read_to_string(file).unwrap().lines().count(), 6);
    q.after = query("2024-03-01", "2024-03-01");
    assert!(store
        .analysis_comparison_summary(rev, &s, &q)
        .unwrap()
        .count
        .is_none());
    for v in store
        .analysis_comparison_page(rev, &s, &q, None, None, 500)
        .unwrap()
    {
        assert!(v.count.is_none());
        assert!(v.amount.is_none());
        assert_eq!(v.status, None);
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
fn changes_complete_scalars_and_gap_pages_include_later_pairs() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let data = fixture();
    let rev = prepare(&store, &data);
    let s = scope(&store, rev, "day", &["A", "B"], None);
    let q = changes("2024-02-28", "2024-03-01", "range");
    let old = golden(&data, "day", &["A", "B"], None, &[], &q);
    let rows = old["changes"].as_array().unwrap();
    let sum = store.analysis_changes_summary(rev, &s, &q).unwrap();
    let total = |metric: &str, field: &str| {
        rows.iter()
            .map(|r| r[metric][field].as_f64().unwrap())
            .sum::<f64>()
    };
    assert_eq!(sum.count.base, total("count", "base"));
    assert_eq!(sum.count.current, total("count", "current"));
    assert_eq!(sum.amount.delta, total("amount", "delta"));
    let pairs = store
        .analysis_change_pair_page(rev, &s, &q, true, None, 1)
        .unwrap();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].org, "B");
    assert_eq!(pairs[0].current_start, "2024-03-01");
    assert!(pairs[0].missing_current);
    assert!(!pairs[0].missing_base);
    assert!(store
        .analysis_change_pair_page(rev, &s, &q, true, Some(&pairs[0].cursor()), 1)
        .unwrap()
        .is_empty());
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
fn old_js_comparison_selected_absent_names_and_display_name_filter() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let data = fixture();
    let rev = prepare(&store, &data);
    let names = ["Group", "group:g", "never seen"];
    let s = scope(&store, rev, "day", &["A", "B"], Some(&names));
    let q = changes("2024-02-28", "2024-02-29", "range");
    let ranges = [("2024-02-28", "2024-02-28"), ("2024-02-29", "2024-02-29")];
    let old = golden(&data, "day", &["A", "B"], Some(&names), &ranges, &q);
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
    assert_eq!(
        store
            .analysis_range_summary(rev, &s, &query(ranges[0].0, ranges[0].1))
            .unwrap()
            .total
            .amount,
        Some(20.)
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
fn corrupt_or_unsupported_settings_are_explicit_and_never_publish_partial_groups() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let rev = prepare(&store, &fixture());
    let c = store.connect().unwrap();
    for raw in ["{\"groups\":[{\"id\":\"x\",\"name\":\"first\",\"works\":[\"one\"]},{\"id\":\"x\",\"name\":\"second\",\"works\":[\"two\"]}]}","{\"groups\":[],\"groups\":[]}"]{c.execute("UPDATE kv SET json=?1 WHERE name='settings'",[raw]).unwrap();let r=store.revision().unwrap();assert!(r>rev);assert!(store.analysis_index_settings(r).is_err());assert_eq!(c.query_row("SELECT COUNT(*) FROM analysis_settings",[],|r|r.get::<_,i64>(0)).unwrap(),0);assert_eq!(c.query_row("SELECT COUNT(*) FROM analysis_groups",[],|r|r.get::<_,i64>(0)).unwrap(),0);}
    let raw = format!("{{\"groups\":[],\"unknown\":\"{}\"}}", "x".repeat(65537));
    c.execute("UPDATE kv SET json=?1 WHERE name='settings'", [&raw])
        .unwrap();
    let r = store.revision().unwrap();
    assert!(store
        .analysis_index_settings(r)
        .unwrap_err()
        .contains("token exceeds"));
    assert_eq!(
        c.query_row("SELECT json FROM kv WHERE name='settings'", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        raw
    );
}
#[test]
fn old_js_nonstring_row_org_ignored_without_publishing_partial_d1_manifest() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    data["reports"][0]["rows"][0]
        .as_object_mut()
        .unwrap()
        .remove("organization");
    let _ = store.revision().unwrap();
    let c = store.connect().unwrap();
    for r in data["reports"].as_array().unwrap() {
        c.execute("INSERT INTO reports(id,report_type,period_start,period_end,json) VALUES(?1,?2,?3,?4,?5)",params![r["id"].as_str(),r["type"].as_str(),r["start"].as_str(),r["end"].as_str(),r.to_string()]).unwrap();
    }
    c.execute(
        "INSERT INTO kv VALUES('settings',?1)",
        [data["settings"].to_string()],
    )
    .unwrap();
    let rev = store.revision().unwrap();
    for r in data["reports"].as_array().unwrap() {
        store
            .analysis_index_report(r["id"].as_str().unwrap(), rev)
            .unwrap();
    }
    store.analysis_index_settings(rev).unwrap();
    check_old(
        &store,
        rev,
        &data,
        "day",
        &["A", "B"],
        &[("2024-02-28", "2024-02-29")],
        &changes("2024-02-28", "2024-02-29", "range"),
    );
    assert!(store.aggregate(rev, &ReadFilter::default()).is_err());
}
#[test]
fn conflict_query_uses_bounded_start_window_index() {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("CREATE TEMP TABLE an_candidates(org TEXT,start TEXT,end TEXT,report_id TEXT,warned INTEGER,PRIMARY KEY(org,start,report_id)) WITHOUT ROWID;CREATE TEMP TABLE an_issues(report_id TEXT,org TEXT,kind TEXT,other_report_id TEXT);").unwrap();
    let mut stmt = c
        .prepare(&format!("EXPLAIN QUERY PLAN {}", analytics::CONFLICT_SQL))
        .unwrap();
    let plans = stmt
        .query_map(["-6 day"], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        plans
            .iter()
            .any(|p| p.contains("SEARCH b USING PRIMARY KEY (org=? AND start>? AND start<?)")),
        "{plans:?}"
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
fn sql_timeline_preserves_missing_and_zero_amounts_without_loading_history() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let rev = prepare(&store, &fixture());
    let selected = scope(&store, rev, "day", &["A", "B"], None);
    let daily = store.analysis_timeline(
        rev, &selected, &query("2024-02-28", "2024-03-01"),
    ).unwrap();
    assert_eq!(daily.granularity, "period");
    assert_eq!(daily.points.len(), 3);
    assert_eq!(daily.points[0].start, "2024-02-28");
    assert_eq!(daily.points[0].total.count, Some(0.));
    assert_eq!(daily.points[0].total.amount, Some(69.));
    assert_eq!(daily.points[1].total.amount, Some(170.));
    assert_eq!(daily.points[2].start, "2024-03-01");
    assert_eq!(daily.points[2].missing, 1); // B is absent; do not render A's 10 as the full total.
    assert_eq!(daily.points[2].total.amount, None);
    assert_eq!(daily.points[2].total.count, None);

    let only_a = scope(&store, rev, "day", &["A"], None);
    let march = store.analysis_timeline(
        rev, &only_a, &query("2024-03-01", "2024-03-01"),
    ).unwrap();
    assert_eq!(march.points.len(), 1);
    assert_eq!(march.points[0].missing, 0);
    assert_eq!(march.points[0].total.amount, Some(10.));

    let year = store.analysis_timeline(
        rev, &selected, &query("2024-01-01", "2024-12-31"),
    ).unwrap();
    assert_eq!(year.granularity, "month");
    assert_eq!(year.points.len(), 12);
    assert_eq!(year.points[0].start, "2024-01");
    assert_eq!(year.points[1].start, "2024-02");
    assert!(year.points.iter().all(|point| point.total.amount.is_none()));
    assert!(year.points.iter().all(|point| point.missing>0));
}


#[test]
fn sql_ranking_is_sorted_bounded_and_complete() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let rev = prepare(&store, &fixture());
    let selected = scope(&store, rev, "day", &["A"], None);
    let days = query("2024-02-28", "2024-02-29");
    let top = store.analysis_ranking(rev, &selected, &days, None, "amount", "all").unwrap();
    assert!(top.complete);
    assert!(!top.rows.is_empty());
    assert!(top.rows.len() <= 12);
    assert_eq!(top.rows[0].kind, "group");
    assert_eq!(top.rows[0].name, "Group");
    assert_eq!(top.rows[0].amount, Some(100.));
    assert_eq!(top.rows[1].name, "Ignored");
    assert_eq!(top.rows[1].amount, Some(70.));

    let counts = store.analysis_ranking(rev, &selected, &days, None, "count", "work").unwrap();
    assert!(counts.complete);
    assert!(counts.rows.iter().all(|r| r.kind == "work"));
    assert_eq!(counts.rows[0].name, "new");
    assert_eq!(counts.rows[0].count, Some(2.));
    assert_eq!(counts.rows[1].name, "gone");
    assert_eq!(counts.rows[1].count, Some(1.));
    let only_groups = store.analysis_ranking(rev, &selected, &days, None, "amount", "group").unwrap();
    assert_eq!(only_groups.rows.len(), 2);
    assert_eq!(only_groups.rows[0].name, "Group");
    assert_eq!(only_groups.rows[1].name, "Ignored");
    assert_eq!(only_groups.rows[1].count, Some(-5.));

    let combined = scope(&store, rev, "day", &["A", "B"], None);
    let incomplete = store.analysis_ranking(
        rev, &combined, &query("2024-02-28", "2024-03-01"), None, "amount", "all"
    ).unwrap();
    assert!(!incomplete.complete);
    assert!(incomplete.rows.is_empty());

    assert!(store.analysis_ranking(rev, &selected, &days, None, "percent", "all").is_err());
    assert!(store.analysis_ranking(rev, &selected, &days, None, "amount", "unknown").is_err());
    assert!(store.analysis_ranking(rev, &selected, &days, Some("B"), "amount", "all").is_err());
}


#[test]
fn organization_trends_are_bounded_and_independently_complete() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let rev = prepare(&store, &fixture());
    let selected = scope(&store, rev, "day", &["A", "B"], None);
    let data = store.analysis_organization_trends(
        rev, &selected, &query("2024-02-28", "2024-03-01"),
    ).unwrap();
    assert_eq!(data.granularity, "period");
    assert_eq!(data.selected_organizations, 2);
    assert_eq!(data.series.len(), 2);
    assert_eq!(data.series[0].organization, "A");
    assert_eq!(data.series[1].organization, "B");
    assert_eq!(data.series[0].points.len(), 3);
    assert_eq!(data.series[1].points.len(), 3);
    assert_eq!(data.series[0].points[0].total.count, Some(0.));
    assert_eq!(data.series[0].points[0].total.amount, Some(69.));
    assert_eq!(data.series[0].points[1].total.amount, Some(170.));
    assert_eq!(data.series[0].points[2].total.amount, Some(10.));
    assert_eq!(data.series[0].points[2].missing, 0);
    // An explicitly present organization with no rows is valid zero, not missing.
    assert_eq!(data.series[1].points[0].total.amount, Some(0.));
    assert_eq!(data.series[1].points[1].total.amount, Some(0.));
    assert_eq!(data.series[1].points[2].missing, 1);
    assert_eq!(data.series[1].points[2].total.amount, None);
    // The missing B report never obscures confirmed data for A.
    assert_eq!(data.series[0].points[2].total.amount, Some(10.));
    let year = store.analysis_organization_trends(
        rev, &selected, &query("2024-01-01", "2024-12-31"),
    ).unwrap();
    assert_eq!(year.granularity, "month");
    assert_eq!(year.series.len(), 2);
    assert_eq!(year.series[0].points.len(), 12);
    assert_eq!(year.series[0].points[0].start, "2024-01");
    assert!(year.series.iter().all(|series| series.points.iter().all(
        |point| point.missing>0 && point.total.amount.is_none()
    )));
}

#[test]
fn organization_trends_explicitly_limits_organization_series() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    // The imported legacy settings schema requires exactly four organization names.
    // Create a synthetic nine-item read-only filter to verify the transport cap,
    // without changing or weakening legacy settings validation.
    let rev = prepare(&store, &fixture());
    let selected = scope(
        &store, rev, "day", &["A","B","C","D","E","F","G","H","I"], None,
    );
    let trends = store.analysis_organization_trends(
        rev, &selected, &query("2024-02-28", "2024-02-29"),
    ).unwrap();
    assert_eq!(trends.selected_organizations, 9);
    assert_eq!(trends.series.len(), 8);
    assert_eq!(trends.series[0].organization, "A");
    assert_eq!(trends.series[7].organization, "H");
    assert!(trends.series.iter().all(|series| series.points.len()==2));
}


#[test]
fn work_timeline_distinguishes_confirmed_absence_from_missing_reports() {
    let dir=tempdir().unwrap();
    let store=Storage::new(dir.path().into()).unwrap();
    let rev=prepare(&store,&fixture());
    let days=query("2024-02-28","2024-03-01");
    let only_a=scope(&store,rev,"day",&["A"],None);
    let zero=store.analysis_work_timeline(
        rev,&only_a,&days,None,"work:zero",
    ).unwrap();
    assert_eq!(zero.kind,"work");
    assert_eq!(zero.name,"zero");
    assert_eq!(zero.granularity,"period");
    assert_eq!(zero.points.len(),3);
    assert_eq!(zero.points[0].total.count,Some(0.));
    assert_eq!(zero.points[0].total.amount,Some(9.));
    assert_eq!(zero.points[1].total.amount,Some(0.));
    // The A report exists on March 1, but contains no row for work 'zero'.
    assert_eq!(zero.points[2].missing,0);
    assert_eq!(zero.points[2].total.amount,Some(0.));

    let group=store.analysis_work_timeline(
        rev,&only_a,&days,None,"group:g",
    ).unwrap();
    assert_eq!(group.kind,"group");
    assert_eq!(group.name,"Group");
    assert_eq!(group.points[0].total.amount,Some(20.));
    assert_eq!(group.points[1].total.amount,Some(80.));
    assert_eq!(group.points[2].total.amount,Some(10.));

    let all=scope(&store,rev,"day",&["A","B"],None);
    let incomplete=store.analysis_work_timeline(
        rev,&all,&days,None,"group:g",
    ).unwrap();
    assert_eq!(incomplete.points[0].missing,0);
    assert_eq!(incomplete.points[0].total.amount,Some(20.));
    assert_eq!(incomplete.points[2].missing,1);
    assert_eq!(incomplete.points[2].total.amount,None);
    assert_eq!(incomplete.points[2].total.count,None);

    let a_only_from_two=store.analysis_work_timeline(
        rev,&all,&days,Some("A"),"group:g",
    ).unwrap();
    assert_eq!(a_only_from_two.points[2].total.amount,Some(10.));
    assert!(store.analysis_work_timeline(
        rev,&only_a,&days,Some("B"),"group:g",
    ).is_err());
    assert!(store.analysis_work_timeline(
        rev,&only_a,&days,None,"work:absent",
    ).is_err());

    let annual=store.analysis_work_timeline(
        rev,&all,&query("2024-01-01","2024-12-31"),None,"group:g",
    ).unwrap();
    assert_eq!(annual.granularity,"month");
    assert_eq!(annual.points.len(),12);
    assert!(annual.points.iter().all(|p| p.missing>0&&p.total.amount.is_none()));
}


#[test]
fn comparison_dashboard_complete_partial_and_signed_changes() {
    let dir=tempdir().unwrap();
    let store=Storage::new(dir.path().into()).unwrap();
    let rev=prepare(&store,&fixture());
    let selected=scope(&store,rev,"day",&["A","B"],None);
    let mut q=ComparisonQuery {
        before:query("2024-02-28","2024-02-28"),
        after:query("2024-02-29","2024-02-29"),
        percent:0.,min_count:0.,min_amount:0.,
    };
    let dash=store.analysis_comparison_dashboard(rev,&selected,&q).unwrap();
    assert!(dash.summary.before.complete);
    assert!(dash.summary.after.complete);
    assert_eq!(dash.summary.values,5);
    assert_eq!(dash.summary.amount.as_ref().unwrap().delta,101.);
    assert_eq!(dash.summary.count.as_ref().unwrap().delta,4.);
    assert_eq!(dash.summary.amount.as_ref().unwrap().percent,Some(101./69.*100.));
    assert_eq!(dash.divisions.len(),2);
    assert_eq!(dash.divisions[0].organization,"A");
    assert_eq!(dash.divisions[0].amount.as_ref().unwrap().delta,101.);
    assert_eq!(dash.divisions[1].organization,"B");
    assert_eq!(dash.divisions[1].amount.as_ref().unwrap().delta,0.);
    assert_eq!(dash.top_amount[0].name,"Group");
    assert_eq!(dash.top_amount[0].amount.as_ref().unwrap().delta,60.);
    assert_eq!(dash.top_amount[1].name,"new");
    assert_eq!(dash.top_amount[1].amount.as_ref().unwrap().delta,50.);
    assert!(dash.top_amount.iter().any(|r|r.name=="zero"
        && r.amount.as_ref().unwrap().delta<0.));
    assert!(dash.top_count.len()<=12);
    q.after=query("2024-03-01","2024-03-01");
    let partial=store.analysis_comparison_dashboard(rev,&selected,&q).unwrap();
    assert!(partial.summary.before.complete);
    assert!(!partial.summary.after.complete);
    assert!(partial.summary.amount.is_none());
    assert!(partial.summary.count.is_none());
    assert!(partial.top_amount.is_empty());
    assert!(partial.top_count.is_empty());
    let a=&partial.divisions[0];
    assert_eq!(a.organization,"A");
    assert_eq!(a.amount.as_ref().unwrap().delta,-59.);
    let b=&partial.divisions[1];
    assert_eq!(b.organization,"B");
    assert_eq!(b.before.amount,Some(0.));
    assert_eq!(b.after.amount,None);
    assert!(b.amount.is_none());
    assert!(b.count.is_none());
    q.after=query("2024-02-30","2024-02-30");
    assert!(store.analysis_comparison_dashboard(rev,&selected,&q).is_err());
}

#[test]
fn comparison_dashboard_ranking_has_fixed_size_and_zero_base_no_percent() {
    let dir=tempdir().unwrap();
    let store=Storage::new(dir.path().into()).unwrap();
    let mut data=fixture();
    for i in 0..30 {
        let name=format!("many-{i:02}");
        data["reports"][1]["rows"].as_array_mut().unwrap().push(
            json!({"organization":"A","work":name,"count":1.,"amount":(i+1) as f64}),
        );
    }
    let rev=prepare(&store,&data);
    let selected=scope(&store,rev,"day",&["A"],None);
    let q=ComparisonQuery {
        before:query("2024-02-28","2024-02-28"),
        after:query("2024-02-29","2024-02-29"),
        percent:0.,min_count:0.,min_amount:0.,
    };
    let result=store.analysis_comparison_dashboard(rev,&selected,&q).unwrap();
    assert_eq!(result.top_amount.len(),12);
    assert_eq!(result.top_count.len(),12);
    assert_eq!(result.top_amount[0].name,"Group");
    assert!(result.top_amount.iter().any(|r|r.name=="many-29"
        && r.amount.as_ref().unwrap().percent.is_none()));
    // Keys from the old and new range are reconciled by canonical group/work key.
    let new=result.top_amount.iter().find(|r|r.name=="many-29").unwrap();
    assert_eq!(new.amount.as_ref().unwrap().base,0.);
    assert_eq!(new.amount.as_ref().unwrap().current,30.);
}


#[test]
fn available_range_totals_do_not_falsify_completeness() {
    let dir=tempdir().unwrap();
    let store=Storage::new(dir.path().into()).unwrap();
    let rev=prepare(&store,&fixture());
    let selected=scope(&store,rev,"day",&["A","B"],None);
    let partial=store.analysis_range_summary(
        rev,&selected,&query("2024-02-28","2024-03-01")
    ).unwrap();
    assert!(!partial.complete);
    assert_eq!(partial.periods,3);
    assert_eq!(partial.organizations,2);
    assert_eq!(partial.missing,1);
    assert_eq!(partial.available_reports,5);
    assert_eq!(partial.total.amount,None);
    assert_eq!(partial.total.count,None);
    assert_eq!(partial.available_total.amount,Some(249.));
    assert_eq!(partial.available_total.count,Some(5.));
    assert_eq!(partial.available_total.average,Some(249./5.));
    let a_only=scope(&store,rev,"day",&["A"],None);
    let full=store.analysis_range_summary(
        rev,&a_only,&query("2024-02-28","2024-03-01")
    ).unwrap();
    assert!(full.complete);
    assert_eq!(full.available_reports,3);
    assert_eq!(full.available_total.amount,full.total.amount);
    assert_eq!(full.available_total.count,full.total.count);
    let none=store.analysis_range_summary(
        rev,&a_only,&query("2024-04-01","2024-04-01")
    ).unwrap();
    assert!(!none.complete);
    assert_eq!(none.available_reports,0);
    assert_eq!(none.available_total.amount,None);
    assert_eq!(none.available_total.count,None);
    assert_eq!(none.total.amount,None);
}


#[test]
fn daily_report_rollups_are_calendar_aligned_and_never_double_count_source_types() {
    let dir=tempdir().unwrap();
    let store=Storage::new(dir.path().into()).unwrap();
    let rev=prepare(&store,&fixture());
    let all=scope(&store,rev,"day",&["A","B"],None);
    let day=store.analysis_daily_rollup(
        rev,&all,&query("2024-02-28","2024-03-01"),"day",None,2,
    ).unwrap();
    assert_eq!(day.points.len(),2);
    assert_eq!(day.next_cursor,Some("2024-02-29".into()));
    assert_eq!(day.points[0].start,"2024-02-28");
    assert_eq!(day.points[0].days,1);
    assert_eq!(day.points[0].available_reports,2);
    assert_eq!(day.points[0].total.amount,Some(69.));
    assert_eq!(day.points[1].total.amount,Some(170.));
    let last=store.analysis_daily_rollup(
        rev,&all,&query("2024-02-28","2024-03-01"),
        "day",day.next_cursor.as_deref(),2,
    ).unwrap();
    assert_eq!(last.points.len(),1);
    assert_eq!(last.next_cursor,None);
    assert_eq!(last.points[0].start,"2024-03-01");
    assert_eq!(last.points[0].available_reports,1);
    assert_eq!(last.points[0].missing,1);
    assert!(!last.points[0].complete);
    assert_eq!(last.points[0].total.amount,None);
    assert_eq!(last.points[0].available_total.amount,Some(10.));

    let week=store.analysis_daily_rollup(
        rev,&all,&query("2024-02-26","2024-03-03"),"week",None,60,
    ).unwrap();
    assert_eq!(week.points.len(),1);
    assert_eq!(week.points[0].start,"2024-02-26");
    assert_eq!(week.points[0].days,7);
    assert_eq!(week.points[0].available_reports,5);
    assert_eq!(week.points[0].missing,9);
    assert_eq!(week.points[0].total.amount,None);
    assert_eq!(week.points[0].available_total.amount,Some(249.));

    let months=store.analysis_daily_rollup(
        rev,&all,&query("2024-02-01","2024-03-31"),"month",None,60,
    ).unwrap();
    assert_eq!(months.points.len(),2);
    assert_eq!(months.points[0].start,"2024-02-01");
    assert_eq!(months.points[0].days,29);
    assert_eq!(months.points[0].available_total.amount,Some(239.));
    assert_eq!(months.points[1].start,"2024-03-01");
    assert_eq!(months.points[1].days,31);
    assert_eq!(months.points[1].available_total.amount,Some(10.));

    let year=store.analysis_daily_rollup(
        rev,&all,&query("2024-01-01","2024-12-31"),"year",None,60,
    ).unwrap();
    assert_eq!(year.points.len(),1);
    assert_eq!(year.points[0].days,366);
    assert_eq!(year.points[0].available_total.amount,Some(249.));
    assert_eq!(year.points[0].total.amount,None);
    let only_a=scope(&store,rev,"day",&["A"],None);
    let a=store.analysis_daily_rollup(
        rev,&only_a,&query("2024-02-28","2024-03-01"),"day",None,60,
    ).unwrap();
    assert!(a.points.iter().all(|p|p.complete));
    assert_eq!(a.points.iter().map(|p|p.total.amount.unwrap()).sum::<f64>(),249.);

    assert!(store.analysis_daily_rollup(
        rev,&all,&query("2024-02-28","2024-03-01"),"week",None,60,
    ).is_err());
    assert!(store.analysis_daily_rollup(
        rev,&all,&query("2024-02-02","2024-02-29"),"month",None,60,
    ).is_err());
    assert!(store.analysis_daily_rollup(
        rev,&all,&query("2024-01-01","2024-12-30"),"year",None,60,
    ).is_err());
    assert!(store.analysis_daily_rollup(
        rev,&all,&query("2024-01-01","2024-12-31"),"year",None,61,
    ).is_err());
}


#[test]
fn daily_rollup_does_not_add_independently_uploaded_week_or_month_reports() {
    let dir=tempdir().unwrap();
    let store=Storage::new(dir.path().into()).unwrap();
    let mut data=fixture();
    let reports=data["reports"].as_array_mut().unwrap();
    reports.push(json!({
        "id":"uploaded_week","type":"week",
        "start":"2024-02-26","end":"2024-03-03",
        "organizations":["A","B"],
        "rows":[{"organization":"A","work":"one","count":100.,"amount":8000.}]
    }));
    reports.push(json!({
        "id":"uploaded_month","type":"month",
        "start":"2024-02-01","end":"2024-02-29",
        "organizations":["A","B"],
        "rows":[{"organization":"A","work":"one","count":100.,"amount":9000.}]
    }));
    let revision=prepare(&store,&data);
    let scope=scope(&store,revision,"day",&["A","B"],None);
    let feb=store.analysis_daily_rollup(
        revision,&scope,&query("2024-02-01","2024-02-29"),
        "month",None,60,
    ).unwrap();
    assert_eq!(feb.points.len(),1);
    assert_eq!(feb.points[0].available_reports,4);
    assert_eq!(feb.points[0].available_total.amount,Some(239.));
    assert_eq!(feb.points[0].available_total.count,Some(4.));
    let week=store.analysis_daily_rollup(
        revision,&scope,&query("2024-02-26","2024-03-03"),
        "week",None,60,
    ).unwrap();
    assert_eq!(week.points.len(),1);
    assert_eq!(week.points[0].available_reports,5);
    assert_eq!(week.points[0].available_total.amount,Some(249.));
    assert_ne!(week.points[0].available_total.amount,Some(17249.));
}
