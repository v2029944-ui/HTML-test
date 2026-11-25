use super::*;
use tempfile::tempdir;

#[test]
fn restored_reports_preserve_quality_without_certifying_a_partial_raw_index() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let conn = store.connect().unwrap();
    let first = "2024-02-01";
    for (id, end, org) in [
        ("invalid-period", "2024-02-03", json!("A")),
        ("omitted-org", first, json!({"legacy":true})),
        ("ordinary", first, json!("A")),
    ] {
        let report = json!({"id":id, "type":"day", "start":first, "end":end,
            "organizations":["A"], "warnings":["Restored report warning"],
            "rows":[{"work":"service","organization":org,"amount":7,"count":1}]});
        conn.execute(
            "INSERT INTO reports(id,report_type,period_start,period_end,json) VALUES(?1,'day',?2,?3,?4)",
            params![id, first, end, report.to_string()],
        ).unwrap();
    }
    let revision = store.revision().unwrap();
    let headers = store
        .report_page(revision, &ReadFilter::default(), None, 20)
        .unwrap();
    assert_eq!(headers.len(), 3);
    for header in headers {
        store
            .index_unified_startup_report(&header, revision, &ManagementJob::new())
            .unwrap();
        let page = store
            .report_quality_page(revision, &header.id, QualityScope::Report, 0, 50)
            .unwrap();
        assert_eq!(
            serde_json::to_value(page).unwrap()["issues"][0]["reason"],
            "Restored report warning"
        );
    }
    let flags: (bool, bool, bool, bool) = conn.query_row(
        "SELECT
            EXISTS(SELECT 1 FROM analysis_reports WHERE report_id='invalid-period' AND issue='invalid_bounds'),
            EXISTS(SELECT 1 FROM management_report_meta WHERE report_id='invalid-period' AND amount=7),
            EXISTS(SELECT 1 FROM report_row_index WHERE report_id='omitted-org'),
            EXISTS(SELECT 1 FROM management_report_meta WHERE report_id='ordinary' AND amount=7)",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    ).unwrap();
    assert_eq!(flags, (true, true, false, true));
    assert_eq!(store.revision().unwrap(), revision);
}
