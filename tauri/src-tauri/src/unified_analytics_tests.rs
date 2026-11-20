use super::*;
use tempfile::tempdir;
fn fixture() -> (tempfile::TempDir, Storage, i64, AnalysisScope) {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().join("store")).unwrap();
    let reports:Vec<Value>=(0..2).map(|day|json!({"id":format!("d{day}"),"type":"day","start":format!("2024-01-0{}",day+1),"end":format!("2024-01-0{}",day+1),"organizations":["A"],"rows":(0..75).map(|i|json!({"organization":"A","work":if i==74 {"ЯРКАЯ услуга".into()}else{format!("Работа {i:02}")},"count":if i==0 {-1.}else{(day+1) as f64},"amount":if i==0 {-10.}else if i==1 {0.}else{((day+1)*10+i) as f64}})).collect::<Vec<_>>()})).collect();
    store
        .save(json!({"reports":reports,"settings":{"orgNames":["A","B","C","D"],"groups":[],"catalog":[]}}))
        .unwrap();
    let rev = store.revision().unwrap();
    for id in ["d0", "d1"] {
        store.analysis_index_report(id, rev).unwrap()
    }
    store.analysis_index_settings(rev).unwrap();
    let id = store.analysis_filter_create(rev, "organizations").unwrap();
    store
        .analysis_filter_append(rev, &id, &["A".into()])
        .unwrap();
    store.analysis_filter_seal(rev, &id).unwrap();
    (
        dir,
        store,
        rev,
        AnalysisScope {
            report_type: "day".into(),
            organizations: id,
            names: None,
        },
    )
}
#[test]
fn unified_metadata_and_pattern_filters_are_paged() {
    let (_dir, s, r, scope) = fixture();
    let q = query();
    let names = s.unified_name_options(r, "", None, 50).unwrap();
    assert_eq!(names.rows.len(), 50);
    assert_eq!(names.total_rows, 75);
    let tail = s
        .unified_name_options(r, "", names.next.as_deref(), 50)
        .unwrap();
    assert_eq!(tail.rows.len(), 25);
    assert_eq!(
        s.unified_name_options(r, "яркая", None, 50)
            .unwrap()
            .rows
            .len(),
        1
    );
    assert!(s
        .unified_name_options(r, "other", names.next.as_deref(), 50)
        .is_err());
    let periods = s.unified_period_options(r, "day", None, None, 1).unwrap();
    assert_eq!(periods.total_rows, 2);
    assert_eq!(periods.min.as_deref(), Some("2024-01-01"));
    assert_eq!(periods.max.as_deref(), Some("2024-01-02"));
    assert!(periods.next.is_some());
    let month = s
        .unified_period_options(r, "day", Some("month"), None, 10)
        .unwrap();
    assert_eq!(month.rows[0].end, "2024-01-31");
    let options = UnifiedPatternOptions {
        range_window: false,
        baseline: None,
        search: "яркая".into(),
        status: "all".into(),
    };
    let pattern = s
        .unified_pattern_page(r, &scope, &q, None, "regularity", 4, &options, None)
        .unwrap();
    assert_eq!(pattern.total_rows, 1);
    assert_eq!(pattern.rows.len(), 1);
    assert_eq!(pattern.rows[0]["name"], "ЯРКАЯ услуга");
    let none = s
        .unified_pattern_page(
            r,
            &scope,
            &q,
            None,
            "regularity",
            4,
            &UnifiedPatternOptions {
                range_window: false,
                baseline: None,
                search: "яркая".into(),
                status: "rare".into(),
            },
            None,
        )
        .unwrap();
    assert_eq!(none.total_rows, 0);
}
fn query() -> ChangeQuery {
    ChangeQuery {
        method: "previous".into(),
        from: "2024-01-02".into(),
        to: "2024-01-02".into(),
        percent: 30.,
        min_count: 5.,
        min_amount: 1000.,
    }
}
fn options() -> UnifiedChangeOptions {
    UnifiedChangeOptions {
        metrics: vec!["count".into(), "amount".into()],
        mode: "all".into(),
        search: "".into(),
        direction: "all".into(),
        sort: "name".into(),
    }
}
#[test]
fn unified_search_before_limit_unicode_and_stable_cursor() {
    let (_dir, s, r, scope) = fixture();
    let q = query();
    let mut o = options();
    let first = s
        .unified_changes_page(r, &scope, &q, None, &o, None)
        .unwrap();
    assert_eq!(first.rows.len(), 50);
    assert_eq!(first.total_rows, 75);
    let second = s
        .unified_changes_page(r, &scope, &q, None, &o, first.next.as_deref())
        .unwrap();
    assert_eq!(second.rows.len(), 25);
    assert!(!first
        .rows
        .iter()
        .any(|a| second.rows.iter().any(|b| a.key == b.key)));
    o.search = "яркая".into();
    let found = s
        .unified_changes_page(r, &scope, &q, None, &o, None)
        .unwrap();
    assert_eq!(found.rows.len(), 1);
    assert_eq!(found.summary.rows, 1);
    assert_eq!(found.rows[0].name, "ЯРКАЯ услуга");
    assert!(s
        .unified_changes_page(r, &scope, &q, None, &o, first.next.as_deref())
        .is_err());
    o.direction = "negative".into();
    o.search.clear();
    let negative = s
        .unified_changes_page(r, &scope, &q, None, &o, None)
        .unwrap();
    assert_eq!(negative.rows.len(), 1);
    assert_eq!(negative.summary.rows, 1);
    o.direction = "up".into();
    assert_eq!(
        s.unified_changes_page(r, &scope, &q, None, &o, None)
            .unwrap()
            .summary
            .rows,
        74
    );
}
#[test]
fn unified_mode_summary_csv_and_validation() {
    let (dir, s, r, scope) = fixture();
    let q = query();
    let mut o = options();
    o.search = "Яркая".into();
    o.mode = "strong".into();
    let page = s
        .unified_changes_page(r, &scope, &q, None, &o, None)
        .unwrap();
    assert_eq!(page.summary.rows, 1);
    assert_eq!(page.total_rows, 1); // small-base notable count
    let path = dir.path().join("result.csv");
    s.unified_changes_csv(r, &scope, &q, None, &o, &path)
        .unwrap();
    let csv = std::fs::read_to_string(&path).unwrap();
    assert!(csv.contains("ЯРКАЯ услуга"));
    assert!(!csv.contains("Работа 01"));
    assert!(s
        .unified_changes_csv(r, &scope, &q, None, &o, &path)
        .is_err());
    assert!(s
        .unified_changes_page(r + 1, &scope, &q, None, &o, None)
        .is_err());
    o.sort = "evil SQL".into();
    assert!(s
        .unified_changes_page(r, &scope, &q, None, &o, None)
        .is_err());
    o = options();
    o.metrics = vec![];
    assert!(s
        .unified_changes_page(r, &scope, &q, None, &o, None)
        .is_err());
}

