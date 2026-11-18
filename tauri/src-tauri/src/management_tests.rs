use super::*;
use std::sync::Arc;
use tempfile::tempdir;
fn management_data() -> Value {
    json!({"reports":[{"id":"keep","type":"day","start":"2026-11-01","end":"2026-11-01","amount":30,"organizations":["A","B"],"warnings":[{"unknown":[1,2]}],"rows":[{"organization":"A","work":"Report only","count":2,"amount":30}]}],"settings":{"orgNames":["A","B","C","D"],"unknown":{"a":[{"preserve":true},[1,2,3]],"label":"escaped \"x\""},"groups":[{"id":"dup","name":"Empty","works":[],"unknown":{"nested":[1,2]}},{"id":"dup","name":"First","works":["one","one","Shared"],"color":"red"},{"id":"other","name":"Second","works":["Shared","group only"]}],"catalog":[{"work":"Catalog only","organization":"B","unknown":{"x":[1,2]}},{"work":"one","organization":"A"}]}})
}
fn management_setup(s: &Storage, data: &Value) -> i64 {
    s.save(data.clone()).unwrap();
    let rev = s.revision().unwrap();
    s.management_index_settings(rev, &ManagementJob::new())
        .unwrap();
    for r in data["reports"].as_array().unwrap() {
        s.management_index_report(r["id"].as_str().unwrap(), rev, &ManagementJob::new())
            .unwrap();
    }
    rev
}
#[test]
fn management_full_fidelity_pages_counts_and_settings_only_crud() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let original = management_data();
    let rev = management_setup(&s, &original);
    let boot = s.management_bootstrap(rev).unwrap();
    assert_eq!(boot.groups, 3);
    assert_eq!(boot.memberships, 5);
    assert_eq!(boot.catalog, 2);
    assert_eq!(boot.reports, 1);
    assert_eq!(boot.org_names, vec!["A", "B", "C", "D"]);
    let first = s.management_group_page(rev, "", None, 1).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].name, "Empty");
    assert_eq!(first[0].members, 0);
    let later = s
        .management_group_page(rev, "", Some(first[0].handle), 500)
        .unwrap();
    assert_eq!(later.len(), 2);
    assert_eq!(later[0].id, "dup");
    assert_eq!(later[0].members, 3);
    assert_eq!(
        s.management_member_page(rev, later[0].handle, None, 1)
            .unwrap()[0]
            .work,
        "one"
    );
    assert_eq!(
        s.management_member_page(rev, later[0].handle, None, 500)
            .unwrap()
            .len(),
        3
    );
    let work = s.management_work_summary(rev, "", None).unwrap();
    assert_eq!(work.total, 5);
    assert_eq!(work.grouped, 3);
    assert_eq!(
        s.management_work_page(rev, "", None, None, 1)
            .unwrap()
            .len(),
        1
    );
    let before_reports = s
        .connect()
        .unwrap()
        .query_row("SELECT json FROM reports WHERE id='keep'", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap();
    let job = Arc::new(ManagementJob::new());
    let mut edit = s
        .management_begin_settings(rev, 64 * 1024 * 1024, job)
        .unwrap();
    edit.rename_group(later[0].handle, "Renamed").unwrap();
    edit.append_members(first[0].handle, &["new".into(), "Shared".into()])
        .unwrap();
    let group = edit.create_group("dup", "Third").unwrap();
    edit.append_members(group, &["last".into()]).unwrap();
    let catalog = s.management_catalog_page(rev, "", None, None, 500).unwrap();
    edit.update_catalog(catalog[0].handle, "Changed catalog", Some("B"))
        .unwrap();
    edit.set_scalar("theme", ManagementScalar::String("dark".into()))
        .unwrap();
    let receipt = s.management_commit_settings(edit).unwrap();
    assert!(receipt.revision > rev);
    assert_eq!(
        s.connect()
            .unwrap()
            .query_row("SELECT json FROM reports WHERE id='keep'", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        before_reports
    );
    let data = s.load().unwrap();
    assert_eq!(data["settings"]["unknown"], original["settings"]["unknown"]);
    assert_eq!(data["settings"]["groups"][1]["color"], "red");
    assert_eq!(
        data["settings"]["groups"][0]["unknown"],
        original["settings"]["groups"][0]["unknown"]
    );
    assert_eq!(
        data["settings"]["groups"][0]["works"],
        json!(["new", "Shared"])
    );
    assert_eq!(
        data["settings"]["groups"][1]["works"],
        original["settings"]["groups"][1]["works"]
    );
    assert_eq!(
        data["settings"]["catalog"][0]["unknown"],
        original["settings"]["catalog"][0]["unknown"]
    );
    assert_eq!(data["settings"]["theme"], "dark");
    assert_eq!(s.management_bootstrap(receipt.revision).unwrap().groups, 4);
}
#[test]
fn management_targeted_delete_clear_stale_and_cancel_preserve_settings() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let mut data = management_data();
    let mut second = data["reports"][0].clone();
    second["id"] = json!("other");
    data["reports"].as_array_mut().unwrap().push(second);
    let rev = management_setup(&s, &data);
    let job = ManagementJob::new();
    job.cancel();
    assert!(s
        .management_delete_reports(rev, &["keep".into()], &job)
        .is_err());
    let receipt = s
        .management_delete_reports(rev, &["keep".into()], &ManagementJob::new())
        .unwrap();
    assert_eq!(receipt.deleted, 1);
    assert_eq!(s.load().unwrap()["reports"].as_array().unwrap().len(), 1);
    assert!(s
        .management_clear_reports(rev, &ManagementJob::new())
        .is_err());
    let receipt = s
        .management_clear_reports(receipt.revision, &ManagementJob::new())
        .unwrap();
    assert_eq!(receipt.deleted, 1);
    assert_eq!(s.load().unwrap()["settings"], data["settings"]);
    assert!(s.load().unwrap()["reports"].as_array().unwrap().is_empty());
}
fn management_golden(data: &Value) -> Value {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/native_management_golden.cjs");
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
        .write_all(data.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn management_real_js_goldens_full_counts_search_navigation_and_unknown_nodes() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let mut data = management_data();
    let mut second = data["reports"][0].clone();
    second["id"] = json!("later");
    second["start"] = json!("2026-11-02");
    second["end"] = json!("2026-11-02");
    data["reports"].as_array_mut().unwrap().push(second);
    let rev = management_setup(&s, &data);
    let old = management_golden(&data);
    let coverage = s.management_work_summary(rev, "", None).unwrap();
    assert_eq!(
        json!({"total":coverage.total,"grouped":coverage.grouped,"ungrouped":coverage.ungrouped}),
        old["coverage"]
    );
    let groups = s.management_group_page(rev, "", None, 500).unwrap();
    let actual: Vec<_> = groups
        .iter()
        .map(|g| json!({"id":g.id,"name":g.name,"members":g.members,"unique":g.unique_members,"works":s.management_member_page(rev,g.handle,None,500).unwrap().iter().map(|m|m.work.clone()).collect::<Vec<_>>()}))
        .collect();
    assert_eq!(json!(actual), old["groups"]);
    assert_eq!(
        json!(s.management_work_page(rev, "", None, None, 500).unwrap()),
        old["union"]
    );
    for report in s
        .management_report_page(rev, &ManagementReportFilter::default(), None, 500)
        .unwrap()
    {
        let old_report = old["reports"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == report.id)
            .unwrap();
        assert_eq!(report.rows, old_report["rows"].as_i64().unwrap());
        assert_eq!(report.saved_amount, old_report["savedAmount"].as_f64());
        assert_eq!(report.warnings, old_report["warnings"].as_i64().unwrap());
        let orgs = s
            .management_report_org_page(rev, &report.id, None, 500)
            .unwrap()
            .into_iter()
            .map(|(_, name)| name)
            .collect::<Vec<_>>();
        assert_eq!(json!(orgs), old_report["organizations"]);
    }
    assert_eq!(s.management_group_count(rev, "Second").unwrap(), 1);
    assert_eq!(
        s.management_catalog_count(rev, "one", Some("A")).unwrap(),
        1
    );
    let f = ManagementReportFilter::default();
    let total = s.management_report_summary(rev, &f).unwrap();
    assert_eq!(
        (total.reports, total.rows, total.amount, total.periods),
        (2, 2, 60., 2)
    );
    let first = s.management_report_page(rev, &f, None, 1).unwrap();
    assert_eq!(first[0].id, "later");
    assert_eq!(first[0].saved_amount, Some(30.));
    assert_eq!(
        s.management_report_page(rev, &f, Some(&first[0].cursor()), 1)
            .unwrap()[0]
            .id,
        "keep"
    );
    let f = ManagementReportFilter {
        report_type: Some("day".into()),
        ..Default::default()
    };
    let periods = s.management_period_page(rev, &f, None, 1).unwrap();
    assert_eq!(periods[0].start, "2026-11-02");
    assert_eq!(
        s.management_period_page(rev, &f, Some(&periods[0].start), 1)
            .unwrap()[0]
            .start,
        "2026-11-01"
    );
    assert_eq!(
        s.management_report_org_page(rev, "keep", None, 500)
            .unwrap(),
        vec![(0, "A".into()), (1, "B".into())]
    );
    let root = s.management_node_page(rev, None, None, 1).unwrap()[0].handle;
    let fields = s.management_node_page(rev, Some(root), None, 500).unwrap();
    let unknown = fields
        .iter()
        .find(|n| n.field.as_deref() == Some("unknown"))
        .unwrap();
    assert_eq!(unknown.kind, "object");
    assert_eq!(unknown.children, 2);
    assert_eq!(
        s.management_node_page(rev, Some(unknown.handle), None, 1)
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn management_stale_cancel_fault_and_private_session_cleanup_preserve_live_data() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let data = management_data();
    let rev = management_setup(&s, &data);
    let groups = s.management_group_page(rev, "", None, 500).unwrap();
    let job = Arc::new(ManagementJob::new());
    let mut edit = s
        .management_begin_settings(rev, 16 * 1024 * 1024, job.clone())
        .unwrap();
    edit.rename_group(groups[0].handle, "not committed")
        .unwrap();
    assert!(edit
        .set_scalar("orgNames", ManagementScalar::String("bad".into()))
        .is_err());
    assert_eq!(s.management_cleanup_sessions().unwrap(), (0, 1));
    job.cancel();
    assert!(s.management_commit_settings(edit).is_err());
    assert_eq!(s.load().unwrap(), data);
    assert_eq!(s.management_cleanup_sessions().unwrap(), (0, 0));
    let mut edit = s
        .management_begin_settings(rev, 16 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .unwrap();
    edit.rename_group(groups[0].handle, "stale").unwrap();
    let c = s.connect().unwrap();
    c.execute("UPDATE reports SET json=json WHERE id='keep'", [])
        .unwrap();
    assert!(s
        .management_commit_settings(edit)
        .unwrap_err()
        .contains("Stale"));
    let new = s.revision().unwrap();
    assert_eq!(s.load().unwrap(), data);
    let mut edit = s
        .management_begin_settings(new, 16 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .unwrap();
    edit.rename_group(groups[0].handle, "failed").unwrap();
    c.execute_batch("CREATE TRIGGER management_injected_failure BEFORE INSERT ON kv WHEN NEW.name='settings' BEGIN SELECT RAISE(ABORT,'injected settings write');END;").unwrap();
    assert!(s
        .management_commit_settings(edit)
        .unwrap_err()
        .contains("injected"));
    assert_eq!(s.load().unwrap(), data);
    assert_eq!(s.revision().unwrap(), new);
    assert_eq!(s.management_cleanup_sessions().unwrap(), (0, 0));
}
#[test]
fn management_session_sqlite_full_rolls_back_batch_and_reports_are_unaddressable() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let data = management_data();
    let rev = management_setup(&s, &data);
    let group = s.management_group_page(rev, "", None, 1).unwrap()[0].handle;
    let mut edit = s
        .management_begin_settings(rev, 4 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .unwrap();
    let huge = vec!["x".repeat(60000); 100];
    assert!(edit.append_members(group, &huge).is_err());
    assert!(edit.member_page(group, None, 500).unwrap().is_empty());
    edit.append_members(group, &["small".into()]).unwrap();
    let c = s.connect().unwrap();
    c.execute_batch("CREATE TRIGGER protect_reports_delete BEFORE DELETE ON reports BEGIN SELECT RAISE(ABORT,'settings touched reports');END;CREATE TRIGGER protect_reports_update BEFORE UPDATE ON reports BEGIN SELECT RAISE(ABORT,'settings touched reports');END;CREATE TRIGGER protect_reports_insert BEFORE INSERT ON reports BEGIN SELECT RAISE(ABORT,'settings touched reports');END;").unwrap();
    let receipt = s.management_commit_settings(edit).unwrap();
    assert!(receipt.revision > rev);
    assert_eq!(s.load().unwrap()["reports"], data["reports"]);
    assert_eq!(
        s.load().unwrap()["settings"]["groups"][0]["works"],
        json!(["small"])
    );
}
#[test]
fn management_live_sqlite_full_rolls_back_settings_blob_revision_and_indexes() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let data = management_data();
    let rev = management_setup(&s, &data);
    let c = s.connect().unwrap();
    let pages: i64 = c.query_row("PRAGMA page_count", [], |r| r.get(0)).unwrap();
    let mut edit = s
        .management_begin_settings(rev, 16 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .unwrap();
    edit.set_scalar("large", ManagementScalar::String("z".repeat(60000)))
        .unwrap();
    let error = s
        .management_commit_settings_with_limit(edit, Some(pages))
        .unwrap_err();
    assert!(error.contains("full"), "{error}");
    assert_eq!(s.revision().unwrap(), rev);
    assert_eq!(s.load().unwrap(), data);
    assert_eq!(s.management_bootstrap(rev).unwrap().memberships, 5);
}
#[test]
fn management_text_blob_roundtrip_delete_domain_and_unknown_shapes() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let data = management_data();
    let _rev = management_setup(&s, &data);
    let c = s.connect().unwrap();
    c.execute(
        "UPDATE kv SET json=CAST(json AS BLOB) WHERE name='settings'",
        [],
    )
    .unwrap();
    let rev = s.revision().unwrap();
    s.management_index_settings(rev, &ManagementJob::new())
        .unwrap();
    assert_eq!(s.load().unwrap()["settings"], data["settings"]);
    let groups = s.management_group_page(rev, "", None, 500).unwrap();
    let catalog = s.management_catalog_page(rev, "", None, None, 500).unwrap();
    let mut edit = s
        .management_begin_settings(rev, 16 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .unwrap();
    edit.delete_group(groups[1].handle).unwrap();
    edit.delete_catalog(&[catalog[1].handle]).unwrap();
    let created = edit.create_catalog("new catalog", Some("A")).unwrap();
    edit.update_catalog(created, "new updated", None).unwrap();
    let newgroup = edit.create_group("dup", "Retained duplicate").unwrap();
    edit.append_members(newgroup, &["new work".into(), "same".into()])
        .unwrap();
    let members = edit.member_page(newgroup, None, 500).unwrap();
    edit.remove_members(newgroup, &[members[0].handle]).unwrap();
    let receipt = s.management_commit_settings(edit).unwrap();
    let data = s.load().unwrap();
    assert_eq!(data["settings"]["groups"][0]["id"], "dup");
    assert_eq!(data["settings"]["groups"][0]["name"], "Empty");
    assert_eq!(data["settings"]["groups"][1]["name"], "Second");
    assert_eq!(data["settings"]["groups"][2]["works"], json!(["same"]));
    assert_eq!(
        data["settings"]["catalog"][1],
        json!({"work":"new updated"})
    );
    assert_eq!(
        s.management_catalog_count(receipt.revision, "", None)
            .unwrap(),
        2
    );
    assert!(s.management_group_page(rev, "", None, 1).is_err());
}
#[test]
fn management_restart_child_session() {
    let Some(root) = std::env::var_os("ANALIZ_MANAGEMENT_CRASH_STAGE") else {
        return;
    };
    let s = Storage::new(PathBuf::from(root)).unwrap();
    let rev = s.management_revision().unwrap();
    let edit = s
        .management_begin_settings(rev, 16 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .unwrap();
    std::mem::forget(edit);
    std::process::exit(0);
}
#[test]
fn management_restart_cleanup_respects_live_leases_and_preserves_unknown_dirs() {
    use std::process::Command;
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let rev = management_setup(&s, &management_data());
    let live = s
        .management_begin_settings(rev, 16 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("storage::management_tests::management_restart_child_session")
        .env("ANALIZ_MANAGEMENT_CRASH_STAGE", dir.path())
        .status()
        .unwrap();
    assert!(status.success());
    let foreign = dir.path().join("management-stages").join("user-directory");
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("keep"), "user data").unwrap();
    assert_eq!(s.management_cleanup_sessions().unwrap(), (1, 1));
    assert!(foreign.join("keep").exists());
    drop(live);
    assert_eq!(s.management_cleanup_sessions().unwrap(), (0, 0));
}
#[test]
fn management_bounded_bootstrap_refuses_implicit_legacy_snapshot_migration() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("reports.json"),
        management_data().to_string(),
    )
    .unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    assert!(s
        .management_revision()
        .unwrap_err()
        .contains("explicit migration"));
    let c = Connection::open(dir.path().join("reports.sqlite3")).unwrap();
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM reports", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(dir.path().join("reports.json").exists());
}
#[test]
fn management_shared_source_artifact_deletion_is_fail_closed_until_c3_lifecycle() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let mut data = management_data();
    for (field, value) in [
        ("sourceDiagnosticsReport", json!("shared-source")),
        ("sourceDiagnostics", json!({"issues":1,"hash":"x"})),
        ("sourceIssues", json!([{"kind":"invalid"}])),
    ] {
        data = management_data();
        data["reports"][0][field] = value;
        let rev = management_setup(&s, &data);
        assert!(
            s.management_report_page(rev, &ManagementReportFilter::default(), None, 1)
                .unwrap()[0]
                .shared_source_artifact
        );
        assert!(s
            .management_delete_reports(rev, &["keep".into()], &ManagementJob::new())
            .unwrap_err()
            .contains("Shared source"));
        assert!(s
            .management_clear_reports(rev, &ManagementJob::new())
            .is_err());
        assert_eq!(s.load().unwrap(), data);
    }
    let c = s.connect().unwrap();
    c.execute("UPDATE reports SET json=json WHERE id='keep'", [])
        .unwrap();
    let rev = s.revision().unwrap();
    assert!(s
        .management_clear_reports(rev, &ManagementJob::new())
        .unwrap_err()
        .contains("unindexed"));
    assert_eq!(s.load().unwrap(), data);
}
#[test]
fn management_bounds_corruption_no_partial_settings_and_session_limit() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let data = management_data();
    let rev = management_setup(&s, &data);
    assert!(s.management_group_page(rev, "", None, 0).is_err());
    assert!(s.management_member_page(rev, 999999, None, 500).is_err());
    assert!(s
        .management_catalog_page(rev, &"x".repeat(513), None, None, 1)
        .is_err());
    let mut sessions = Vec::new();
    for _ in 0..4 {
        sessions.push(
            s.management_begin_settings(rev, 4 * 1024 * 1024, Arc::new(ManagementJob::new()))
                .unwrap(),
        );
    }
    assert!(s
        .management_begin_settings(rev, 4 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .is_err());
    drop(sessions);
    assert_eq!(s.management_cleanup_sessions().unwrap(), (0, 0));
    let c = s.connect().unwrap();
    for raw in [
        b"{\"groups\":[],\"groups\":[],\"catalog\":[],\"orgNames\":[]}".to_vec(),
        vec![0xff, 0xfe],
        format!(
            "{{\"groups\":[],\"catalog\":[],\"orgNames\":[],\"unknown\":\"{}\"}}",
            "x".repeat(65537)
        )
        .into_bytes(),
    ] {
        c.execute("UPDATE kv SET json=?1 WHERE name='settings'", [&raw])
            .unwrap();
        let rev = s.revision().unwrap();
        assert!(s
            .management_index_settings(rev, &ManagementJob::new())
            .is_err());
        assert!(s
            .management_bootstrap(rev)
            .unwrap_err()
            .contains("unindexed"));
        assert_eq!(
            c.query_row("SELECT json FROM kv WHERE name='settings'", [], |r| r
                .get::<_, Vec<u8>>(0))
                .unwrap(),
            raw
        );
    }
}
#[test]
fn management_concurrent_writer_and_mid_parse_cancel_never_publish_old_settings() {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let mut data = management_data();
    data["settings"]["largeUnknown"] = json!((0..40000).collect::<Vec<_>>());
    s.save(data.clone()).unwrap();
    let c = s.connect().unwrap();
    let rev = s.revision().unwrap();
    let job = Arc::new(ManagementJob::new());
    let worker_job = job.clone();
    let root = dir.path().to_path_buf();
    let worker = thread::spawn(move || {
        Storage::new(root)
            .unwrap()
            .management_index_settings(rev, &worker_job)
    });
    let start = Instant::now();
    while job.records_processed() < 100 {
        assert!(start.elapsed() < Duration::from_secs(10));
        thread::yield_now();
    }
    c.execute("UPDATE kv SET json=json WHERE name='settings'", [])
        .unwrap();
    assert!(worker.join().unwrap().is_err());
    let rev = s.revision().unwrap();
    assert!(s.management_bootstrap(rev).is_err());
    assert_eq!(s.load().unwrap()["settings"], data["settings"]);
    let job = Arc::new(ManagementJob::new());
    let worker_job = job.clone();
    let root = dir.path().to_path_buf();
    let worker = thread::spawn(move || {
        Storage::new(root)
            .unwrap()
            .management_index_settings(rev, &worker_job)
    });
    let start = Instant::now();
    while job.records_processed() < 100 {
        assert!(start.elapsed() < Duration::from_secs(10));
        thread::yield_now();
    }
    assert!(job.cancel());
    assert!(worker.join().unwrap().is_err());
    assert_eq!(s.revision().unwrap(), rev);
    assert!(s.management_bootstrap(rev).is_err());
    assert_eq!(s.management_cleanup_sessions().unwrap(), (0, 0));
}
#[test]
fn management_report_metadata_requires_complete_raw_index_without_implicit_backfill() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let rev = management_setup(&s, &management_data());
    let c = s.connect().unwrap();
    c.execute("DELETE FROM report_row_index WHERE report_id='keep'", [])
        .unwrap();
    assert!(s
        .management_index_report("keep", rev, &ManagementJob::new())
        .unwrap_err()
        .contains("Raw report index"));
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM report_row_index WHERE report_id='keep'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    s.index_report("keep", rev).unwrap();
    s.management_index_report("keep", rev, &ManagementJob::new())
        .unwrap();
}
#[test]
fn management_delete_preserves_unrelated_kv_receipts_and_report_indexes() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let mut data = management_data();
    let mut other = data["reports"][0].clone();
    other["id"] = json!("other");
    data["reports"].as_array_mut().unwrap().push(other);
    let rev = management_setup(&s, &data);
    let c = s.connect().unwrap();
    c.execute(
        "INSERT INTO kv VALUES('unrelated','{\"keep\":[1,2,3]}')",
        [],
    )
    .unwrap();
    let operation = "a".repeat(64);
    c.execute(
        "INSERT INTO import_receipts VALUES(?1,?2,2,2,0)",
        params![operation, rev],
    )
    .unwrap();
    let rev = s.revision().unwrap();
    assert!(s
        .management_delete_reports(
            rev,
            &["keep".into(), "missing".into()],
            &ManagementJob::new()
        )
        .is_err());
    assert_eq!(s.management_bootstrap(rev).unwrap().reports, 2);
    let job = ManagementJob::new();
    let receipt = s
        .management_delete_reports(rev, &["keep".into(), "keep".into()], &job)
        .unwrap();
    assert_eq!(receipt.deleted, 1);
    assert!(!job.cancel());
    assert_eq!(
        s.management_bootstrap(receipt.revision)
            .unwrap()
            .indexed_reports,
        1
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM report_row_index WHERE report_id='other'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM import_receipts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("SELECT json FROM kv WHERE name='unrelated'", [], |r| r
            .get::<_, String>(
            0
        ))
        .unwrap(),
        "{\"keep\":[1,2,3]}"
    );
    let cleared = s
        .management_clear_reports(receipt.revision, &ManagementJob::new())
        .unwrap();
    assert_eq!(cleared.deleted, 1);
    assert_eq!(s.load().unwrap()["settings"], data["settings"]);
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM import_receipts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM kv WHERE name='unrelated'", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap(),
        1
    );
}

