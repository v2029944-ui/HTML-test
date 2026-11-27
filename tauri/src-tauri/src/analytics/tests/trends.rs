use super::*;

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
fn daily_trend_and_csv_leave_an_invalid_day_empty_between_valid_days() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let row = |count: f64| json!({"organization":"A","work":"Work","count":count,"amount":count*10.0});
    let report = |id: &str, kind: &str, from: &str, to: &str, rows: Value| {
        json!({"id":id,"type":kind,"start":from,"end":to,"organizations":["A"],"rows":rows})
    };
    let data = json!({
        "reports":[
            report("observed","day","2024-01-01","2024-01-01",json!([row(2.5)])),
            report("invalid","day","2024-01-02","2024-01-03",json!([row(99.)])),
            report("zero","day","2024-01-03","2024-01-03",json!([row(0.)])),
            report("next","day","2024-01-04","2024-01-04",json!([row(3.)])),
            report("weekly","week","2024-01-01","2024-01-07",json!([row(100.)])),
            report("monthly","month","2024-01-01","2024-01-31",json!([row(200.)]))
        ],
        "settings":{"orgNames":["A","B","C","D"],"groups":[],"catalog":[]}
    });
    let revision = prepare(&store, &data);
    let selected = scope(&store, revision, "day", &["A"], Some(&["Work"]));
    let dates = query("2024-01-01", "2024-01-04");
    let points = store.analysis_timeline_grouped(revision, &selected, &dates, Some("day"))
        .unwrap().points;
    let counts: Vec<_> = points.iter().map(|p| (p.missing, p.total.count)).collect();
    assert_eq!(counts, [(0, Some(2.5)), (1, None), (0, Some(0.)), (0, Some(3.))]);

    let summary = store.analysis_range_summary(revision, &selected, &dates).unwrap();
    assert!(!summary.complete);
    assert_eq!(summary.invalid_reports, 1);
    assert_eq!(summary.total.count, None);
    assert_eq!(summary.available_total.count, Some(5.5));
    let quality = store.quality_matrix_page(revision, "day", &dates.from, &dates.to, &["A".into()]).unwrap();
    assert_eq!(quality.rows[0].cells[1].invalid_reports, 1);
    assert!(quality.rows[0].cells[2].numeric_zero);

    let dest = dir.path().join("days.csv");
    let options = serde_json::from_value(json!({
        "table": "works", "metrics": ["count"], "hideEmpty": false
    })).unwrap();
    store.reference_trend_csv(revision, &selected, &dates, Some("day"), &options, &dest).unwrap();
    let csv = fs::read_to_string(dest).unwrap();
    assert!(csv.lines().any(|line|line == r#""Work";"2,5";"";"0";"3""#), "{csv}");
    assert_eq!(store.revision().unwrap(), revision);
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
fn organization_trends_still_rejects_overflow_across_finite_points() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let rev = prepare(&store, &fixture());
    let selected = scope(&store, rev, "day", &["A"], None);
    let conn = store.connect().unwrap();
    conn.execute(
        "UPDATE report_rows SET amount=1e308 WHERE organization='A' AND row_index=0",
        [],
    ).unwrap();
    let first = store.analysis_organization_trends(
        rev, &selected, &query("2024-02-28", "2024-02-28"),
    ).unwrap();
    assert!(first.series[0].points[0].total.amount.unwrap().is_finite());
    let error = store.analysis_organization_trends(
        rev, &selected, &query("2024-02-28", "2024-03-01"),
    ).unwrap_err();
    assert!(error.contains("numeric overflow"), "{error}");
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

#[test]
fn work_group_filter_is_consistent_across_kpis_lines_orgs_and_calendar() {
    let dir=tempdir().unwrap();
    let store=Storage::new(dir.path().into()).unwrap();
    let rev=prepare(&store,&fixture());
    let selected=scope(&store,rev,"day",&["A","B"],Some(&["Group"]));
    let q=query("2024-02-28","2024-02-29");

    let kpis=store.analysis_range_summary(rev,&selected,&q).unwrap();
    assert!(kpis.complete);
    assert_eq!(kpis.total.count,Some(6.));
    assert_eq!(kpis.total.amount,Some(100.));
    assert_eq!(kpis.missing,0);

    let timeline=store.analysis_timeline(rev,&selected,&q).unwrap();
    assert_eq!(timeline.points.len(),2);
    assert_eq!(timeline.points[0].total.amount,Some(20.));
    assert_eq!(timeline.points[1].total.amount,Some(80.));
    assert_eq!(timeline.points[0].total.count,Some(2.));
    assert_eq!(timeline.points[1].total.count,Some(4.));

    let divisions=store.analysis_organization_page(
        rev,&selected,&q,None,10
    ).unwrap();
    assert_eq!(divisions.len(),2);
    assert_eq!(divisions[0].org,"A");
    assert_eq!(divisions[0].total.amount,Some(100.));
    assert_eq!(divisions[1].total.amount,Some(0.));
    let org_lines=store.analysis_organization_trends(rev,&selected,&q).unwrap();
    assert_eq!(org_lines.series.len(),2);
    assert_eq!(org_lines.series[0].points[0].total.amount,Some(20.));
    assert_eq!(org_lines.series[0].points[1].total.amount,Some(80.));
    assert_eq!(org_lines.series[1].points[0].total.amount,Some(0.));

    let ranking=store.analysis_ranking(
        rev,&selected,&q,None,"amount","all"
    ).unwrap();
    assert!(ranking.complete);
    assert_eq!(ranking.rows.len(),1);
    assert_eq!(ranking.rows[0].name,"Group");
    assert_eq!(ranking.rows[0].amount,Some(100.));

    let feb=store.analysis_daily_rollup(
        rev,&selected,&query("2024-02-01","2024-02-29"),"month",None,60
    ).unwrap();
    assert_eq!(feb.points.len(),1);
    assert_eq!(feb.points[0].available_reports,4);
    assert_eq!(feb.points[0].missing,54);
    assert_eq!(feb.points[0].total.amount,None);
    assert_eq!(feb.points[0].available_total.amount,Some(100.));
    assert_eq!(feb.points[0].available_total.count,Some(6.));

    let missing=scope(&store,rev,"day",&["A"],Some(&["Never recorded"]));
    let absent=store.analysis_range_summary(rev,&missing,&q).unwrap();
    assert!(absent.complete);
    assert_eq!(absent.total.amount,Some(0.));
    let points=store.analysis_timeline(rev,&missing,&q).unwrap();
    assert!(points.points.iter().all(|p|p.total.amount==Some(0.)));
    let rolls=store.analysis_daily_rollup(
        rev,&missing,&q,"day",None,60
    ).unwrap();
    assert!(rolls.points.iter().all(|p|p.complete&&p.total.amount==Some(0.)));
}

#[test]
fn selected_work_filter_preserves_missing_reports_and_negative_amounts() {
    let dir=tempdir().unwrap();
    let store=Storage::new(dir.path().into()).unwrap();
    let rev=prepare(&store,&fixture());
    let selected=scope(&store,rev,"day",&["A","B"],Some(&["Ignored"]));
    let q=query("2024-02-28","2024-03-01");
    let kpis=store.analysis_range_summary(rev,&selected,&q).unwrap();
    assert!(!kpis.complete);
    assert_eq!(kpis.missing,1);
    assert_eq!(kpis.total.amount,None);
    assert_eq!(kpis.available_total.amount,Some(70.));
    let chart=store.analysis_timeline(rev,&selected,&q).unwrap();
    assert_eq!(chart.points[0].total.amount,Some(30.));
    assert_eq!(chart.points[1].total.amount,Some(40.));
    assert_eq!(chart.points[2].total.amount,None);
    let org=store.analysis_organization_trends(rev,&selected,&q).unwrap();
    assert_eq!(org.series[0].points[2].total.amount,Some(0.));
    assert_eq!(org.series[1].points[2].total.amount,None);
    let daily=store.analysis_daily_rollup(
        rev,&selected,&q,"day",None,60
    ).unwrap();
    assert_eq!(daily.points[0].total.amount,Some(30.));
    assert_eq!(daily.points[1].total.amount,Some(40.));
    assert_eq!(daily.points[2].total.amount,None);
    assert_eq!(daily.points[2].available_total.amount,Some(0.));
}

#[test]
fn daily_source_trends_respect_explicit_calendar_buckets() {
    let dir=tempdir().unwrap();
    let store=Storage::new(dir.path().into()).unwrap();
    let rev=prepare(&store,&fixture());
    let s=scope(&store,rev,"day",&["A"],None);
    let q=RangeQuery{from:"2024-02-26".into(),to:"2024-03-10".into()};
    let timeline=store.analysis_timeline_grouped(rev,&s,&q,Some("week")).unwrap();
    assert_eq!(timeline.granularity,"week");
    assert_eq!(timeline.points.len(),2);
    assert_eq!(timeline.points[0].start,"2024-02-26");
    assert_eq!(timeline.points[0].periods,7);
    assert_eq!(timeline.points[0].missing,4);
    assert!(timeline.points[0].total.amount.is_none());
    let orgs=store.analysis_organization_trends_grouped(rev,&s,&q,Some("week")).unwrap();
    assert_eq!(orgs.series[0].points.len(),2);
    assert_eq!(orgs.series[0].points[0].missing,4);
    let work=store.analysis_work_timeline_grouped(rev,&s,&q,None,"group:g",Some("week"));
    assert!(work.is_ok(),"{work:?}");
    assert_eq!(work.unwrap().points.len(),2);
    let annual=RangeQuery{from:"2024-01-01".into(),to:"2024-12-31".into()};
    let year=store.analysis_timeline_grouped(rev,&s,&annual,Some("year")).unwrap();
    assert_eq!(year.points.len(),1);assert_eq!(year.points[0].periods,366);
    assert!(store.analysis_timeline_grouped(rev,&s,&q,Some("nonsense")).is_err());
}

#[test]
fn imported_week_does_not_fill_a_missing_daily_report_or_duplicate_daily_totals() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    let mut reports = Vec::new();
    for day in 1..=13 {
        let date = format!("2024-01-{day:02}");
        reports.push(json!({"id":date,"type":"day","start":date,"end":date,"organizations":["A"],
            "rows":[{"organization":"A","work":"W","count":1,"amount":10}]}));
    }
    reports.push(json!({"id":"weekly","type":"week","start":"2024-01-08","end":"2024-01-14",
        "organizations":["A"],"rows":[{"organization":"A","work":"W","count":9,"amount":95}]}));
    data["reports"] = json!(reports);
    let rev = prepare(&store, &data);
    let daily = scope(&store, rev, "day", &["A"], None);
    let weekly = scope(&store, rev, "week", &["A"], None);
    let range = |from: &str, to: &str| RangeQuery { from: from.into(), to: to.into() };
    let first = range("2024-01-01", "2024-01-07");
    let second = range("2024-01-08", "2024-01-14");

    let full = store.analysis_range_summary(rev, &daily, &first).unwrap();
    assert!(full.complete);
    assert_eq!((full.available_reports, full.total.amount), (7, Some(70.)));
    let partial = store.analysis_range_summary(rev, &daily, &second).unwrap();
    assert!(!partial.complete);
    assert_eq!((partial.available_reports, partial.missing), (6, 1));
    assert_eq!((partial.total.amount, partial.available_total.amount), (None, Some(60.)));
    let week_start = range("2024-01-08", "2024-01-08");
    let imported = store.analysis_range_summary(rev, &weekly, &week_start).unwrap();
    assert!(imported.complete);
    assert_eq!((imported.available_reports, imported.total.amount), (1, Some(95.)));
}

#[test]
fn work_tree_timeline_rolls_up_descendant_works_once_and_can_open_an_individual_work() {
    let dir = tempfile::tempdir().unwrap();
    let store = Storage::new(dir.path().to_owned()).unwrap();
    let reports = json!({"reports":[
        {"id":"day-one","type":"day","start":"2024-02-28","end":"2024-02-28","organizations":["A"],
         "rows":[
          {"organization":"A","work":"Direct","count":2,"amount":20},
          {"organization":"A","work":"Nested","count":3,"amount":45},
          {"organization":"A","work":"Outside","count":9,"amount":900}]},
        {"id":"day-two","type":"day","start":"2024-02-29","end":"2024-02-29","organizations":["A"],
         "rows":[
          {"organization":"A","work":"Direct","count":1,"amount":10},
          {"organization":"A","work":"Nested","count":4,"amount":null}]}
    ],"settings":{"orgNames":["A","B","C","D"],"catalog":[],
        "groups":[{"id":"root","name":"Root","works":[]},{"id":"branch","name":"Branch","works":["Direct"]},
            {"id":"leaf","name":"Leaf","works":["Nested"]}],
        "workGroupTreeParents":[{"child":"branch","parent":"root"},{"child":"leaf","parent":"branch"}]
    }});
    let revision = prepare(&store, &reports);
    let only_root = scope(&store, revision, "day", &["A"], Some(&["Root"]));
    let range = query("2024-02-28", "2024-02-29");
    let overlap = scope(&store, revision, "day", &["A"], Some(&["Root", "Leaf", "Nested"]));
    for selected in [&only_root, &overlap] {
        let summary = store.analysis_range_summary(revision, selected, &range).unwrap();
        assert_eq!(summary.available_total.count, Some(10.));
        assert_eq!(summary.available_total.amount, Some(75.));
        let timeline = store.analysis_timeline(revision, selected, &range).unwrap();
        assert_eq!(timeline.points[0].available_total.amount, Some(65.));
        assert_eq!(timeline.points[1].available_total.amount, Some(10.));
    }
    let named_work = scope(&store, revision, "day", &["A"], Some(&["Nested"]));
    let timeline = store.analysis_timeline(revision, &named_work, &range).unwrap();
    assert_eq!(timeline.points[0].available_total.amount, Some(45.));

    let catalog = store.unified_name_options_at_page(revision, "", 0, 50).unwrap();
    assert!(catalog.rows.iter().any(|row| row.key == "work:Nested"));
    let top_level = store.unified_work_series_at_page(revision, 0, 50).unwrap();
    assert_eq!(top_level.rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
        ["work:Outside", "group:root"]);

    let selection = store.unified_selected_name_options(revision,
        &["group:root".into(), "group:leaf".into(), "work:Nested".into()]).unwrap();
    let counts = selection.work_counts.unwrap();
    assert_eq!((counts.selected, counts.total), (2, 3));
    let raw = selection.rows.iter().find(|row| row.key == "work:Nested").unwrap();
    assert_eq!(raw.ancestor_keys.len(), 3);
    let children = store.unified_group_children_at_page(revision, "group:root", 0, 50).unwrap();
    assert_eq!(children.total_rows, 1);
    assert_eq!(children.rows[0].key, "group:branch");
    assert!(children.rows[0].has_children);

    let root = store.analysis_work_timeline(revision, &only_root, &range, None, "group:root").unwrap();
    assert_eq!(root.points[0].available_total.count, Some(5.));
    assert_eq!(root.points[0].available_total.amount, Some(65.));
    assert_eq!(root.points[1].available_total.count, Some(5.));
    assert_eq!(root.points[1].available_total.amount, Some(10.));
    assert_eq!(root.points[1].total.average, None);
    assert_eq!(root.points[1].available_total.average, Some(10.));
    assert_eq!(root.points[1].available_total.partial_average_quantity, Some(1.));
    let last_day = query("2024-02-29", "2024-02-29");
    let summary = store.analysis_range_summary(revision, &overlap, &last_day).unwrap();
    assert_eq!(summary.available_total.average, root.points[1].available_total.average);
    assert_eq!((summary.report_rows, summary.priced_rows), (2, 1));

    let leaf = store.analysis_work_timeline(revision, &only_root, &range, None, "group:leaf").unwrap();
    assert_eq!(leaf.points[0].available_total.amount, Some(45.));
    let nested = store.analysis_work_timeline(revision, &only_root, &range, None, "work:Nested").unwrap();
    assert_eq!(nested.points[0].available_total.count, Some(3.));
    assert_eq!(nested.points[0].available_total.amount, leaf.points[0].available_total.amount);
    assert!(store.analysis_work_timeline(revision, &only_root, &range, None, "work:Outside").is_err());

    let csv = dir.path().join("tree.csv");
    let options = TrendCsvOptions { table: TrendTable::Works, metrics: vec![TrendMetric::Amount],
        hide_empty: false, match_chart: true };
    store.reference_trend_csv(revision, &overlap, &range, Some("day"), &options, &csv).unwrap();
    let exported = fs::read_to_string(csv).unwrap();
    let values = exported.lines().skip(1).take(6).collect::<Vec<_>>();
    assert_eq!(values, [
        r#""Итого по выбранным работам";"65";"10""#,
        r#""Root";"65";"10""#,
        r#""  ↳ Branch";"65";"10""#,
        r#""    ↳ Direct";"20";"10""#,
        r#""    ↳ Leaf";"45";"""#,
        r#""      ↳ Nested";"45";"""#,
    ]);
    assert!(!exported.contains("Outside"));

    let detail_csv = dir.path().join("tree-by-department.csv");
    let detail = serde_json::from_value(json!({
        "section":"organizations","metric":"amount","baseOrg":"A"
    })).unwrap();
    let change_query = ChangeQuery {
        from: range.from.clone(), to: range.to.clone(), method: "range".into(),
        percent: 0., min_count: 0., min_amount: 0.,
    };
    store.reference_detail_csv(revision, &overlap, &change_query, None, &detail, &detail_csv).unwrap();
    let detail = fs::read_to_string(detail_csv).unwrap();
    assert!(detail.contains(r#""Итого по выбранным работам";"75""#), "{detail}");
    assert!(detail.contains(r#""Root (группа)";"75""#), "{detail}");
    assert!(!detail.lines().any(|line| line.starts_with(r#""Leaf"#)
        || line.starts_with(r#""Nested"#) || line.starts_with(r#""Outside"#)), "{detail}");

    let only_leaf = scope(&store, revision, "day", &["A"], Some(&["Leaf"]));
    assert!(store.analysis_work_timeline(revision, &only_leaf, &range, None, "group:root").is_err());
}

#[test]
fn work_tree_exact_name_keeps_group_and_excludes_unrelated_namesake() {
    let dir = tempfile::tempdir().unwrap();
    let store = Storage::new(dir.path().to_owned()).unwrap();
    let data = json!({"reports":[
        {"id":"report","type":"day","start":"2024-02-29","end":"2024-02-29","organizations":["A"],
         "rows":[
            {"organization":"A","work":"Head","count":2,"amount":20},
            {"organization":"A","work":"Other","count":5,"amount":50},
            {"organization":"A","work":"Only","count":1,"amount":10},
            {"organization":"A","work":"Solo","count":7,"amount":70}]}
    ],"settings":{"orgNames":["A","B","C","D"],"catalog":[],"groups":[
        {"id":"head","name":"Head","works":[]},
        {"id":"child","name":"Child","works":["Head","Other"]},
        {"id":"solo","name":"Solo","works":["Only"]}
    ],"workGroupTreeParents":[{"child":"child","parent":"head"}]}});
    let revision = prepare(&store, &data);
    let catalog = store.unified_name_options_at_page(revision, "", 0, 50).unwrap();
    assert!(catalog.has_omitted_names);
    for (name, key) in [("Head", "group:head"), ("Solo", "group:solo")] {
        let matches: Vec<_> = catalog.rows.iter().filter(|row| row.name == name)
            .map(|row| row.key.as_str()).collect();
        assert_eq!(matches, [key]);
    }

    let range = query("2024-02-29", "2024-02-29");
    let head = scope(&store, revision, "day", &["A"], Some(&["Head"]));
    let summary = store.analysis_range_summary(revision, &head, &range).unwrap();
    assert_eq!(summary.available_total.amount, Some(70.));
    let children = store.unified_group_children_at_page(revision, "group:child", 0, 50).unwrap();
    assert!(children.rows.iter().any(|row| row.key == "work:Head"));
    let work = store.analysis_work_timeline(revision, &head, &range, None, "work:Head").unwrap();
    assert_eq!(work.points[0].available_total.amount, Some(20.));

    let solo = scope(&store, revision, "day", &["A"], Some(&["Solo"]));
    let summary = store.analysis_range_summary(revision, &solo, &range).unwrap();
    assert_eq!(summary.available_total.amount, Some(10.));
    assert!(store.analysis_work_timeline(revision, &solo, &range, None, "work:Solo").is_err());
}
