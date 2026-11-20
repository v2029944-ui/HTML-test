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

#[test]
fn evidence_retains_old_unknown_and_saved_false_without_inventing_completeness() {
    let (_dir,store,conn)=setup();
    insert(&conn,"old",r#"{"warnings":["missing day maybe"],"hasMismatch":true,"sourceSheet":"physical.xml"}"#);
    insert(&conn,"tagged",r#"{"sourceFormat":"daily-wide","sheetName":"Q1","sourceSheet":"physical.xml","explicitZero":false,"sourceCoverage":{"start":"2024-01-01","end":"2024-03-31"},"issues":[{"kind":"mismatch","reason":"a control warning"}]}"#);
    let revision=store.revision().unwrap();
    let old=store.report_quality_page(revision,"old",QualityScope::Report,0,50).unwrap();
    assert_eq!(old.evidence.document_id,"old"); assert_eq!(old.evidence.explicit_zero,None); assert_eq!(old.evidence.sheet_name,None); assert_eq!(old.evidence.invalid_fields,0); assert_eq!(old.evidence.missing_day_entries,0); assert_eq!(old.evidence.omitted_row_entries,0);
    let tagged=store.report_quality_page(revision,"tagged",QualityScope::Report,0,50).unwrap();
    assert_eq!(tagged.evidence.explicit_zero,Some(false)); assert_eq!(tagged.evidence.sheet_name.as_deref(),Some("Q1")); assert_eq!(tagged.evidence.source_format.as_deref(),Some("daily-wide"));
    assert_eq!(tagged.evidence.coverage.as_ref().unwrap().end,"2024-03-31"); assert_eq!(tagged.evidence.invalid_fields,0); assert_eq!(tagged.issues[0].sheet.as_deref(),Some("physical.xml"));
    // Declared source coverage need not contain this report's metadata date.
    assert_eq!(tagged.evidence.missing_day_entries,0);
}

#[test]
fn evidence_counts_all_active_entries_and_distinguishes_deferred_unknown_and_omitted() {
    let (_dir,store,conn)=setup();
    let mut issues:Vec<_>=(0..53).map(|i|json!({"kind":"missing-day","reason":format!("day {i}"),"scope":"source-period","date":"2024-01-01","relevant":i%2==0})).collect();
    issues.extend([
        json!({"kind":"missing-day","reason":"old unknown","scope":"source-period","date":"2024-01-01"}),
        json!({"kind":"missing-day","reason":"bad date","scope":"source-period","date":"2024-02-30","relevant":true}),
        json!({"kind":"missing-day","reason":"no scope","date":"2024-01-01","relevant":true}),
        json!({"kind":"invalid","type":"invalid","reason":"skipped row","scope":"report","skipped":true}),
        json!({"type":"invalid","reason":"old skipped row","skipped":true}),
        json!({"kind":"invalid","reason":"source control","scope":"source-period","skipped":true}),
        json!({"kind":"empty-name","reason":"empty name","scope":"report","skipped":true}),
        json!({"kind":"mismatch","reason":"not a missing day","scope":"source-period","relevant":true}),
    ]);
    insert(&conn,"pages",&json!({"issues":issues,"sourceIssues":[{"kind":"missing-day","reason":"ignored source copy","scope":"source-period","date":"2024-01-01","relevant":true}]}).to_string());
    insert(&conn,"source-pages",&json!({"sourceIssues":issues,"issues":[{"kind":"missing-day","reason":"ignored local copy","scope":"source-period","date":"2024-01-01","relevant":true}]}).to_string());
    let revision=store.revision().unwrap();
    let first=store.report_quality_page(revision,"pages",QualityScope::Report,0,50).unwrap();
    let second=store.report_quality_page(revision,"pages",QualityScope::Report,50,50).unwrap();
    assert_eq!(first.next_offset,Some(50)); assert_eq!(second.next_offset,None);
    for page in [&first,&second] { assert_eq!(page.evidence.missing_day_entries,27); assert_eq!(page.evidence.deferred_day_entries,26); assert_eq!(page.evidence.unclassified_day_entries,3); assert_eq!(page.evidence.omitted_row_entries,2); }
    assert_eq!(first.issues[1].relevant,Some(false)); assert_eq!(first.issues[1].scope.as_deref(),Some("source-period"));
    assert_eq!(second.issues[6].skipped,Some(true));
    assert_eq!(serde_json::to_value(&first.evidence).unwrap(),serde_json::to_value(&second.evidence).unwrap());
    let source_first=store.report_quality_page(revision,"source-pages",QualityScope::Source,0,50).unwrap();
    let source_second=store.report_quality_page(revision,"source-pages",QualityScope::Source,50,50).unwrap();
    for page in [&source_first,&source_second] { assert_eq!(page.evidence.missing_day_entries,27); assert_eq!(page.evidence.deferred_day_entries,26); assert_eq!(page.evidence.unclassified_day_entries,3); assert_eq!(page.evidence.omitted_row_entries,2); }
    assert_eq!(serde_json::to_value(&source_first.evidence).unwrap(),serde_json::to_value(&source_second.evidence).unwrap());
}

#[test]
fn malformed_metadata_and_classification_cannot_assert_saved_facts() {
    let (_dir,store,conn)=setup();
    insert(&conn,"bad",r#"{"sourceFormat":false,"sheetName":{},"explicitZero":"true","sourceCoverage":{"start":"2024-2-01","end":"2024-01-01"},"issues":[{"kind":"missing-day","reason":"duplicate relevance","scope":"source-period","date":"2024-01-01","relevant":false,"relevant":true},{"kind":"invalid","reason":"duplicate scope","skipped":true,"scope":"source-period","scope":"report"},{"kind":"invalid","reason":"duplicate skipped","skipped":false,"skipped":true},{"kind":"missing-day","reason":"duplicate date","scope":"source-period","date":"2024-01-01","date":"2024-01-02","relevant":true},{"kind":"missing-day","type":"invalid","reason":"conflicting kinds","scope":"source-period","date":"2024-01-01","relevant":true},{"kind":"missing-day","kind":"missing-day","reason":"duplicate kind","scope":"source-period","date":"2024-01-01","relevant":true},{"type":"invalid","type":"invalid","reason":"duplicate type","skipped":true},{"kind":"missing-day","reason":"malformed relevance","scope":"source-period","date":"2024-01-01","relevant":"yes"}]}"#);
    for (id,raw) in [
        ("dupformat",r#"{"sourceFormat":"a","sourceFormat":"a"}"#),
        ("dupsheet",r#"{"sheetName":"a","sheetName":"a"}"#),
        ("dupzero",r#"{"explicitZero":false,"explicitZero":true}"#),
        ("dupcoverage",r#"{"sourceCoverage":null,"sourceCoverage":null}"#),
        ("dupstart",r#"{"sourceCoverage":{"start":"2024-01-01","start":"2024-01-01","end":"2024-02-01"}}"#),
        ("dupend",r#"{"sourceCoverage":{"start":"2024-01-01","end":"2024-02-01","end":"2024-02-01"}}"#),
    ] { insert(&conn,id,raw); }
    let revision=store.revision().unwrap();
    let page=store.report_quality_page(revision,"bad",QualityScope::Report,0,50).unwrap();
    assert_eq!(page.issues.len(),8); assert_eq!(page.evidence.source_format,None); assert_eq!(page.evidence.sheet_name,None); assert_eq!(page.evidence.coverage,None); assert_eq!(page.evidence.explicit_zero,None);
    assert!(page.evidence.invalid_fields>=12); assert_eq!(page.evidence.missing_day_entries,0); assert_eq!(page.evidence.deferred_day_entries,0); assert_eq!(page.evidence.omitted_row_entries,0); assert!(page.evidence.unclassified_day_entries>=3);
    assert_eq!(page.issues[0].relevant,None); assert_eq!(page.issues[4].kind,None); assert_eq!(page.issues[2].skipped,None);
    for id in ["dupformat","dupsheet","dupzero","dupcoverage","dupstart","dupend"] { assert!(store.report_quality_page(revision,id,QualityScope::Report,0,50).is_err(),"accepted {id}"); }
}

#[test]
fn source_evidence_uses_live_anchor_and_mirror_omission_but_lost_relevance_stays_unknown() {
    let (_dir,store,conn)=setup();
    insert(&conn,"selected",r#"{"sourceDiagnosticsReport":"old","sheetName":"selected sheet","explicitZero":false,"issues":[{"kind":"missing-day","reason":"mirror","source_issue_sequence":1,"scope":"source-period","date":"2024-01-01","relevant":true}]}"#);
    insert(&conn,"live",r#"{"sheetName":"anchor sheet","explicitZero":true,"sourceFormat":"daily-wide","sourceIssues":[{"kind":"missing-day","reason":"live deferred","scope":"source-period","date":"2024-01-01","relevant":false}]}"#);
    insert(&conn,"lost",r#"{"sourceDiagnosticsReport":"gone","issues":[{"kind":"missing-day","reason":"lost copy","source_issue_sequence":1,"scope":"source-period","date":"2024-01-01"}]}"#);
    conn.execute("INSERT INTO report_source_diagnostic_links VALUES('selected','live'),('lost','gone')",[]).unwrap();
    let revision=store.revision().unwrap();
    let selected=store.report_quality_page(revision,"selected",QualityScope::Report,0,50).unwrap(); assert!(selected.issues.is_empty()); assert_eq!(selected.evidence.document_id,"selected"); assert_eq!(selected.evidence.explicit_zero,Some(false)); assert_eq!(selected.evidence.missing_day_entries,0);
    let source=store.report_quality_page(revision,"selected",QualityScope::Source,0,50).unwrap(); assert_eq!(source.evidence.document_id,"live"); assert_eq!(source.evidence.sheet_name.as_deref(),Some("anchor sheet")); assert_eq!(source.evidence.explicit_zero,Some(true)); assert_eq!(source.evidence.deferred_day_entries,1);
    let lost=store.report_quality_page(revision,"lost",QualityScope::Report,0,50).unwrap(); assert_eq!(lost.issues.len(),1); assert_eq!(lost.evidence.missing_day_entries,0); assert_eq!(lost.evidence.unclassified_day_entries,1);
}

#[test]
fn evidence_reads_streamed_rows_and_preserves_live_data_and_schema_bytes() {
    let (_dir,store,conn)=setup();
    let raw=format!(r#"{{"rows":[{}null],"sheetName":"large","explicitZero":true,"issues":[{{"kind":"invalid","reason":"saved skipped row","skipped":true}}]}}"#,r#"{"organization":"ignored","work":"Work","count":1,"amount":10},"#.repeat(200_000));
    insert(&conn,"large-evidence",&raw); drop(raw);
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    let revision=store.revision().unwrap(); let before=std::fs::read(store.db_path()).unwrap();
    let page=store.report_quality_page(revision,"large-evidence",QualityScope::Report,0,1).unwrap(); assert_eq!(page.evidence.omitted_row_entries,1); assert_eq!(page.evidence.explicit_zero,Some(true));
    assert_eq!(std::fs::read(store.db_path()).unwrap(),before); assert_eq!(store.revision().unwrap(),revision);
    assert!(store.report_quality_page(revision-1,"large-evidence",QualityScope::Report,0,1).is_err());
}

#[test]
fn native_nullable_fields_are_unknown_without_erasing_proven_skipped_rows() {
    let (_dir,store,conn)=setup();
    insert(&conn,"native-null",r#"{"sourceFormat":null,"sheetName":null,"sourceCoverage":null,"explicitZero":null,"issues":[{"kind":"invalid","type":null,"reason":"saved omitted row","skipped":true,"date":null,"scope":null,"relevant":null},{"kind":"missing-day","reason":"unknown relevance","date":"2024-01-01","scope":"source-period","relevant":null,"skipped":null},{"kind":"mismatch","reason":"native source control","date":null,"scope":"source-period","relevant":true,"skipped":null},{"kind":null,"type":"invalid","reason":"nullable native alias","skipped":true,"scope":"report"}]}"#);
    insert(&conn,"duplicate-null",r#"{"issues":[{"kind":"invalid","reason":"duplicate null remains ambiguous","skipped":true,"scope":null,"scope":null}]}"#);
    insert(&conn,"wrong-types",r#"{"issues":[{"kind":"invalid","reason":"bad scope","skipped":true,"scope":false},{"kind":"invalid","reason":"bad date","skipped":true,"date":"2024-02-30"},{"kind":"missing-day","reason":"bad relevance","scope":"source-period","date":"2024-01-01","relevant":"true"}]}"#);
    let revision=store.revision().unwrap();
    let page=store.report_quality_page(revision,"native-null",QualityScope::Report,0,50).unwrap();
    assert_eq!(page.evidence.invalid_fields,0);
    assert_eq!(page.evidence.omitted_row_entries,2);
    assert_eq!(page.evidence.missing_day_entries,0);
    assert_eq!(page.evidence.deferred_day_entries,0);
    assert_eq!(page.evidence.unclassified_day_entries,1);
    assert_eq!(page.issues[0].skipped,Some(true)); assert_eq!(page.issues[0].scope,None); assert_eq!(page.issues[0].date,None); assert_eq!(page.issues[1].relevant,None);
    assert_eq!(page.evidence.source_format,None); assert_eq!(page.evidence.sheet_name,None); assert_eq!(page.evidence.coverage,None); assert_eq!(page.evidence.explicit_zero,None);
    let duplicate=store.report_quality_page(revision,"duplicate-null",QualityScope::Report,0,50).unwrap(); assert_eq!(duplicate.evidence.invalid_fields,1); assert_eq!(duplicate.evidence.omitted_row_entries,0);
    let wrong=store.report_quality_page(revision,"wrong-types",QualityScope::Report,0,50).unwrap(); assert_eq!(wrong.evidence.invalid_fields,3); assert_eq!(wrong.evidence.omitted_row_entries,1); assert_eq!(wrong.evidence.unclassified_day_entries,1);
}

#[test]
fn malformed_fields_do_not_erase_independent_saved_facts() {
    let (_dir,store,conn)=setup();
    insert(&conn,"independent",r#"{"issues":[{"kind":"invalid","reason":"omitted row with bad date","scope":"report","skipped":true,"date":"not a date"},{"kind":"invalid","reason":"omitted row with bad relevance","scope":"report","skipped":true,"relevant":"unknown"},{"kind":"missing-day","reason":"missing day with bad skipped flag","scope":"source-period","date":"2024-01-01","relevant":true,"skipped":"bad"},{"kind":"missing-day","reason":"deferred day with duplicate skipped flag","scope":"source-period","date":"2024-01-01","relevant":false,"skipped":false,"skipped":true},{"kind":"invalid","reason":"ambiguous scope cannot become absent","scope":"report","scope":null,"skipped":true}]}"#);
    let page=store.report_quality_page(store.revision().unwrap(),"independent",QualityScope::Report,0,50).unwrap();
    assert_eq!(page.evidence.invalid_fields,5); assert_eq!(page.evidence.omitted_row_entries,2); assert_eq!(page.evidence.missing_day_entries,1); assert_eq!(page.evidence.deferred_day_entries,1); assert_eq!(page.evidence.unclassified_day_entries,0);
    assert_eq!(page.issues[0].skipped,Some(true)); assert_eq!(page.issues[0].date,None); assert_eq!(page.issues[1].skipped,Some(true)); assert_eq!(page.issues[2].relevant,Some(true)); assert_eq!(page.issues[3].relevant,Some(false)); assert_eq!(page.issues[4].skipped,None);
}

#[test]
fn explicitly_skipped_unknown_organization_rows_count_without_inventing_attribution() {
    let (_dir,store,conn)=setup();
    insert(&conn,"unassigned-skips",r#"{"issues":[{"kind":"skipped","reason":"native omitted row","skipped":true,"organization":null,"source_row":7},{"kind":"skipped","reason":"old unmarked note"},{"kind":"skipped","reason":"shared source note","skipped":true,"scope":"source-period"},{"kind":"empty-name","reason":"included row","skipped":true}]}"#);
    let page=store.report_quality_page(store.revision().unwrap(),"unassigned-skips",QualityScope::Report,0,50).unwrap();
    assert_eq!(page.evidence.omitted_row_entries,1);assert_eq!(page.issues[0].organization,None);assert_eq!(page.issues[0].source_row,Some(7));assert_eq!(page.issues[0].skipped,Some(true));
}
