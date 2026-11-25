use super::*;
use tempfile::tempdir;

fn add(
    c: &Connection,
    id: &str,
    kind: &str,
    start: &str,
    stop: &str,
    valid: Option<bool>,
    orgs: &[&str],
    values: &[(f64, f64)],
    warned: bool,
) {
    c.execute("INSERT INTO reports(id,report_type,period_start,period_end,source_name,json) VALUES(?1,?2,?3,?4,?5,'malformed JSON is deliberately never read')", params![id,kind,start,stop,format!("{id}.xlsx")]).unwrap();
    if let Some(valid) = valid {
        c.execute(
            "INSERT INTO analysis_reports VALUES(?1,?2,?3,?4)",
            params![id, valid, (!valid).then_some("invalid_rows"), warned],
        )
        .unwrap();
        for org in orgs {
            c.execute("INSERT INTO analysis_orgs VALUES(?1,?2)", params![id, org])
                .unwrap();
        }
        if valid {
            for (index, (count, amount)) in values.iter().enumerate() {
                c.execute(
                    "INSERT INTO report_rows VALUES(?1,?2,?3,'Work',?4,?5)",
                    params![id, index as i64, orgs[0], count, amount],
                )
                .unwrap();
            }
            c.execute(
                "INSERT INTO report_row_index VALUES(?1,?2)",
                params![id, values.len() as i64],
            )
            .unwrap();
        }
    }
}
fn page(store: &Storage, from: &str, to: &str, orgs: &[&str]) -> QualityMatrixPage {
    store
        .quality_matrix_page(
            store.revision().unwrap(),
            "day",
            from,
            to,
            &orgs.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
        .unwrap()
}
#[test]
fn matrix_distinguishes_missing_invalid_conflicts_zero_activity_and_unassigned() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let c = store.connect().unwrap();
    add(
        &c,
        "zero",
        "day",
        "2024-01-01",
        "2024-01-01",
        Some(true),
        &["A"],
        &[(0., 0.)],
        false,
    );
    add(
        &c,
        "cancel",
        "day",
        "2024-01-02",
        "2024-01-02",
        Some(true),
        &["A"],
        &[(1., 10.), (-1., -10.)],
        false,
    );
    add(
        &c,
        "empty",
        "day",
        "2024-01-03",
        "2024-01-03",
        Some(true),
        &["A"],
        &[],
        false,
    );
    for id in ["dup1", "dup2"] {
        add(
            &c,
            id,
            "day",
            "2024-01-04",
            "2024-01-04",
            Some(true),
            &["A"],
            &[(2., 20.)],
            true,
        );
    }
    add(
        &c,
        "baddup",
        "day",
        "2024-01-04",
        "2024-01-04",
        Some(false),
        &["A"],
        &[],
        true,
    );
    add(
        &c,
        "unassigned",
        "day",
        "2024-01-05",
        "2024-01-05",
        Some(false),
        &[],
        &[],
        true,
    );
    add(
        &c,
        "invalid",
        "day",
        "2024-01-06",
        "2024-01-06",
        Some(false),
        &["A"],
        &[],
        false,
    );
    add(
        &c,
        "good",
        "day",
        "2024-01-07",
        "2024-01-07",
        Some(true),
        &["A"],
        &[(3., 30.)],
        false,
    );
    add(
        &c,
        "badgood",
        "day",
        "2024-01-07",
        "2024-01-07",
        Some(false),
        &["A"],
        &[],
        false,
    );
    let before = c.total_changes();
    let revision = store.revision().unwrap();
    let p = page(&store, "2024-01-01", "2024-01-08", &["A", "B"]);
    assert_eq!(
        (
            p.revision,
            p.report_type.as_str(),
            p.periods.len(),
            p.rows.len()
        ),
        (revision, "day", 8, 2)
    );
    let cells = &p.rows[0].cells;
    for i in 0..3 {
        assert!(cells[i].available);
        assert!(cells[i].numeric_zero);
        assert_eq!((cells[i].count, cells[i].amount), (Some(0.), Some(0.)));
    }
    assert!(!cells[0].has_activity);
    assert!(cells[1].has_activity);
    assert!(!cells[2].has_activity);
    let dup = &cells[3];
    assert_eq!(
        (
            dup.matching_reports,
            dup.invalid_reports,
            dup.duplicate_reports,
            dup.overlap_reports,
            dup.warned_reports
        ),
        (3, 1, 2, 0, 3)
    );
    assert!(!dup.available);
    assert_eq!(dup.count, None);
    assert_eq!(p.periods[4].unassigned_invalid, 1);
    assert_eq!(cells[4].matching_reports, 0);
    assert_eq!(p.rows[1].cells[4].matching_reports, 0);
    assert_eq!(cells[5].invalid_reports, 1);
    assert!(!cells[5].available);
    assert!(cells[6].available);
    assert_eq!(
        (
            cells[6].matching_reports,
            cells[6].invalid_reports,
            cells[6].count
        ),
        (2, 1, Some(3.))
    );
    assert!(!cells[7].available);
    assert!(!cells[7].numeric_zero);
    assert_eq!(cells[7].amount, None);
    assert_eq!(c.total_changes(), before);
    assert_eq!(store.revision().unwrap(), revision);
    assert_eq!(
        c.query_row("SELECT count(*) FROM analysis_filters", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let evidence = store
        .quality_matrix_reports(revision, "day", "2024-01-05", None, None, 20)
        .unwrap();
    assert_eq!(evidence.rows.len(), 1);
    assert_eq!(evidence.rows[0].id, "unassigned");
    assert_eq!(
        evidence.rows[0].source_name.as_deref(),
        Some("unassigned.xlsx")
    );
    assert_eq!(evidence.rows[0].issue.as_deref(), Some("invalid_rows"));
}
#[test]
fn weekly_shifted_is_not_itself_conflict_and_outside_partners_are_checked() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let c = store.connect().unwrap();
    add(
        &c,
        "exact",
        "week",
        "2024-01-08",
        "2024-01-14",
        Some(true),
        &["A", "B"],
        &[(1., 10.)],
        false,
    );
    add(
        &c,
        "outside",
        "week",
        "2024-01-02",
        "2024-01-08",
        Some(true),
        &["A"],
        &[(1., 10.)],
        false,
    );
    add(
        &c,
        "exact-duplicate",
        "week",
        "2024-01-08",
        "2024-01-14",
        Some(true),
        &["A"],
        &[],
        true,
    );
    add(
        &c,
        "other-type",
        "day",
        "2024-01-08",
        "2024-01-08",
        Some(true),
        &["B"],
        &[(1., 10.)],
        false,
    );
    add(
        &c,
        "invalid-partner",
        "week",
        "2024-01-09",
        "2024-01-15",
        Some(false),
        &["B"],
        &[],
        false,
    );
    add(
        &c,
        "shifted-only",
        "week",
        "2024-01-20",
        "2024-01-26",
        Some(true),
        &["A"],
        &[(1., 10.)],
        false,
    );
    let rev = store.revision().unwrap();
    let p = store
        .quality_matrix_page(
            rev,
            "week",
            "2024-01-08",
            "2024-01-22",
            &["A".into(), "B".into()],
        )
        .unwrap();
    assert_eq!(
        (
            p.rows[0].cells[0].overlap_reports,
            p.rows[0].cells[0].shifted_reports
        ),
        (2, 1)
    );
    assert_eq!(
        (
            p.rows[0].cells[0].duplicate_reports,
            p.rows[0].cells[0].warned_reports
        ),
        (2, 1)
    );
    assert!(!p.rows[0].cells[0].available);
    assert!(p.rows[1].cells[0].available);
    assert_eq!(
        (
            p.rows[0].cells[2].matching_reports,
            p.rows[0].cells[2].shifted_reports,
            p.rows[0].cells[2].overlap_reports
        ),
        (0, 1, 0)
    );
    let e = store
        .quality_matrix_reports(rev, "week", "2024-01-22", Some("A"), None, 20)
        .unwrap();
    assert_eq!(e.rows.len(), 1);
    assert!(e.rows[0].shifted);
    assert!(!e.rows[0].duplicate);
    assert!(!e.rows[0].overlap);
    add(
        &c,
        "far-partner",
        "week",
        "2024-01-14",
        "2024-01-20",
        Some(true),
        &["A"],
        &[],
        false,
    );
    let rev = store.revision().unwrap();
    let e = store
        .quality_matrix_reports(rev, "week", "2024-01-22", Some("A"), None, 20)
        .unwrap();
    assert_eq!(e.rows.len(), 1);
    assert!(e.rows[0].overlap);
    c.execute(
        "DELETE FROM analysis_reports WHERE report_id='far-partner'",
        [],
    )
    .unwrap();
    assert!(store
        .quality_matrix_reports(rev, "week", "2024-01-22", Some("A"), None, 20)
        .is_err());
    // A shifted report's own partners do not affect this matrix cell's exact-start availability.
    assert_eq!(
        store
            .quality_matrix_page(rev, "week", "2024-01-22", "2024-01-22", &["A".into()])
            .unwrap()
            .rows[0]
            .cells[0]
            .shifted_reports,
        1
    );
    // Unassigned evidence is limited to exact-start invalid reports.
    assert!(store
        .quality_matrix_reports(rev, "week", "2024-01-22", None, None, 20)
        .unwrap()
        .rows
        .is_empty());
    c.execute("DELETE FROM analysis_reports WHERE report_id='outside'", [])
        .unwrap();
    assert!(store
        .quality_matrix_page(rev, "week", "2024-01-08", "2024-01-08", &["A".into()])
        .unwrap_err()
        .contains("index"));
    assert!(store
        .quality_matrix_reports(rev, "week", "2024-01-08", Some("A"), None, 20)
        .is_err());
}
#[test]
fn reports_keyset_is_bounded_and_cursor_binds_all_query_state() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let c = store.connect().unwrap();
    for i in 0..25 {
        add(
            &c,
            &format!("r{i:02}"),
            "day",
            "2024-01-01",
            "2024-01-01",
            Some(true),
            &["A"],
            &[],
            false,
        );
    }
    let rev = store.revision().unwrap();
    let first = store
        .quality_matrix_reports(rev, "day", "2024-01-01", Some("A"), None, 20)
        .unwrap();
    assert_eq!(first.rows.len(), 20);
    assert!(first
        .rows
        .iter()
        .all(|r| r.duplicate && !r.overlap && !r.shifted));
    let cursor = first.next.unwrap();
    let second = store
        .quality_matrix_reports(rev, "day", "2024-01-01", Some("A"), Some(&cursor), 20)
        .unwrap();
    assert_eq!(second.rows.len(), 5);
    assert!(second.next.is_none());
    assert_eq!(second.rows[0].id, "r20");
    let direct = store.quality_matrix_reports_at_page(rev, "day", "2024-01-01", Some("A"), 1, 20).unwrap();
    assert_eq!((direct.total_rows, direct.rows.len()), (25, 5));
    assert_eq!(direct.rows[0].id, second.rows[0].id);
    assert!(direct.next.is_none());
    assert!(direct.rows.iter().all(|r| r.duplicate && !r.overlap && !r.shifted));
    assert!(store.quality_matrix_reports_at_page(rev, "day", "2024-01-01", Some("A"), 2, 20).is_err());
    let empty = store.quality_matrix_reports_at_page(rev, "day", "2024-01-01", Some("B"), 0, 20).unwrap();
    assert_eq!(empty.total_rows, 0);
    assert!(empty.rows.is_empty());
    for (kind, start, org) in [
        ("day", "2024-01-02", Some("A")),
        ("week", "2024-01-01", Some("A")),
        ("day", "2024-01-01", Some("B")),
        ("day", "2024-01-01", None),
    ] {
        assert!(store
            .quality_matrix_reports(rev, kind, start, org, Some(&cursor), 20)
            .is_err());
    }
    c.execute("UPDATE storage_revision SET revision=revision+1", [])
        .unwrap();
    assert!(store
        .quality_matrix_reports(rev, "day", "2024-01-01", Some("A"), Some(&cursor), 20)
        .is_err());
    assert!(store
        .quality_matrix_reports(rev + 1, "day", "2024-01-01", Some("A"), Some(&cursor), 20)
        .is_err());
}
#[test]
fn unrelated_unindexed_malformed_history_is_never_loaded_and_queries_stay_indexed() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let c = store.connect().unwrap();
    add(
        &c,
        "good",
        "day",
        "2024-01-01",
        "2024-01-01",
        Some(true),
        &["A"],
        &[(1., 10.)],
        false,
    );
    c.execute("INSERT INTO reports(id,report_type,period_start,period_end,json) VALUES('huge','day','2001-01-01','2001-01-01',zeroblob(20000000))",[]).unwrap();
    for i in 0..500 {
        add(
            &c,
            &format!("old{i}"),
            "week",
            "2000-01-01",
            "2000-01-07",
            None,
            &[],
            &[],
            false,
        );
    }
    // A malformed monthly start cannot be a valid partner of an aligned month.
    add(
        &c,
        "off-calendar-month",
        "month",
        "2024-01-15",
        "2024-02-14",
        None,
        &[],
        &[],
        false,
    );
    assert!(
        !store
            .quality_matrix_page(
                store.revision().unwrap(),
                "month",
                "2024-01-01",
                "2024-01-01",
                &["A".into()]
            )
            .unwrap()
            .rows[0]
            .cells[0]
            .available
    );
    assert_eq!(
        page(&store, "2024-01-01", "2024-01-01", &["A"]).rows[0].cells[0].count,
        Some(1.)
    );
    let rows = store
        .quality_matrix_reports(
            store.revision().unwrap(),
            "day",
            "2024-01-01",
            Some("A"),
            None,
            20,
        )
        .unwrap();
    assert_eq!(rows.rows.len(), 1);
    for sql in [CANDIDATES_SQL, PARTNER_SQL] {
        let mut statement = c.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let plan = statement
            .query_map(
                params!["day", "2024-01-01", "2024-01-01", "A", "good"],
                |r| r.get::<_, String>(3),
            )
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n");
        assert!(plan.contains("reports_by_period"), "{plan}");
        assert!(!plan.contains("SCAN r"), "{plan}");
        if sql == PARTNER_SQL {
            assert!(plan.contains("analysis_org_lookup"), "{plan}");
        }
    }
}
#[test]
fn range_organization_limits_and_missing_manifests_are_enforced() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let c = store.connect().unwrap();
    let rev = store.revision().unwrap();
    for (kind, from, to, orgs) in [
        ("day", "2024-01-01", "2024-02-01", vec!["A".into()]),
        ("week", "2024-01-01", "2024-01-09", vec!["A".into()]),
        ("month", "2024-01-02", "2024-01-02", vec!["A".into()]),
        ("month", "2024-01-01", "2025-01-01", vec!["A".into()]),
        ("day", "2024-01-01", "2024-01-01", vec![]),
        (
            "day",
            "2024-01-01",
            "2024-01-01",
            vec!["A".into(), "A".into()],
        ),
        ("day", "2024-01-01", "2024-01-01", vec!["".into()]),
    ] {
        assert!(store
            .quality_matrix_page(rev, kind, from, to, &orgs)
            .is_err());
    }
    assert_eq!(
        store
            .quality_matrix_page(rev, "month", "2024-01-01", "2024-12-01", &["A".into()])
            .unwrap()
            .periods
            .len(),
        12
    );
    for limit in [0, 21] {
        assert!(store
            .quality_matrix_reports(rev, "day", "2024-01-01", Some("A"), None, limit)
            .is_err());
    }
    assert_eq!(
        store
            .quality_matrix_page(rev, "day", "2024-01-01", "2024-01-31", &["A".into()])
            .unwrap()
            .periods
            .len(),
        31
    );
    assert_eq!(
        store
            .quality_matrix_page(rev, "week", "2024-01-01", "2024-03-18", &["A".into()])
            .unwrap()
            .periods
            .len(),
        12
    );
    assert!(store
        .quality_matrix_page(rev, "week", "2024-01-01", "2024-03-25", &["A".into()])
        .is_err());
    assert!(store
        .quality_matrix_page(
            rev,
            "day",
            "2024-01-01",
            "2024-01-01",
            &(0..9).map(|i| i.to_string()).collect::<Vec<_>>()
        )
        .is_err());
    assert!(store
        .quality_matrix_page(rev, "day", "2024-01-01", "2024-01-01", &["x".repeat(65537)])
        .is_err());
    assert!(store
        .quality_matrix_reports(
            rev,
            "day",
            "2024-01-01",
            Some("A"),
            Some("invalid cursor"),
            20
        )
        .is_err());
    add(
        &c,
        "missing",
        "day",
        "2024-01-01",
        "2024-01-01",
        None,
        &[],
        &[],
        false,
    );
    assert!(store
        .quality_matrix_page(
            store.revision().unwrap(),
            "day",
            "2024-01-01",
            "2024-01-01",
            &["A".into()]
        )
        .is_err());
}
