//! Run separately after fixture generation; report in-process Linux VmHWM.
use analiz_storage_core::storage::*;
use std::{fs, path::PathBuf, sync::Arc, time::Instant};
fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("management ROOT"));
    let input: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("probe.json")).unwrap()).unwrap();
    let n = input["items"].as_i64().unwrap();
    let started = Instant::now();
    let store = Storage::new(root.clone()).unwrap();
    let rev = store.management_revision().unwrap();
    store
        .management_index_settings(rev, &ManagementJob::new())
        .unwrap();
    store.index_report("keep", rev).unwrap();
    store
        .management_index_report("keep", rev, &ManagementJob::new())
        .unwrap();
    let boot = store.management_bootstrap(rev).unwrap();
    assert_eq!((boot.groups, boot.memberships, boot.catalog), (2, n, n));
    let big = store
        .management_group_page(rev, "Big", None, 1)
        .unwrap()
        .remove(0);
    assert_eq!(big.members, n);
    assert_eq!(
        store
            .management_member_page(rev, big.handle, None, 1)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .management_catalog_page(rev, "", None, None, 1)
            .unwrap()
            .len(),
        1
    );
    let before = store.management_work_summary(rev, "", None).unwrap();
    assert_eq!(before.total, 2 * n + 1);
    let mut edit = store
        .management_begin_settings(rev, 1024 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .unwrap();
    let g = edit.create_group("dup", "Added").unwrap();
    edit.append_members(
        g,
        &(0..500).map(|i| format!("added-{i}")).collect::<Vec<_>>(),
    )
    .unwrap();
    edit.append_members(g, &["added-500".into()]).unwrap();
    let cat = store
        .management_catalog_page(rev, "", None, None, 1)
        .unwrap();
    edit.update_catalog(cat[0].handle, "updated-catalog", Some("A"))
        .unwrap();
    edit.set_scalar("theme", ManagementScalar::String("dark".into()))
        .unwrap();
    let receipt = store.management_commit_settings(edit).unwrap();
    let boot = store.management_bootstrap(receipt.revision).unwrap();
    assert_eq!(
        (boot.groups, boot.memberships, boot.catalog, boot.reports),
        (3, n + 501, n, 1)
    );
    let works = store
        .management_work_summary(receipt.revision, "", None)
        .unwrap();
    assert_eq!(works.total, 2 * n + 502);
    assert_eq!(
        store
            .management_member_page(receipt.revision, big.handle, None, 1)
            .unwrap()
            .len(),
        1
    );
    let report = store
        .management_report_summary(receipt.revision, &ManagementReportFilter::default())
        .unwrap();
    assert_eq!((report.reports, report.rows, report.amount), (1, 1, 30.));
    let c = rusqlite::Connection::open(root.join("reports.sqlite3")).unwrap();
    let (kind, bytes): (String, i64) = c
        .query_row(
            "SELECT typeof(json),octet_length(json) FROM kv WHERE name='settings'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(kind, "blob");
    let rss = fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find(|l| l.starts_with("VmHWM:"))
        .unwrap()
        .to_string();
    println!("items={n} settings_bytes={} output_blob_bytes={bytes} groups={} memberships={} catalog={} works={} reports={} elapsed_seconds={:.3} {rss}",input["settings_bytes"],boot.groups,boot.memberships,boot.catalog,works.total,boot.reports,started.elapsed().as_secs_f64());
}
