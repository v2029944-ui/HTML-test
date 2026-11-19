use super::*;
use rusqlite::params;
use serde_json::json;

fn setup() -> (tempfile::TempDir, Storage, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let store = Storage::new(dir.path().to_owned()).unwrap();
    let conn = store.connect().unwrap();
    (dir, store, conn)
}

fn insert(conn: &Connection, id: &str, json: &str) {
    conn.execute(
        "INSERT INTO reports VALUES(?1,'day','2025-02-02','2025-02-02','control.xlsx',?2)",
        params![id, json],
    ).unwrap();
}

#[test]
fn pages_native_issues_and_preserves_excel_provenance() {
    let (_dir, store, conn) = setup();
    let issues: Vec<_> = (1..=52).map(|row| json!({
        "kind":"invalid","type":"invalid","reason":format!("Строка {row}"),
        "row":row,"source_row":row,"work_cell":format!("A{row}"),
        "count_cell":format!("F{row}"),"amount_cell":format!("AA{row}"),"date":"2025-02-02",
        "cells":[format!("A{row}"),format!("F{row}"),format!("AA{row}")]
    })).collect();
    insert(&conn, "day", &json!({"sourceSheet":"СМУ-16","issues":issues,"warnings":["Строка 1"]}).to_string());
    let rev = store.revision().unwrap();
    let first = store.report_quality_page(rev, "day", QualityScope::Report, 0, 50).unwrap();
    let next = first.next_offset.unwrap();
    assert_eq!(first.issues.len(), 50);
    let second = store.report_quality_page(rev, "day", QualityScope::Report, next, 50).unwrap();
    assert_eq!(second.issues.len(), 2);
    assert_eq!(second.next_offset, None);
    let issue = &second.issues[0];
    assert_eq!(issue.source_row, Some(51));
    assert_eq!(issue.sheet.as_deref(), Some("СМУ-16"));
    assert_eq!(issue.date.as_deref(), Some("2025-02-02"));
    assert_eq!(issue.work_cell.as_deref(), Some("A51"));
    assert_eq!(issue.count_cell.as_deref(), Some("F51"));
    assert_eq!(issue.amount_cell.as_deref(), Some("AA51"));
    assert_eq!(issue.kind.as_deref(), Some("invalid"));
    assert!(store.report_quality_page(rev - 1, "day", QualityScope::Report, 0, 50).is_err());
    assert!(store.report_quality_page(rev, "day", QualityScope::Report, 0, 51).is_err());
}