#[test]
fn unified_sorts_and_missing_reports_are_not_zero() {
    let (_dir, s, r, scope) = fixture();
    let mut q = query();
    let mut o = options();
    for sort in [
        "name",
        "significance-count",
        "significance-amount",
        "absolute-count",
        "absolute-amount",
        "percent-count",
        "percent-amount",
        "current-count",
        "current-amount",
    ] {
        o.sort = sort.into();
        let a = s
            .unified_changes_page(r, &scope, &q, None, &o, None)
            .unwrap();
        let b = s
            .unified_changes_page(r, &scope, &q, None, &o, None)
            .unwrap();
        assert_eq!(
            a.rows.iter().map(|v| &v.key).collect::<Vec<_>>(),
            b.rows.iter().map(|v| &v.key).collect::<Vec<_>>()
        );
        assert_eq!(a.summary.totals.missing_pairs, 0);
    }
    q.to = "2024-01-03".into();
    q.from = q.to.clone();
    let absent = s
        .unified_changes_page(r, &scope, &q, None, &o, None)
        .unwrap();
    assert_eq!(absent.total_rows, 0);
    assert_eq!(absent.summary.totals.missing_pairs, 1);
    assert_eq!(absent.missing_examples.len(), 1);
}

#[test]
fn unified_duplicate_reports_remain_excluded() {
    let (_dir, s, _r, _scope) = fixture();
    let mut data = s.load().unwrap();
    let mut duplicate = data["reports"][1].clone();
    duplicate["id"] = json!("duplicate");
    data["reports"].as_array_mut().unwrap().push(duplicate);
    s.save(data).unwrap();
    let r = s.revision().unwrap();
    for id in ["d0", "d1", "duplicate"] {
        s.analysis_index_report(id, r).unwrap();
    }
    s.analysis_index_settings(r).unwrap();
    let id = s.analysis_filter_create(r, "organizations").unwrap();
    s.analysis_filter_append(r, &id, &["A".into()]).unwrap();
    s.analysis_filter_seal(r, &id).unwrap();
    let scope = AnalysisScope {
        report_type: "day".into(),
        organizations: id,
        names: None,
    };
    let page = s
        .unified_changes_page(r, &scope, &query(), None, &options(), None)
        .unwrap();
    assert_eq!(page.total_rows, 0);
    assert!(page.summary.totals.duplicate_issues > 0);
    assert_eq!(page.summary.totals.missing_pairs, 1);
}

