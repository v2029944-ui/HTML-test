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
fn independent_unequal_intervals_require_complete_org_coverage_and_keep_real_works() {
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
    assert_eq!(result.comparable, ["A", "Empty"]);
    assert_eq!(result.total, 4);
    assert!(result.rows.iter().all(|r| r.org == "A"));
    assert!(result.rows.iter().all(|r| r.name != "Combined"));
    let work = result.rows.iter().find(|r| r.name == "Work").unwrap();
    assert_eq!(
        (work.before, work.after, work.delta, work.percent),
        (100., 150., 50., Some(50.))
    );
    let new = result.rows.iter().find(|r| r.name == "New").unwrap();
    assert_eq!((new.before, new.after, new.percent), (0., 40., None));
    assert_eq!(
        (
            result.counts.up,
            result.counts.down,
            result.counts.new,
            result.counts.gone
        ),
        (1, 0, 1, 1)
    );
    assert_eq!(result.leader.unwrap().name, "Work");
    let mut filtered = q.clone();
    filtered.kind = "significant".into();
    filtered.search = "new".into();
    let found = store.reference_changes(&filtered).unwrap();
    assert_eq!(found.total, 1);
    assert_eq!(found.rows[0].name, "New");
    assert_eq!(found.counts.up, 1);
    assert_eq!(found.leader.unwrap().name, "Work");
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
    assert_eq!(csv.lines().count(), 28);
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
    assert!(missing.rows.is_empty());
    assert_eq!(missing.total, 0);
    assert!(missing.leader.is_none());
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
    assert!(csv.find("\"A\";").unwrap()<csv.find("\"Empty\";").unwrap());
    assert!(csv.find("\"Empty\";").unwrap()<csv.find("\"Missing\";").unwrap());
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