#[test]
fn follows_live_source_link_and_exposes_shared_diagnostics_once() {
    let (_dir, store, conn) = setup();
    insert(&conn, "day", r#"{"sourceDiagnosticsReport":"old","issues":[{"reason":"local"},{"reason":"projection","source_issue_sequence":1}],"warnings":["projection"]}"#);
    insert(&conn, "live", r#"{"sourceDiagnosticsReport":"old","sourceIssues":[{"reason":"projection","sheet":"Квартал","count_cell":"BA900","amount_cell":"BB900","source_row":900,"relevant":true},{"reason":"deferred","sheet":"Квартал","relevant":false}]}"#);
    conn.execute("INSERT INTO report_source_diagnostic_links VALUES('day','live'),('live','live')", []).unwrap();
    let rev = store.revision().unwrap();
    let local = store.report_quality_page(rev, "day", QualityScope::Report, 0, 50).unwrap();
    assert_eq!(local.issues.len(), 1);
    assert_eq!(local.issues[0].reason, "local");
    assert_eq!(local.source_anchor.as_deref(), Some("live"));
    let source = store.report_quality_page(rev, "day", QualityScope::Source, 0, 1).unwrap();
    assert_eq!(source.source_anchor.as_deref(), Some("live"));
    assert_eq!(source.issues[0].reason, "projection");
    assert_eq!(source.issues[0].count_cell.as_deref(), Some("BA900"));
    assert_eq!(source.issues[0].relevant, Some(true));
    let second = store.report_quality_page(rev, "day", QualityScope::Source, source.next_offset.unwrap(), 1).unwrap();
    assert_eq!(second.issues[0].reason, "deferred");
    assert_eq!(second.issues[0].relevant, Some(false));
    assert_eq!(second.next_offset, None);
}

#[test]
fn warning_fallback_missing_malformed_and_empty_are_distinguishable() {
    let (_dir, store, conn) = setup();
    insert(&conn, "old", r#"{"warnings":["legacy warning"]}"#);
    insert(&conn, "empty", r#"{"warnings":[]}"#);
    insert(&conn, "bad", r#"{"issues":[{"reason":"first"}],"rows":["#);
    insert(&conn, "lost", r#"{"sourceDiagnosticsReport":"gone","issues":[{"reason":"saved copy","source_issue_sequence":1}]}"#);
    conn.execute("INSERT INTO report_source_diagnostic_links VALUES('lost','gone')", []).unwrap();
    insert(&conn, "deep", &format!(r#"{{"rows":{}null{}}}"#, "[".repeat(129), "]".repeat(129)));
    insert(&conn, "legacy", r#"{"sourceDiagnosticsReport":"anchor"}"#);
    insert(&conn, "anchor", r#"{"sourceIssues":[{"reason":"classic","type":"invalid","row":42,"cells":["A42","D42"]}]}"#);
    let rev = store.revision().unwrap();
    let get = |id| store.report_quality_page(rev,id,QualityScope::Report,0,50);
    let old = get("old").unwrap();
    assert_eq!(old.issues[0].reason, "legacy warning");
    assert!(old.warning_fallback);
    let empty = get("empty").unwrap();
    assert_eq!(empty.issues.len(), 0);
    assert!(!empty.warning_fallback);
    assert!(get("bad").is_err());
    assert!(get("missing").is_err());
    assert!(get("deep").is_err());
    assert_eq!(get("lost").unwrap().issues[0].reason, "saved copy");
    assert!(store.report_quality_page(rev,"lost",QualityScope::Source,0,50).is_err());
    let classic = store.report_quality_page(rev,"legacy",QualityScope::Source,0,50).unwrap();
    assert_eq!(classic.issues[0].source_row, Some(42));
    assert_eq!(classic.issues[0].cells, ["A42", "D42"]);
}

#[test]
fn streams_large_row_payloads_and_bounds_issue_fields() {
    let (_dir, store, conn) = setup();
    let raw = format!(r#"{{"rows":[{}null],"issues":[{{"reason":"{}"}}]}}"#,
        r#"{"organization":"ignored","raw":[1,2,3]},"#.repeat(200_000), "Я".repeat(8000));
    insert(&conn, "big", &raw);
    drop(raw);
    let page = store.report_quality_page(store.revision().unwrap(), "big", QualityScope::Report, 0, 50).unwrap();
    assert_eq!(page.issues.len(), 1);
    assert!(page.issues[0].reason.len() <= 4096);
    assert!(page.issues[0].reason.ends_with('…'));
}

#[test]
fn keeps_distinct_warnings_after_structured_issues_across_pages() {
    let (_dir, store, conn) = setup();
    insert(&conn, "mixed", r#"{"warnings":["shared","by itself","by itself"],"issues":[{"reason":"shared"}]}"#);
    let rev = store.revision().unwrap();
    let first = store.report_quality_page(rev, "mixed", QualityScope::Report, 0, 1).unwrap();
    assert_eq!(first.issues[0].reason, "shared");
    let second = store.report_quality_page(rev, "mixed", QualityScope::Report, first.next_offset.unwrap(), 1).unwrap();
    assert_eq!(second.issues[0].reason, "by itself");
    assert_eq!(second.next_offset, None);
}

#[test]
fn malformed_diagnostics_do_not_hide_valid_siblings() {
    let (_dir, store, conn) = setup();
    insert(&conn, "mixed", r#"{"warnings":[null,"still useful",{"text":"bad"}],"issues":[null,{"kind":"missing-reason"},{"reason":"valid issue","organization":false,"cells":["A1",null,"C1"]},{"reason":"other valid issue"}]}"#);
    let rev = store.revision().unwrap();
    let page = store.report_quality_page(rev, "mixed", QualityScope::Report, 0, 50).unwrap();
    assert_eq!(page.issues.iter().map(|issue| issue.reason.as_str()).collect::<Vec<_>>(), ["valid issue","other valid issue","still useful"]);
    assert_eq!(page.issues[0].cells, ["A1","C1"]);
    assert!(serde_json::to_value(&page).unwrap()["invalid_entries"].as_u64().unwrap() >= 4);
}