#[test]
fn unified_group_audit_search_coverage_and_handles() {
    let (_dir, s, _r, _scope) = fixture();
    let mut data = s.load().unwrap();
    data["settings"]["groups"] = json!([
        {"id":"active","name":"Действующая","works":["ЯРКАЯ услуга"]},
        {"id":"absent","name":"Неиспользованная","works":["Нет выполнений"]}
    ]);
    data["settings"]["catalog"] = json!([{"work":"Только справочник","organization":"A"}]);
    s.save(data).unwrap();
    let r = s.revision().unwrap();
    for id in ["d0", "d1"] {
        s.analysis_index_report(id, r).unwrap();
    }
    s.analysis_index_settings(r).unwrap();
    s.management_index_settings(r, &ManagementJob::new())
        .unwrap();
    let id = s.analysis_filter_create(r, "organizations").unwrap();
    s.analysis_filter_append(r, &id, &["A".into()]).unwrap();
    s.analysis_filter_seal(r, &id).unwrap();
    let scope = AnalysisScope {
        report_type: "day".into(),
        organizations: id,
        names: None,
    };
    let mut q = RangeQuery {
        from: "2024-01-01".into(),
        to: "2024-01-02".into(),
    };
    let page = s
        .unified_group_audit_page(r, &scope, &q, "all", "яркая", 0, 50)
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].group_name.as_deref(), Some("Действующая"));
    assert!(page.rows[0].group.as_ref().unwrap().parse::<i64>().is_ok());
    assert_eq!(page.rows[0].active, Some(true));
    let unused = s
        .unified_group_audit_page(r, &scope, &q, "unused", "", 0, 50)
        .unwrap();
    assert_eq!(unused.total, 1);
    assert_eq!(unused.rows[0].active, Some(false));
    assert!(unused.coverage.complete);
    let catalog = s
        .unified_group_audit_page(r, &scope, &q, "ungrouped", "только", 0, 50)
        .unwrap();
    assert_eq!(catalog.total, 1);
    assert_eq!(catalog.rows[0].active, Some(false));
    q.to = "2024-01-03".into();
    let missing = s
        .unified_group_audit_page(r, &scope, &q, "unused", "", 0, 50)
        .unwrap();
    assert!(!missing.coverage.complete);
    assert_eq!(missing.rows[0].active, None);
    assert!(s
        .unified_group_audit_page(r, &scope, &q, "bad", "", 0, 50)
        .is_err());
    assert!(s
        .unified_group_audit_page(r + 1, &scope, &q, "all", "", 0, 50)
        .is_err());
}

#[test]
fn unified_structure_limit_preserves_remainder_and_old_default() {
    let (_dir, s, r, scope) = fixture();
    let mut q = query();
    q.method = "range".into();
    let before = RangeQuery {
        from: "2024-01-01".into(),
        to: "2024-01-01".into(),
    };
    for limit in [10, 20] {
        let page = s
            .advanced_composition_limit(
                r,
                &scope,
                &q,
                None,
                &before,
                "structure",
                "amount",
                None,
                limit,
            )
            .unwrap();
        let rows = page["rows"].as_array().unwrap();
        assert_eq!(rows.len(), limit + 1);
        assert_eq!(rows.last().unwrap()["other"], true);
        let sum: f64 = rows.iter().map(|r| r["current"].as_f64().unwrap()).sum();
        assert_eq!(sum, page["total"].as_f64().unwrap());
    }
    let legacy = s
        .advanced_composition(r, &scope, &q, None, &before, "structure", "amount", None)
        .unwrap();
    assert_eq!(legacy["rows"].as_array().unwrap().len(), 13);
}

