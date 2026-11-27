use super::*;

#[test]
fn missing_money_preserves_quantity_in_ranges_comparisons_and_exports() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    data["reports"][1]["rows"][0]["amount"] = Value::Null;
    let rev = prepare(&store, &data);
    let all = scope(&store, rev, "day", &["A"], None);
    let range = |day: &str| RangeQuery { from: day.into(), to: day.into() };
    let before = range("2024-02-28");
    let after = range("2024-02-29");
    let current = store.analysis_range_summary(rev, &all, &after).unwrap();
    assert!(current.complete);
    assert_eq!(current.total.count, Some(4.0));
    assert_eq!(current.total.amount, None);
    assert_eq!(current.total.average, None);
    let filtered = scope(&store, rev, "day", &["A"], Some(&["zero"]));
    let zero = store.analysis_range_summary(rev, &filtered, &after).unwrap();
    assert_eq!(zero.total.amount, Some(0.0));

    let compare = ComparisonQuery { before, after: after.clone(), min_count: 0., min_amount: 0., percent: 0. };
    let comparison = store.analysis_comparison_summary(rev, &all, &compare).unwrap();
    assert_eq!(comparison.count.unwrap().delta, 4.0);
    assert!(comparison.amount.is_none());
    let csv = dir.path().join("range.csv");
    store.analysis_export_range(rev, &all, &after, None, &csv).unwrap();
    let exported = fs::read_to_string(csv).unwrap();
    assert!(exported.lines().any(|line|line.contains("Group") && line.ends_with(r#";"4";"";"";"true""#)), "{exported}");

    let matrix = store.quality_matrix_page(rev, "day", &after.from, &after.to, &["A".into()]).unwrap();
    let cell = &matrix.rows[0].cells[0];
    assert!(cell.available);
    assert_eq!((cell.count, cell.amount), (Some(4.0), None));
    assert!(!cell.numeric_zero);
}

#[test]
fn detail_views_distinguish_missing_amount_missing_report_and_zero_baseline() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let report = |day: &str, rows: Value| json!({
        "id":day,"type":"day","start":day,"end":day,"organizations":["A"],"rows":rows
    });
    let row = |work: &str, count: i64, amount: Option<i64>| {
        json!({"organization":"A","work":work,"count":count,"amount":amount})
    };
    let rev = prepare(&store, &json!({"reports":[
        report("2024-02-27", json!([row("Alpha", 0, Some(0))])),
        report("2024-02-28", json!([row("Alpha", 2, Some(20))])),
        report("2024-02-29", json!([row("Alpha", 3, Some(30)), row("Beta", 1, None)]))
    ],"settings":{"orgNames":["A","B","C","D"],"catalog":[],"groups":[]}}));
    let all = scope(&store, rev, "day", &["A"], None);
    let base = query("2024-02-27", "2024-02-27");
    let options = UnifiedPatternOptions {
        search: "".into(), status: "all".into(), range_window: true, baseline: None,
    };
    for (end, complete) in [("2024-02-29", true), ("2024-03-01", false)] {
        let current = query("2024-02-29", end);
        let changes = ChangeQuery {
            method: "range".into(), from: current.from.clone(), to: current.to.clone(),
            percent: 0., min_count: 0., min_amount: 0.,
        };
        for (metric, total) in [("count", 4.), ("amount", 30.)] {
            let structure = store.advanced_composition(rev, &all, &changes, None, &base, "structure", metric, None).unwrap();
            assert_eq!(structure["total"], total);
            let alpha = &structure["rows"][0];
            assert_eq!(alpha["name"], "Alpha");
            assert_eq!(structure["currentComplete"], complete && metric == "count");
            let beta = &structure["rows"][1];
            if metric == "count" {
                assert_eq!(alpha["share"], 75.);
                assert_eq!(alpha["cumulative"], 75.);
                assert_eq!(beta["share"], 25.);
                assert_eq!(beta["cumulative"], 100.);
            } else {
                assert_eq!(alpha["share"], 100.);
                assert_eq!(alpha["cumulative"], 100.);
                assert!(beta["current"].is_null() && beta["share"].is_null() && beta["cumulative"].is_null());
            }
        }
        let comparison = store.analysis_comparison_summary(rev, &all, &ComparisonQuery {
            before: base.clone(), after: current, percent: 0., min_count: 0., min_amount: 0.,
        }).unwrap();
        assert!(comparison.amount.is_none());
        if complete {
            let count = comparison.count.unwrap();
            assert_eq!((count.base, count.current, count.delta, count.percent), (0., 4., 4., None));
        } else {
            assert!(comparison.count.is_none());
        }
        let usual = store.unified_pattern_page_at_page(rev, &all, &changes, None, "usual", 4, &options, 0).unwrap();
        let alpha = usual.rows.iter().find(|row| row["name"] == "Alpha").unwrap();
        let beta = usual.rows.iter().find(|row| row["name"] == "Beta").unwrap();
        assert!(beta["amount"]["delta"].is_null() && beta["amount"]["percent"].is_null());
        if complete {
            assert_eq!(alpha["amount"]["base"], 10.);
            assert_eq!(alpha["amount"]["delta"], 20.);
            assert_eq!(alpha["amount"]["percent"], 200.);
        } else {
            assert_eq!(alpha["amount"]["base"], 20.);
            assert!(alpha["amount"]["delta"].is_null() && alpha["amount"]["percent"].is_null());
        }
    }
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
    assert_eq!(sum.amount.unwrap().delta, total("amount", "delta"));
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
fn sql_ranking_is_sorted_bounded_and_uses_available_reports() {
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
    assert_eq!(incomplete.rows[0].name, "Group");
    assert_eq!(incomplete.rows[0].amount, Some(110.));
    let absent = store.analysis_ranking(
        rev, &combined, &query("2024-04-01", "2024-04-02"), None, "amount", "all"
    ).unwrap();
    assert!(!absent.complete);
    assert!(absent.rows.is_empty());

    assert!(store.analysis_ranking(rev, &selected, &days, None, "percent", "all").is_err());
    assert!(store.analysis_ranking(rev, &selected, &days, None, "amount", "unknown").is_err());
    assert!(store.analysis_ranking(rev, &selected, &days, Some("B"), "amount", "all").is_err());
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
    assert_eq!(partial.summary.amount.as_ref().unwrap().delta, -59.);
    assert!(partial.summary.count.is_some());
    for ranking in [&partial.top_amount, &partial.top_count] {
        assert!(!ranking.is_empty());
        assert!(ranking.iter().all(|row| row.status.is_none()));
    }
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
    for (slot, amount) in [(0, 2.), (1, 1.)] {
        data["reports"][slot]["rows"].as_array_mut().unwrap().push(
            json!({"organization":"A","work":"small decline","count":1.,"amount":amount}),
        );
    }
    let rev=prepare(&store,&data);
    let selected=scope(&store,rev,"day",&["A"],None);
    let q=ComparisonQuery {
        before:query("2024-02-28","2024-02-28"),
        after:query("2024-02-29","2024-02-29"),
        percent:0.,min_count:0.,min_amount:0.,
    };
    let detail=store.analysis_comparison_dashboard(rev,&selected,&q).unwrap();
    assert!(detail.amount_leaders.up.is_empty() && detail.count_leaders.down.is_empty());
    let result=store.analysis_comparison_dashboard_with_leaders(rev,&selected,&q,true).unwrap();
    assert_eq!(result.amount_leaders.up.len(), 5);
    assert_eq!(result.amount_leaders.up[0].name, "one");
    assert!(result.amount_leaders.up.iter().all(|row| row.name != "Group" && row.delta > 0.));
    assert!(result.amount_leaders.down.iter().any(|row| row.name == "small decline" && row.delta == -1.));
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
fn selectable_ranking_filters_names_before_the_limit() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    let rows = data["reports"][0]["rows"].as_array_mut().unwrap();
    for i in 0..13 {
        let work = format!("{}{i}", "К".repeat(300));
        rows.push(json!({"organization":"A","work":work,"count":1.,"amount":100000+i}));
    }
    rows.push(json!({"organization":"A","work":"Group","count":1.,"amount":120000.}));
    for i in 0..9 {
        rows.push(json!({"organization":"A","work":format!("Work{i}"),"count":1.,"amount":1000+i}));
    }
    let revision = prepare(&store, &data);
    let selected = scope(&store, revision, "day", &["A"], None);
    let day = query("2024-02-28","2024-02-28");
    for org in [None, Some("A")] {
        let all = store.analysis_ranking(revision,&selected,&day,org,"amount","all").unwrap();
        assert!(all.rows.iter().all(|row| row.name == "Group" || row.name.len() > 512));
        let eligible = store.analysis_ranking(revision,&selected,&day,org,"amount","selectable").unwrap();
        let first_eight: Vec<_> = eligible.rows.iter().take(8).map(|row| row.name.clone()).collect();
        assert_eq!(first_eight, (1..=8).rev().map(|i| format!("Work{i}")).collect::<Vec<_>>());
        assert!(eligible.rows.iter().all(|row| row.name != "Group" && row.name.len() <= 512));
    }
}

#[test]
fn partial_analytics_keeps_observed_amounts_and_report_gaps_separate() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    data["reports"] = json!([
        {"id":"first","type":"day","start":"2024-01-01","end":"2024-01-01","organizations":["A","B"],"rows":[
            {"organization":"A","work":"Wash","count":2,"amount":20},
            {"organization":"B","work":"Wash","count":3,"amount":null}
        ]},
        {"id":"second","type":"day","start":"2024-01-02","end":"2024-01-02","organizations":["B"],"rows":[
            {"organization":"B","work":"Wash","count":4,"amount":30}
        ]}
    ]);
    let revision = prepare(&store, &data);
    let selected = scope(&store, revision, "day", &["A","B"], None);
    let range = RangeQuery { from:"2024-01-01".into(), to:"2024-01-03".into() };
    let summary = store.analysis_range_summary(revision, &selected, &range).unwrap();
    assert_eq!(summary.total.amount, None);
    assert_eq!(summary.available_total.amount, Some(50.));
    assert_eq!(summary.available_total.count, Some(9.));
    assert_eq!(summary.available_total.average, Some(50. / 6.));
    let ranking = store.analysis_ranking(revision, &selected, &range, None, "amount", "selectable").unwrap();
    assert_eq!(ranking.rows.len(), 1);
    assert_eq!(ranking.rows[0].amount, Some(50.));
    assert_eq!(ranking.rows[0].average, Some(50. / 6.));
    let timeline = serde_json::to_value(store.analysis_timeline(revision, &selected, &range).unwrap()).unwrap();
    assert_eq!(timeline["points"][0]["available_total"]["amount"], 20.);
    assert_eq!(timeline["points"][0]["available_total"]["average"], 10.);
    assert_eq!(timeline["points"][1]["available_total"]["amount"], 30.);
    assert!(timeline["points"][2]["available_total"]["count"].is_null());
    let departments = serde_json::to_value(store.analysis_organization_page(revision, &selected, &range, None, 50).unwrap()).unwrap();
    assert_eq!(departments[0]["available_total"]["amount"], 20.);
    assert_eq!(departments[1]["available_total"]["amount"], 30.);
    assert_eq!(departments[1]["available_total"]["average"], 7.5);
    assert_eq!(departments[0]["amount_missing_rows"], false);
    assert_eq!(departments[1]["amount_missing_rows"], true);
    let by_org = serde_json::to_value(store.analysis_organization_trends(revision, &selected, &range).unwrap()).unwrap();
    assert!(by_org["series"][1]["points"][0]["available_total"]["amount"].is_null());
    assert_eq!(by_org["series"][1]["points"][0]["available_total"]["count"], 3.);
    let detail = serde_json::to_value(store.analysis_work_timeline(revision, &selected, &range, None, "work:Wash").unwrap()).unwrap();
    assert_eq!(detail["points"][0]["available_total"]["average"], 10.);
    assert_eq!(detail["points"][1]["available_total"]["amount"], 30.);
    let q = ComparisonQuery {
        before: RangeQuery { from:"2023-12-31".into(), to:"2024-01-01".into() },
        after: RangeQuery { from:"2024-01-02".into(), to:"2024-01-03".into() },
        percent:0., min_count:0., min_amount:0.
    };
    let page = serde_json::to_value(store.analysis_comparison_window(revision, &selected, &q, None, 0, 50).unwrap()).unwrap();
    assert_eq!(page["rows"][0]["observed_before"]["amount"], 20.);
    assert_eq!(page["rows"][0]["observed_before"]["average"], 10.);
    assert_eq!(page["rows"][0]["average"]["delta"], -2.5);
    assert_eq!(page["rows"][0]["observed_after"]["amount"], 30.);
    assert_eq!(page["rows"][0]["amount"]["delta"], 10.);
    assert_eq!(page["rows"][0]["count"]["delta"], -1.);
    assert!(page["rows"][0]["status"].is_null());
    let selection = ChangeQuery { method:"range".into(), from:range.from.clone(),to:range.to.clone(),
        percent:0., min_count:0., min_amount:0. };
    let contribution_query = ChangeQuery { from: q.after.from.clone(), to: q.after.to.clone(), ..selection.clone() };
    let contributions = store.advanced_composition(revision, &selected, &contribution_query, None,
        &q.before, "contributions", "amount", None).unwrap();
    assert_eq!(contributions["rows"][0]["delta"], 10.);
    let structure = store.advanced_composition(revision,&selected,&selection,None,&q.before,"structure","amount",None).unwrap();
    assert_eq!(structure["currentAvailable"],true);
    assert_eq!(structure["currentComplete"],false);
    assert_eq!(structure["total"],50.);
    assert_eq!(structure["rows"][0]["current"],50.);
    assert_eq!(structure["sharesValid"], true);
    assert_eq!(structure["rows"][0]["share"], 100.);
    assert_eq!(structure["rows"][0]["cumulative"], 100.);

    let known_day = "2024-01-01".to_owned();
    let known = RangeQuery { from: known_day.clone(), to: known_day.clone() };
    let exact_selection = ChangeQuery { from: known_day.clone(), to: known_day, ..selection };
    let counts = store.advanced_composition(revision, &selected, &exact_selection, None, &known, "structure", "count", None).unwrap();
    assert_eq!(counts["currentComplete"], true);
    assert_eq!(counts["sharesValid"], true);
    assert_eq!(counts["rows"][0]["share"], 100.);
    assert_eq!(counts["rows"][0]["cumulative"], 100.);
    let same_day = ComparisonQuery { before: known.clone(), after: known, ..q };
    let same = serde_json::to_value(store.analysis_comparison_window(revision, &selected, &same_day, None, 0, 50).unwrap()).unwrap();
    assert_eq!(same["rows"][0]["count"]["delta"], 0.);
    assert_eq!(same["rows"][0]["count"]["percent"], 0.);
    assert_eq!(same["rows"][0]["amount"]["delta"], 0.);
}

#[test]
fn summary_average_uses_priced_rows_across_selected_works() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    data["settings"]["groups"] = json!([
        {"id":"mixed","name":"Mixed","works":["Wash","Repair"]}
    ]);
    data["reports"] = json!([
        {"id":"first","type":"day","start":"2024-01-01","end":"2024-01-01","organizations":["A"],"rows":[
            {"organization":"A","work":"Wash","count":10,"amount":20000},
            {"organization":"A","work":"Wash","count":5,"amount":15000},
            {"organization":"A","work":"Wash","count":4,"amount":null}
        ]},
        {"id":"second","type":"day","start":"2024-01-02","end":"2024-01-02","organizations":["A"],"rows":[
            {"organization":"A","work":"Repair","count":2,"amount":5000},
            {"organization":"A","work":"Repair","count":3,"amount":null},
            {"organization":"A","work":"Fence","count":1,"amount":10}
        ]},
        {"id":"third","type":"day","start":"2024-01-03","end":"2024-01-03","organizations":["A"],"rows":[
            {"organization":"A","work":"Wash","count":2,"amount":0},
            {"organization":"A","work":"Wash","count":8,"amount":null}
        ]},
        {"id":"fourth","type":"day","start":"2024-01-04","end":"2024-01-04","organizations":["A"],"rows":[
            {"organization":"A","work":"Wash","count":-1,"amount":100},
            {"organization":"A","work":"Wash","count":3,"amount":null}
        ]}
    ]);
    let revision = prepare(&store, &data);
    let selected = scope(&store, revision, "day", &["A"], Some(&["Mixed"]));
    let first = query("2024-01-01", "2024-01-01");
    let second = query("2024-01-02", "2024-01-02");
    let summary = store.analysis_range_summary(revision, &selected, &first).unwrap();
    assert_eq!(summary.available_total.count, Some(19.));
    assert_eq!(summary.available_total.amount, Some(35000.));
    assert_eq!(summary.available_total.partial_average_quantity, Some(15.));
    assert_eq!((summary.priced_rows, summary.report_rows), (2, 3));
    assert_eq!(summary.available_total.average, Some(35000. / 15.));
    let mixed = store.analysis_range_summary(revision, &selected, &query("2024-01-01", "2024-01-02")).unwrap();
    assert_eq!(mixed.available_total.amount, Some(40000.));
    assert_eq!(mixed.available_total.count, Some(24.));
    assert_eq!(mixed.available_total.average, Some(40000. / 17.));
    let all = scope(&store, revision, "day", &["A"], None);
    let summary = store.analysis_range_summary(revision, &all, &query("2023-12-31", "2024-01-02")).unwrap();
    assert!(!summary.complete);
    assert_eq!((summary.priced_rows, summary.report_rows), (4, 6));
    assert_eq!(summary.available_total.count, Some(25.));
    assert_eq!(summary.available_total.amount, Some(40010.));
    assert_eq!(summary.available_total.average, Some(40010. / 18.));
    let comparison = ComparisonQuery { before: first, after: second, percent: 0., min_count: 0., min_amount: 0. };
    let dashboard = store.analysis_comparison_dashboard(revision, &selected, &comparison).unwrap();
    assert_eq!(dashboard.summary.average.base, Some(35000. / 15.));
    assert_eq!(dashboard.summary.average.current, Some(2500.));
    assert_eq!(dashboard.summary.average.delta, Some(2500. - 35000. / 15.));
    let rows = store.analysis_comparison_window(revision, &selected, &comparison, None, 0, 50).unwrap().rows;
    assert_eq!(rows[0].average.delta, None);
    for (day, expected) in [("2024-01-03", Some(0.)), ("2024-01-04", None)] {
        let summary = store.analysis_range_summary(revision, &selected, &query(day, day)).unwrap();
        assert_eq!(summary.available_total.average, expected);
    }
}