#[test]
fn c3_source_diagnostic_links_block_management_delete_even_without_json_sidecars() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let mut data = management_data();
    let mut other = data["reports"][0].clone();
    other["id"] = json!("other");
    data["reports"].as_array_mut().unwrap().push(other);
    let revision = management_setup(&s, &data);
    // Model a native-wide day's authoritative source-sidecar link. The JSON
    // metadata cache alone must never certify deletion of a linked report.
    let c = s.connect().unwrap();
    c.execute(
        "INSERT INTO report_source_diagnostic_links(report_id,anchor_id) VALUES('keep','keep')",
        [],
    ).unwrap();
    let before = s.load().unwrap();
    let error = s.management_delete_reports(
        revision,
        &["keep".into()],
        &ManagementJob::new(),
    ).unwrap_err();
    assert!(error.contains("source diagnostics"), "{error}");
    let error = s.management_clear_reports(
        revision,
        &ManagementJob::new(),
    ).unwrap_err();
    assert!(error.contains("source diagnostics"), "{error}");
    assert_eq!(s.revision().unwrap(), revision);
    assert_eq!(s.load().unwrap(), before);
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM report_source_diagnostic_links", [], |r| r.get::<_, i64>(0)).unwrap(),
        1,
    );
    // Ordinary unrelated reports can still be removed through bounded CRUD.
    let receipt = s.management_delete_reports(
        revision,
        &["other".into()],
        &ManagementJob::new(),
    ).unwrap();
    assert_eq!(receipt.deleted, 1);
    assert_eq!(s.load().unwrap()["reports"][0]["id"], "keep");
    assert_eq!(s.load().unwrap()["settings"], data["settings"]);
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM report_source_diagnostic_links", [], |r| r.get::<_, i64>(0)).unwrap(),
        1,
    );
}