#[test]
fn unified_historical_picker_keys_have_zero_only_with_complete_coverage() {
    let (_dir,s,_r,_scope)=fixture();let mut data=s.load().unwrap();
    // Both identities remain in the global picker but have no row anywhere
    // in the selected current range, reproducing Select All across history.
    data["reports"][1]["rows"].as_array_mut().unwrap().retain(|row|row["work"]!="ЯРКАЯ услуга"&&row["work"]!="Работа 01");
    data["settings"]["groups"]=json!([{"id":"historical","name":"Историческая группа","works":["Работа 01"]}]);
    s.save(data).unwrap();let r=s.revision().unwrap();for id in ["d0","d1"] {s.analysis_index_report(id,r).unwrap();}s.analysis_index_settings(r).unwrap();
    let org=s.analysis_filter_create(r,"organizations").unwrap();s.analysis_filter_append(r,&org,&["A".into()]).unwrap();s.analysis_filter_seal(r,&org).unwrap();
    let mut scope=AnalysisScope{report_type:"day".into(),organizations:org,names:None};
    let current=RangeQuery{from:"2024-01-02".into(),to:"2024-01-02".into()};
    let missing=RangeQuery{from:"2024-01-03".into(),to:"2024-01-03".into()};
    for key in ["work:ЯРКАЯ услуга","group:historical"] {
        let zero=s.analysis_work_timeline(r,&scope,&current,None,key).unwrap();assert_eq!(zero.points.len(),1);assert_eq!(zero.points[0].missing,0);assert_eq!(zero.points[0].total.count,Some(0.));assert_eq!(zero.points[0].total.amount,Some(0.));assert_eq!(zero.points[0].total.average,None);
        let unknown=s.analysis_work_timeline(r,&scope,&missing,None,key).unwrap();assert_eq!(unknown.points[0].missing,1);assert_eq!(unknown.points[0].total.count,None);assert_eq!(unknown.points[0].total.amount,None);
    }
    assert!(s.analysis_work_timeline(r,&scope,&current,None,"work:invented").is_err());
    let filter=s.analysis_filter_create(r,"names").unwrap();s.analysis_filter_append(r,&filter,&["Работа 02".into()]).unwrap();s.analysis_filter_seal(r,&filter).unwrap();scope.names=Some(filter);
    assert!(s.analysis_work_timeline(r,&scope,&current,None,"group:historical").is_err());
    assert!(s.analysis_work_timeline(r+1,&scope,&current,None,"work:ЯРКАЯ услуга").is_err());
}

#[test]
fn reference_usual_uses_whole_equal_windows_and_excludes_partial_samples() {
    let (_dir, store, revision, scope) = fixture();
    let options = UnifiedPatternOptions {
        search: "яркая".into(), status: "all".into(), range_window: true, baseline: None,
    };
    let run = |from: &str, to: &str| {
        let query = ChangeQuery { from: from.into(), to: to.into(), ..query() };
        store.unified_pattern_page(revision, &scope, &query, None, "usual", 4, &options, None).unwrap()
    };
    let current = run("2024-01-01", "2024-01-02");
    assert_eq!(current.rows[0]["count"]["current"], 3.0);
    assert_eq!(current.used, 0);
    assert!(current.rows[0]["count"]["base"].is_null());
    let with_sample = run("2024-01-03", "2024-01-04");
    assert_eq!(with_sample.used, 1);
    assert_eq!(with_sample.rows[0]["count"]["base"], 3.0);
    assert!(with_sample.rows[0]["count"]["current"].is_null());
    let partial_sample = run("2024-01-04", "2024-01-06");
    assert_eq!(partial_sample.used, 0);
    assert!(partial_sample.rows[0]["count"]["base"].is_null());
}

#[test]
fn reference_frequency_counts_overlapping_and_future_baseline_days_separately() {
    let (_dir, store, revision, scope) = fixture();
    let q = ChangeQuery { from: "2024-01-01".into(), to: "2024-01-01".into(), ..query() };
    let options = UnifiedPatternOptions {
        search: "яркая".into(), status: "all".into(), range_window: false,
        baseline: Some(RangeQuery { from: "2024-01-01".into(), to: "2024-01-02".into() }),
    };
    let result = store.unified_pattern_page(revision, &scope, &q, None, "regularity", 4, &options, None).unwrap();
    assert_eq!(result.used, 1);
    assert_eq!(result.previous_used, 2);
    assert_eq!(result.rows[0]["hits"], 1);
    assert_eq!(result.rows[0]["previousHits"], 2);
    assert_eq!(result.rows[0]["last"], "2024-01-01");
}

#[path="reference_detail_csv_tests.rs"]
mod reference_detail_csv_tests;