#[test]
fn unpriced_work_does_not_block_an_average_of_comparable_priced_rows() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    data["settings"]["groups"] = json!([
        {"id":"mixed","name":"Mixed","works":["Wash","Repair"]}
    ]);
    data["reports"] = json!([
            {"id":"first","type":"day","start":"2024-01-01","end":"2024-01-01","organizations":["A","B"],"rows":[
                {"organization":"A","work":"Wash","count":10,"amount":20000},
                {"organization":"B","work":"Repair","count":4,"amount":null}
            ]},
            {"id":"second","type":"day","start":"2024-01-02","end":"2024-01-02","organizations":["A"],"rows":[
                {"organization":"A","work":"Wash","count":5,"amount":15000},
                {"organization":"A","work":"Repair","count":3,"amount":null},
                {"organization":"A","work":"Unpriced","count":2,"amount":null}
            ]}
    ]);
    let revision = prepare(&store, &data);
    let all = scope(&store, revision, "day", &["A", "B"], None);
    let mixed = scope(&store, revision, "day", &["A", "B"], Some(&["Mixed"]));
    let first = query("2024-01-01", "2024-01-01");
    let second = query("2024-01-02", "2024-01-03");
    let both = query("2024-01-01", "2024-01-03");

    let total = store.analysis_range_summary(revision, &all, &both).unwrap();
    assert_eq!(total.available_total.average, Some(35000. / 15.));
    assert_eq!(total.available_total.partial_average_quantity, Some(15.));
    assert_eq!(total.available_total.count, Some(24.));
    assert_eq!(total.available_total.amount, Some(35000.));
    assert!(!total.complete);

    let comparison = ComparisonQuery {
        before: first, after: second, percent: 0., min_count: 0., min_amount: 0.,
    };
    let dashboard = store.analysis_comparison_dashboard(revision, &mixed, &comparison).unwrap();
    assert_eq!(dashboard.summary.average.delta, Some(1000.));
    assert_eq!(serde_json::to_value(&dashboard).unwrap()["top_average"][0]["average"]["delta"], 1000.);
    let page = store.analysis_comparison_window(revision, &mixed, &comparison, None, 0, 50).unwrap();
    assert_eq!(page.rows[0].average.delta, Some(1000.));

    let changes = ChangeQuery {
        method: "range".into(), from: "2024-01-01".into(), to: "2024-01-02".into(),
        percent: 0., min_count: 0., min_amount: 0.,
    };
    let options = UnifiedChangeOptions {
        metrics: vec!["average".into()], mode: "all".into(),
        search: String::new(), direction: "up".into(), sort: "name".into(),
    };
    let page = store.unified_changes_page(revision, &mixed, &changes, None, &options, None).unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].average.delta, Some(1000.));
    let serialized = serde_json::to_value(&page).unwrap();
    assert_eq!(serialized["summary"]["top_average"]["average"]["delta"], 1000.);
    assert_eq!(serialized["summary"]["rows_with_missing_amount"], 1);
    assert_eq!(serialized["summary"]["totals"]["missing_pairs"], 1);
    assert_eq!(serialized["summary"]["totals"]["partial_pairs"], 0);

    let timeline = store.analysis_timeline(revision, &all, &both).unwrap();
    assert_eq!(timeline.points[0].available_total.average, Some(2000.));
    assert_eq!(timeline.points[1].available_total.average, Some(3000.));
    assert_eq!(timeline.points[2].available_total.average, None);
    let departments = store.analysis_organization_trends(revision, &all, &both).unwrap();
    assert_eq!(departments.series[0].points[1].available_total.average, Some(3000.));
    assert_eq!(departments.series[1].points[0].available_total.average, None);
    let detail = store.analysis_work_timeline(revision, &mixed, &both, None, "group:mixed").unwrap();
    assert_eq!(detail.points[1].available_total.average, Some(3000.));

    let rollup = store.analysis_daily_rollup(revision, &mixed, &both, "day", None, 5).unwrap();
    assert_eq!(rollup.points[0].available_total.average, Some(2000.));
    assert_eq!(rollup.points[1].available_total.average, Some(3000.));
    assert_eq!(rollup.points[2].available_total.average, None);

    let destination = dir.path().join("trends.csv");
    let options = TrendCsvOptions {
        table: TrendTable::Works, metrics: vec![TrendMetric::Average],
        hide_empty: false, match_chart: true,
    };
    store.reference_trend_csv(revision, &mixed, &both, None, &options, &destination).unwrap();
    let csv = fs::read_to_string(destination).unwrap();
    assert!(csv.lines().any(|line| line.contains(r#";"2000";"3000";"""#)), "{csv}");
}

#[test]
fn range_reads_each_selected_report_for_its_eligible_organizations() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    data["reports"] = json!([
        {"id":"outside","type":"day","start":"2024-01-01","end":"2024-01-01","organizations":["A"],
         "rows":[{"organization":"A","work":"one","count":90,"amount":900}]},
        {"id":"selected","type":"day","start":"2024-01-02","end":"2024-01-02","organizations":["A","B","C"],
         "rows":[
            {"organization":"A","work":"one","count":2,"amount":20},
            {"organization":"A","work":"one","count":3,"amount":null},
            {"organization":"A","work":"other","count":40,"amount":400},
            {"organization":"B","work":"one","count":4,"amount":0},
            {"organization":"C","work":"one","count":80,"amount":800}
         ]}
    ]);
    let rev = prepare(&store, &data);
    store.connect().unwrap().execute_batch("ANALYZE main").unwrap();
    let scope = scope(&store, rev, "day", &["A", "B", "D"], Some(&["Group"]));
    let summary = store
        .analysis_range_summary(rev, &scope, &query("2024-01-02", "2024-01-02"))
        .unwrap();
    assert_eq!((summary.available_reports, summary.missing), (2, 1));
    assert_eq!((summary.report_rows, summary.priced_rows), (3, 2));
    assert_eq!(summary.available_total.count, Some(9.));
    assert_eq!(summary.available_total.amount, Some(20.));
    assert_eq!(summary.available_total.average, Some(20. / 6.));
}
