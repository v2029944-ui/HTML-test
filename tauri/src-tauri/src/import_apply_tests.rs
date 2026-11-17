use super::*;
use analiz_import_core::{LegacyOptions, Limits, PreparedImport, StageArea};
use std::{io::Write, sync::atomic::AtomicBool};
use zip::{write::SimpleFileOptions, ZipWriter};

pub(super) fn fixture(path: &Path, rows: usize) {
    let mut zip = ZipWriter::new(fs::File::create(path).unwrap());
    zip.start_file("xl/worksheets/sheet1.xml", SimpleFileOptions::default())
        .unwrap();
    write!(zip, "<worksheet><sheetData><row r=\"2\"><c r=\"A2\" t=\"inlineStr\"><is><t>СМУ16</t></is></c><c r=\"B2\"><v>{rows}</v></c><c r=\"C2\"><v>{}</v></c></row>", rows * 2).unwrap();
    for n in 3..rows + 3 {
        write!(zip, "<row r=\"{n}\"><c r=\"A{n}\" t=\"inlineStr\"><is><t>Работа {n}</t></is></c><c r=\"B{n}\"><v>1</v></c><c r=\"C{n}\"><v>2</v></c></row>").unwrap();
    }
    write!(zip, "</sheetData></worksheet>").unwrap();
    zip.finish().unwrap();
}
pub(super) fn prepared<'a>(
    area: &'a StageArea,
    source: &Path,
    kind: &str,
    start: &str,
) -> PreparedImport<'a> {
    let cancel = AtomicBool::new(false);
    let mut stage = area
        .ingest(source, Limits::default(), &cancel, |_| {})
        .unwrap();
    stage
        .convert_legacy(
            &LegacyOptions {
                sheet: "xl/worksheets/sheet1.xml".into(),
                kind: kind.into(),
                start: start.into(),
                organizations: ["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
                mappings: Some(vec![analiz_import_core::TripleMapping {
                    header_row: 1,
                    work_col: 1,
                    count_col: 2,
                    amount_col: 3,
                }]),
            },
            &cancel,
            |_| {},
        )
        .unwrap();
    stage.prepare(&cancel, |_| {}).unwrap()
}
pub(super) fn original(storage: &Storage) -> Value {
    let data = json!({"reports":[{"id":"old","type":"day","start":"2025-01-01","end":"2025-01-01","rows":[{"organization":"A","work":"old","count":3,"amount":4}]}],"settings":{"custom":"keep","orgNames":["A","B","C","D"],"groups":[],"catalog":[]}});
    storage.save(data.clone()).unwrap();
    data
}
#[test]
fn prepared_preview_apply_roundtrip_backup_and_receipt() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("данные.xlsx");
    fixture(&source, 120);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = prepared(&area, &source, "day", "2026-11-01");
    let storage = Storage::new(root.path().join("live")).unwrap();
    let before = original(&storage);
    let rev = storage.revision().unwrap();
    let preview = storage.preview_import(&p).unwrap();
    assert_eq!(preview.revision(), rev);
    assert_eq!(preview.counts().new_reports, 1);
    assert_eq!(storage.load().unwrap(), before);
    let job = ImportJob::new();
    let receipt = storage
        .apply_import(&p, &preview, ImportConfirmation::default(), &job, |_| {})
        .unwrap();
    assert_eq!(receipt.reports, 1);
    assert_eq!(receipt.rows, 120);
    assert!(receipt.revision > rev);
    let loaded = storage.load().unwrap();
    assert_eq!(loaded["settings"], before["settings"]);
    assert_eq!(loaded["reports"][0], before["reports"][0]);
    assert_eq!(loaded["reports"][1]["rows"].as_array().unwrap().len(), 120);
    assert_eq!(loaded["reports"][1]["desktopSource"], "данные.xlsx");
    let filter = ReadFilter::default();
    assert_eq!(
        storage
            .row_page(receipt.revision, &filter, None, 1)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        storage.aggregate(receipt.revision, &filter).unwrap().rows,
        121
    );
    storage
        .backup_sqlite(&root.path().join("backup.sqlite3"))
        .unwrap();
    let conn = Connection::open(root.path().join("backup.sqlite3")).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM report_rows", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        121
    );
    assert_eq!(
        conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert_eq!(
        conn.query_row("SELECT typeof(json) FROM reports WHERE id='old'", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "text"
    );
    let live_conn = storage.connect().unwrap();
    let imported_id = loaded["reports"][1]["id"].as_str().unwrap();
    assert!(live_conn
        .query_row("SELECT json FROM reports WHERE id=?1", [imported_id], |r| r
            .get::<_, String>(0))
        .is_err());
    live_conn
        .execute(
            "DELETE FROM report_row_index WHERE report_id=?1",
            [imported_id],
        )
        .unwrap();
    live_conn
        .execute("DELETE FROM report_rows WHERE report_id=?1", [imported_id])
        .unwrap();
    assert_eq!(
        storage.index_report(imported_id, receipt.revision).unwrap(),
        120
    );
    fs::remove_file(p.directory().join("report-1.json")).unwrap();
    fs::write(&source, b"changed after committed").unwrap();
    assert!(!job.cancel());
    assert_eq!(
        storage
            .apply_import(&p, &preview, ImportConfirmation::default(), &job, |_| {})
            .unwrap(),
        receipt
    );
    let reopened = Storage::new(root.path().join("live")).unwrap();
    assert_eq!(
        reopened.committed_import(p.operation_id()).unwrap(),
        Some(receipt)
    );
    assert_eq!(reopened.load().unwrap(), loaded);
}
#[test]
fn stale_source_files_revision_and_cancel_preserve_original() {
    for fault in [
        "source", "missing", "changed", "revision", "kv", "conflict", "cancel", "mid-copy",
    ] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("data.xlsx");
        fixture(&source, 120);
        let area = StageArea::open(root.path().join("stage")).unwrap();
        let p = prepared(&area, &source, "day", "2026-11-01");
        let storage = Storage::new(root.path().join("live")).unwrap();
        original(&storage);
        let preview = storage.preview_import(&p).unwrap();
        let job = ImportJob::new();
        match fault {
            "source" => fs::write(&source, b"changed").unwrap(),
            "missing" => fs::remove_file(p.directory().join("report-1.json")).unwrap(),
            "changed" => fs::write(p.directory().join("report-1.json"), b"{}").unwrap(),
            "revision" => {
                storage.save(storage.load().unwrap()).unwrap();
            }
            "kv" => {
                storage
                    .connect()
                    .unwrap()
                    .execute("INSERT INTO kv VALUES('other','123')", [])
                    .unwrap();
            }
            "conflict" => {
                let mut data = storage.load().unwrap();
                data["reports"].as_array_mut().unwrap().push(json!({"id":"new-conflict","type":"day","start":"2026-11-01","end":"2026-11-01","rows":[]}));
                storage.save(data).unwrap();
            }
            "cancel" => {
                assert!(job.cancel());
            }
            _ => {}
        }
        let before = storage.load().unwrap();
        let rev = storage.revision().unwrap();
        assert!(
            storage
                .apply_import(&p, &preview, ImportConfirmation::default(), &job, |event| {
                    if fault == "mid-copy" && event.phase == "copy" {
                        job.cancel();
                    }
                })
                .is_err(),
            "{fault}"
        );
        assert_eq!(storage.load().unwrap(), before, "{fault}");
        assert_eq!(storage.revision().unwrap(), rev, "{fault}");
    }
}
#[test]
fn explicit_replacement_and_overlap_confirmation() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 2);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let storage = Storage::new(root.path().join("live")).unwrap();
    original(&storage);
    let p = prepared(&area, &source, "week", "2026-11-02");
    let preview = storage.preview_import(&p).unwrap();
    storage
        .apply_import(
            &p,
            &preview,
            ImportConfirmation::default(),
            &ImportJob::new(),
            |_| {},
        )
        .unwrap();
    let p2 = prepared(&area, &source, "week", "2026-11-02");
    let preview2 = storage.preview_import(&p2).unwrap();
    assert_eq!(preview2.counts().replacements, 1);
    assert!(storage
        .apply_import(
            &p2,
            &preview2,
            ImportConfirmation::default(),
            &ImportJob::new(),
            |_| {}
        )
        .is_err());
    storage
        .apply_import(
            &p2,
            &preview2,
            ImportConfirmation {
                replace_exact: true,
                accept_cross_type: false,
                ..Default::default()
            },
            &ImportJob::new(),
            |_| {},
        )
        .unwrap();
    let overlap = prepared(&area, &source, "week", "2026-11-03");
    assert!(storage.preview_import(&overlap).is_err());
    let day = prepared(&area, &source, "day", "2026-11-03");
    let dp = storage.preview_import(&day).unwrap();
    assert_eq!(dp.counts().cross_type_overlaps, 1);
    assert!(storage
        .apply_import(
            &day,
            &dp,
            ImportConfirmation::default(),
            &ImportJob::new(),
            |_| {}
        )
        .is_err());
    storage
        .apply_import(
            &day,
            &dp,
            ImportConfirmation {
                replace_exact: false,
                accept_cross_type: true,
                ..Default::default()
            },
            &ImportJob::new(),
            |_| {},
        )
        .unwrap();
}
#[test]
fn unconverted_and_empty_stages_cannot_prepare() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 2);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    assert!(area
        .ingest(&source, Limits::default(), &cancel, |_| {})
        .unwrap()
        .prepare(&cancel, |_| {})
        .is_err());
}
#[test]
fn source_identity_is_bound_and_never_automatically_replaces_another_folder() {
    let root = tempfile::tempdir().unwrap();
    let folder1 = root.path().join("one");
    let folder2 = root.path().join("two");
    fs::create_dir(&folder1).unwrap();
    fs::create_dir(&folder2).unwrap();
    let source1 = folder1.join("same.xlsx");
    let source2 = folder2.join("same.xlsx");
    fixture(&source1, 2);
    fixture(&source2, 2);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p1 = prepared(&area, &source1, "day", "2026-11-01");
    let p2 = prepared(&area, &source2, "day", "2026-11-01");
    assert_ne!(p1.source_folder(), p2.source_folder());
    assert_ne!(p1.operation_id(), p2.operation_id());
    let storage = Storage::new(root.path().join("live")).unwrap();
    original(&storage);
    let preview1 = storage.preview_import(&p1).unwrap();
    assert!(
        storage
            .apply_import(
                &p1,
                &preview1,
                ImportConfirmation {
                    replace_source: true,
                    ..Default::default()
                },
                &ImportJob::new(),
                |_| {}
            )
            .is_err(),
        "manual artifact cannot imply folder ownership"
    );
    assert!(storage
        .apply_import(
            &p2,
            &preview1,
            ImportConfirmation::default(),
            &ImportJob::new(),
            |_| {}
        )
        .is_err());
    storage
        .apply_import(
            &p1,
            &preview1,
            ImportConfirmation::default(),
            &ImportJob::new(),
            |_| {},
        )
        .unwrap();
    let before = storage.load().unwrap();
    let preview2 = storage.preview_import(&p2).unwrap();
    assert!(storage
        .apply_import(
            &p2,
            &preview2,
            ImportConfirmation::default(),
            &ImportJob::new(),
            |_| {}
        )
        .is_err());
    assert_eq!(storage.load().unwrap(), before);
    // Explicit manual exact-period replacement remains separate from folder ownership.
}
#[test]
fn preparation_cancellation_empty_data_and_budgets_leave_live_database_intact() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 1000);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let storage = Storage::new(root.path().join("live")).unwrap();
    let before = original(&storage);
    let options = LegacyOptions {
        sheet: "xl/worksheets/sheet1.xml".into(),
        kind: "day".into(),
        start: "2026-11-01".into(),
        organizations: ["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
        mappings: Some(vec![analiz_import_core::TripleMapping {
            header_row: 1,
            work_col: 1,
            count_col: 2,
            amount_col: 3,
        }]),
    };
    for fault in ["cancel", "empty", "limit", "invalid"] {
        cancel.store(false, std::sync::atomic::Ordering::Release);
        let mut stage = area
            .ingest(&source, Limits::default(), &cancel, |_| {})
            .unwrap();
        stage.convert_legacy(&options, &cancel, |_| {}).unwrap();
        if fault == "invalid" {
            Connection::open(stage.path().join("stage.sqlite3"))
                .unwrap()
                .execute("UPDATE converted_reports SET kind='bad'", [])
                .unwrap();
        }
        if fault == "empty" {
            Connection::open(stage.path().join("stage.sqlite3"))
                .unwrap()
                .execute("UPDATE converted_reports SET row_count=0", [])
                .unwrap();
        }
        if fault == "limit" {
            Connection::open(stage.path().join("stage.sqlite3"))
                .unwrap()
                .execute(
                    "UPDATE converted_rows SET work=?1 WHERE row_index=1",
                    ["x".repeat(65536)],
                )
                .unwrap();
        }
        assert!(
            stage
                .prepare(&cancel, |_| {
                    if fault == "cancel" {
                        cancel.store(true, std::sync::atomic::Ordering::Release);
                    }
                })
                .is_err(),
            "{fault}"
        );
        assert_eq!(storage.load().unwrap(), before);
        assert_eq!(fs::read_dir(root.path().join("stage")).unwrap().count(), 1);
    }
}
#[test]
fn complete_multi_report_preview_is_bound_to_target_database() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 2);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let mut stage = area
        .ingest(&source, Limits::default(), &cancel, |_| {})
        .unwrap();
    let mapping = analiz_import_core::TripleMapping {
        header_row: 1,
        work_col: 1,
        count_col: 2,
        amount_col: 3,
    };
    stage
        .convert_legacy(
            &LegacyOptions {
                sheet: "xl/worksheets/sheet1.xml".into(),
                kind: "day".into(),
                start: "2026-11-01".into(),
                organizations: ["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
                mappings: Some(vec![mapping.clone(), mapping.clone(), mapping]),
            },
            &cancel,
            |_| {},
        )
        .unwrap();
    let p = stage.prepare(&cancel, |_| {}).unwrap();
    assert_eq!(p.report_count(), 3);
    let storage = Storage::new(root.path().join("live")).unwrap();
    original(&storage);
    let preview = storage.preview_import(&p).unwrap();
    assert_eq!(preview.counts().new_reports, 3);
    let other = Storage::new(root.path().join("other")).unwrap();
    original(&other);
    assert_eq!(other.revision().unwrap(), preview.revision());
    assert!(other
        .apply_import(
            &p,
            &preview,
            ImportConfirmation::default(),
            &ImportJob::new(),
            |_| {}
        )
        .is_err());
    let receipt = storage
        .apply_import(
            &p,
            &preview,
            ImportConfirmation::default(),
            &ImportJob::new(),
            |_| {},
        )
        .unwrap();
    assert_eq!((receipt.reports, receipt.rows), (3, 6));
    assert_eq!(
        storage
            .report_page(receipt.revision, &ReadFilter::default(), None, 1)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        storage
            .aggregate(receipt.revision, &ReadFilter::default())
            .unwrap()
            .rows,
        7
    );
    let again = prepared(&area, &source, "day", "2026-11-02");
    assert_eq!(
        storage
            .preview_import(&again)
            .unwrap()
            .counts()
            .replacements,
        1
    );
}
#[test]
fn preparation_holds_one_private_conversion_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 1000);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let mut stage = area
        .ingest(&source, Limits::default(), &cancel, |_| {})
        .unwrap();
    stage
        .convert_legacy(
            &LegacyOptions {
                sheet: "xl/worksheets/sheet1.xml".into(),
                kind: "day".into(),
                start: "2026-11-01".into(),
                organizations: ["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
                mappings: Some(vec![analiz_import_core::TripleMapping {
                    header_row: 1,
                    work_col: 1,
                    count_col: 2,
                    amount_col: 3,
                }]),
            },
            &cancel,
            |_| {},
        )
        .unwrap();
    let path = stage.path().join("stage.sqlite3");
    let mut attempted = false;
    let p = stage
        .prepare(&cancel, |_| {
            if !attempted {
                attempted = true;
                let c = Connection::open(&path).unwrap();
                c.busy_timeout(std::time::Duration::ZERO).unwrap();
                assert!(c
                    .execute("UPDATE converted_rows SET amount=999", [])
                    .is_err());
            }
        })
        .unwrap();
    assert!(attempted);
    let storage = Storage::new(root.path().join("live")).unwrap();
    storage.revision().unwrap();
    let preview = storage.preview_import(&p).unwrap();
    let receipt = storage
        .apply_import(
            &p,
            &preview,
            ImportConfirmation::default(),
            &ImportJob::new(),
            |_| {},
        )
        .unwrap();
    assert_eq!(
        storage
            .aggregate(receipt.revision, &ReadFilter::default())
            .unwrap()
            .amount,
        2000.0
    );
}
#[test]
fn preview_requires_initialized_storage_without_migrating_legacy_data() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 2);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = prepared(&area, &source, "day", "2026-11-01");
    let live = root.path().join("live");
    let storage = Storage::new(live.clone()).unwrap();
    assert!(storage.preview_import(&p).is_err());
    assert!(!live.join("reports.sqlite3").exists());
    storage.revision().unwrap();
    let revision = storage.revision().unwrap();
    assert!(storage.preview_import(&p).is_ok());
    assert_eq!(storage.revision().unwrap(), revision);
}
#[test]
fn cleanup_failure_is_separate_from_committed_receipt() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 2);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = prepared(&area, &source, "day", "2026-11-01");
    let storage = Storage::new(root.path().join("live")).unwrap();
    original(&storage);
    let preview = storage.preview_import(&p).unwrap();
    let receipt = storage
        .apply_import(
            &p,
            &preview,
            ImportConfirmation::default(),
            &ImportJob::new(),
            |_| {},
        )
        .unwrap();
    let path = p.directory().to_path_buf();
    fs::rename(&path, path.with_extension("orphan")).unwrap();
    fs::write(&path, b"blocks cleanup").unwrap();
    assert!(p.discard().is_err());
    assert_eq!(
        storage.committed_import(&receipt.operation_id).unwrap(),
        Some(receipt)
    );
    assert_eq!(
        storage.load().unwrap()["reports"].as_array().unwrap().len(),
        2
    );
}
#[test]
fn documents_share_the_ingestion_stage_disk_budget() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 1000);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let options = LegacyOptions {
        sheet: "xl/worksheets/sheet1.xml".into(),
        kind: "day".into(),
        start: "2026-11-01".into(),
        organizations: ["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
        mappings: Some(vec![analiz_import_core::TripleMapping {
            header_row: 1,
            work_col: 1,
            count_col: 2,
            amount_col: 3,
        }]),
    };
    let mut baseline = area
        .ingest(&source, Limits::default(), &cancel, |_| {})
        .unwrap();
    baseline.convert_legacy(&options, &cancel, |_| {}).unwrap();
    let bytes = fs::metadata(baseline.path().join("stage.sqlite3"))
        .unwrap()
        .len();
    drop(baseline);
    let mut stage = area
        .ingest(
            &source,
            Limits {
                stage_bytes: bytes + 65536,
                ..Limits::default()
            },
            &cancel,
            |_| {},
        )
        .unwrap();
    stage.convert_legacy(&options, &cancel, |_| {}).unwrap();
    let error = stage.prepare(&cancel, |_| {}).err().unwrap().to_string();
    assert!(
        error.contains("byte limit") || error.contains("full"),
        "{error}"
    );
    assert_eq!(fs::read_dir(root.path().join("stage")).unwrap().count(), 1);
}
#[test]
fn prepared_documents_keep_structured_diagnostics_controls_and_bound_source_hash() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    let mut zip = ZipWriter::new(fs::File::create(&source).unwrap());
    zip.start_file("xl/worksheets/sheet1.xml", SimpleFileOptions::default())
        .unwrap();
    zip.write_all("<worksheet><sheetData><row r=\"2\"><c r=\"A2\" t=\"inlineStr\"><is><t>СМУ16</t></is></c><c r=\"B2\"><v>3</v></c><c r=\"C2\"><v>6</v></c></row><row r=\"3\"><c r=\"A3\" t=\"inlineStr\"><is><t>Работа</t></is></c><c r=\"B3\"><v>1</v></c><c r=\"C3\"><v>2</v></c></row><row r=\"4\"><c r=\"A4\" t=\"inlineStr\"><is><t>Ошибка</t></is></c><c r=\"B4\" t=\"inlineStr\"><is><t>bad</t></is></c><c r=\"C4\"><v>4</v></c></row></sheetData></worksheet>".as_bytes()).unwrap();
    zip.finish().unwrap();
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let mut stage = area
        .ingest(&source, Limits::default(), &cancel, |_| {})
        .unwrap();
    let source_hash = stage.source_hash.clone();
    stage.source_hash = "caller forged visible hash".into();
    stage
        .convert_legacy(
            &LegacyOptions {
                sheet: "xl/worksheets/sheet1.xml".into(),
                kind: "day".into(),
                start: "2026-11-01".into(),
                organizations: ["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
                mappings: Some(vec![analiz_import_core::TripleMapping {
                    header_row: 1,
                    work_col: 1,
                    count_col: 2,
                    amount_col: 3,
                }]),
            },
            &cancel,
            |_| {},
        )
        .unwrap();
    let info = stage.report_page(0, 1).unwrap().remove(0);
    let issues = serde_json::to_value(stage.report_issues(&info.id, 0, 100).unwrap()).unwrap();
    let totals = serde_json::to_value(stage.report_totals(&info.id, 0, 100).unwrap()).unwrap();
    let orgs = serde_json::to_value(stage.report_organizations(&info.id).unwrap()).unwrap();
    assert!(issues.as_array().unwrap().len() >= 2);
    let p = stage.prepare(&cancel, |_| {}).unwrap();
    let report = p.next_report(0).unwrap().unwrap();
    let doc: Value = serde_json::from_reader(p.open_document(&report).unwrap()).unwrap();
    assert_eq!(
        report.quality.issues,
        issues.as_array().unwrap().len() as u64
    );
    assert!(report.quality.invalid > 0);
    assert!(report.quality.mismatches > 0);
    assert_eq!(
        serde_json::to_value(p.issues(&report, 0, 1).unwrap())
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        serde_json::to_value(p.controls(&report, 0, 100).unwrap()).unwrap(),
        totals
    );
    assert_eq!(
        serde_json::to_value(p.organizations(&report).unwrap()).unwrap(),
        orgs
    );
    let invalid = doc["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["type"] == "invalid")
        .unwrap();
    assert_eq!(invalid["cells"], json!(["B4"]));
    assert_eq!(invalid["row"], 4);
    assert_eq!(invalid["expected"], Value::Null);
    assert_eq!(invalid["actual"], Value::Null);
    assert_eq!(invalid["count_cell"], "B4");
    let mismatch = doc["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["type"] == "mismatch")
        .unwrap();
    assert_eq!(mismatch["cells"], json!(["B2", "C2"]));
    assert_eq!(mismatch["expected"], json!({"count":3.0,"amount":6.0}));
    assert_eq!(mismatch["actual"], json!({"count":1.0,"amount":2.0}));
    assert_eq!(doc["issues"], issues);
    assert_eq!(doc["totals"], totals);
    assert_eq!(doc["organizationTotals"], orgs);
    assert_eq!(doc["sourceHash"], source_hash);
    assert_eq!(p.source_hash(), source_hash);
    let storage = Storage::new(root.path().join("live")).unwrap();
    original(&storage);
    let preview = storage.preview_import(&p).unwrap();
    assert!(
        storage
            .apply_import(
                &p,
                &preview,
                ImportConfirmation::default(),
                &ImportJob::new(),
                |_| {}
            )
            .is_err(),
        "quality notices require explicit acknowledgment"
    );
    assert_eq!(preview.quality().issues, report.quality.issues);
    storage
        .apply_import(
            &p,
            &preview,
            ImportConfirmation {
                accept_quality: true,
                ..Default::default()
            },
            &ImportJob::new(),
            |_| {},
        )
        .unwrap();

    assert_eq!(doc["issueCount"], issues.as_array().unwrap().len());
    assert_eq!(
        doc["warnings"].as_array().unwrap().len(),
        issues.as_array().unwrap().len()
    );
}

fn folder_prepared<'a>(area: &'a StageArea, source: &Path, days: usize) -> PreparedImport<'a> {
    let cancel = AtomicBool::new(false);
    let mut stage = area
        .ingest(source, Limits::default(), &cancel, |_| {})
        .unwrap();
    let mapping = analiz_import_core::TripleMapping {
        header_row: 1,
        work_col: 1,
        count_col: 2,
        amount_col: 3,
    };
    stage
        .convert_legacy(
            &LegacyOptions {
                sheet: "xl/worksheets/sheet1.xml".into(),
                kind: "day".into(),
                start: "2026-11-01".into(),
                organizations: ["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
                mappings: Some(vec![mapping; days]),
            },
            &cancel,
            |_| {},
        )
        .unwrap();
    stage
        .prepare_with_mode(analiz_import_core::ImportMode::Folder, &cancel, |_| {})
        .unwrap()
}
#[test]
fn folder_shrink_replaces_complete_source_only_and_receipt_counts_removals() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 2);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let storage = Storage::new(root.path().join("live")).unwrap();
    let mut data = original(&storage);
    data["reports"].as_array_mut().unwrap().push(json!({"id":"different-folder","type":"day","start":"2026-11-04","end":"2026-11-04","desktopSource":"data.xlsx","desktopFolder":"/different","rows":[]}));
    storage.save(data).unwrap();
    let p = folder_prepared(&area, &source, 3);
    let preview = storage.preview_import(&p).unwrap();
    storage
        .apply_import(
            &p,
            &preview,
            ImportConfirmation::default(),
            &ImportJob::new(),
            |_| {},
        )
        .unwrap();
    let p = folder_prepared(&area, &source, 2);
    let preview = storage.preview_import(&p).unwrap();
    assert_eq!(preview.counts().replacements, 3);
    assert_eq!(preview.counts().new_reports, 0);
    assert!(storage
        .apply_import(
            &p,
            &preview,
            ImportConfirmation::default(),
            &ImportJob::new(),
            |_| {}
        )
        .is_err());
    let receipt = storage
        .apply_import(
            &p,
            &preview,
            ImportConfirmation {
                replace_source: true,
                ..Default::default()
            },
            &ImportJob::new(),
            |_| {},
        )
        .unwrap();
    assert_eq!(receipt.replaced, 3);
    assert_eq!(
        storage
            .apply_import(
                &p,
                &preview,
                ImportConfirmation::default(),
                &ImportJob::new(),
                |_| {}
            )
            .unwrap(),
        receipt
    );
    let loaded = storage.load().unwrap();
    assert_eq!(loaded["reports"].as_array().unwrap().len(), 4);
    assert!(loaded["reports"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["id"] == "different-folder"));
    assert!(!loaded["reports"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["start"] == "2026-11-03"));
    assert_eq!(
        storage
            .aggregate(receipt.revision, &ReadFilter::default())
            .unwrap()
            .rows,
        5
    );
    storage
        .backup_sqlite(&root.path().join("backup.sqlite3"))
        .unwrap();
    let conn = Connection::open(root.path().join("backup.sqlite3")).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM report_row_index", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        4
    );
}
#[test]
fn supported_generation_rotation_invalidates_same_revision_preview() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 2);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = prepared(&area, &source, "day", "2026-11-01");
    let storage = Storage::new(root.path().join("live")).unwrap();
    original(&storage);
    let preview = storage.preview_import(&p).unwrap();
    storage.invalidate_import_previews_after_restore().unwrap();
    assert_eq!(storage.revision().unwrap(), preview.revision());
    assert!(storage
        .apply_import(
            &p,
            &preview,
            ImportConfirmation::default(),
            &ImportJob::new(),
            |_| {}
        )
        .unwrap_err()
        .contains("generation"));
}

#[test]
fn preview_and_apply_cancel_while_waiting_for_storage_gate_or_writer() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 2);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = prepared(&area, &source, "day", "2026-11-01");
    let storage = Storage::new(root.path().join("live")).unwrap();
    original(&storage);
    let preview = storage.preview_import(&p).unwrap();
    let cancel = AtomicBool::new(false);
    let guard = storage.gate.lock().unwrap();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            cancel.store(true, std::sync::atomic::Ordering::Release);
        });
        let start = std::time::Instant::now();
        assert!(storage.preview_import_with_cancel(&p, &cancel).is_err());
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    });
    drop(guard);
    let conn = storage.connect().unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    let job = ImportJob::new();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            job.cancel();
        });
        let start = std::time::Instant::now();
        assert!(storage
            .apply_import(&p, &preview, ImportConfirmation::default(), &job, |_| {})
            .is_err());
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
        eprintln!(
            "c2 writer-wait cancellation observed in {} ms",
            start.elapsed().as_millis()
        );
    });
    conn.execute_batch("ROLLBACK").unwrap();
}

fn quality_fixture(path: &Path, invalid: usize) {
    let mut zip = ZipWriter::new(fs::File::create(path).unwrap());
    zip.start_file("xl/worksheets/sheet1.xml", SimpleFileOptions::default())
        .unwrap();
    write!(zip,"<worksheet><sheetData><row r=\"2\"><c r=\"A2\" t=\"inlineStr\"><is><t>СМУ16</t></is></c><c r=\"B2\"><v>1</v></c><c r=\"C2\"><v>2</v></c></row><row r=\"3\"><c r=\"A3\" t=\"inlineStr\"><is><t>Работа</t></is></c><c r=\"B3\"><v>1</v></c><c r=\"C3\"><v>2</v></c></row>").unwrap();
    for n in 4..invalid + 4 {
        write!(zip,"<row r=\"{n}\"><c r=\"A{n}\" t=\"inlineStr\"><is><t>Ошибка</t></is></c><c r=\"B{n}\" t=\"inlineStr\"><is><t>bad</t></is></c><c r=\"C{n}\"><v>2</v></c></row>").unwrap();
    }
    write!(zip,"<row r=\"{}\"><c r=\"A{}\" t=\"inlineStr\"><is><t>Итого</t></is></c></row></sheetData></worksheet>",invalid+4,invalid+4).unwrap();
    zip.finish().unwrap();
}
#[test]
fn complete_quality_counts_are_independent_of_pages_and_controls_preserve_nulls() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    quality_fixture(&source, 125);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = prepared(&area, &source, "day", "2026-11-01");
    let reports = p.reports(0, 1).unwrap();
    let report = &reports[0];
    let first = p.issues(report, 0, 1).unwrap();
    assert_eq!(first.len(), 1);
    assert!(report.quality.issues > 100);
    let mut seen = 0;
    let mut cursor = 0;
    loop {
        let page = p.issues(report, cursor, 100).unwrap();
        if page.is_empty() {
            break;
        }
        cursor = page.last().unwrap().sequence;
        seen += page.len() as u64;
    }
    assert_eq!(seen, report.quality.issues);
    assert_eq!(seen, p.quality().issues);
    let controls = p.controls(report, 0, 100).unwrap();
    assert!(controls.last().unwrap().count.is_none());
    assert!(controls.last().unwrap().amount.is_none());
    let serialized = serde_json::to_value(&first[0]).unwrap();
    assert_eq!(serialized["type"], "invalid");
    assert_eq!(serialized["row"], 4);
    assert_eq!(serialized["cells"], json!(["B4"]));
    assert_eq!(serialized["expected"], Value::Null);
    assert_eq!(serialized["actual"], Value::Null);
    assert!(p.issues(report, 0, 101).is_err());
    let conn = Connection::open(p.directory().join("stage.sqlite3")).unwrap();
    conn.execute("UPDATE converted_issues SET reason='damaged'", [])
        .unwrap();
    assert!(p.issues(report, 0, 1).is_err());
}
#[test]
fn folder_shrink_failures_leave_all_old_days_and_settings_unchanged() {
    for fault in ["quality", "read", "write", "cancel", "source", "revision"] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("data.xlsx");
        fixture(&source, 2);
        let area = StageArea::open(root.path().join("stage")).unwrap();
        let storage = Storage::new(root.path().join("live")).unwrap();
        original(&storage);
        let p = folder_prepared(&area, &source, 3);
        let preview = storage.preview_import(&p).unwrap();
        storage
            .apply_import(
                &p,
                &preview,
                ImportConfirmation::default(),
                &ImportJob::new(),
                |_| {},
            )
            .unwrap();
        if fault == "quality" {
            quality_fixture(&source, 1);
        }
        let p = folder_prepared(&area, &source, 2);
        let preview = storage.preview_import(&p).unwrap();
        if fault == "revision" {
            storage.save(storage.load().unwrap()).unwrap();
        }
        let before = storage.load().unwrap();
        let revision = storage.revision().unwrap();
        if fault == "read" {
            fs::remove_file(p.directory().join("report-2.json")).unwrap();
        }
        if fault == "source" {
            fs::write(&source, b"changed").unwrap();
        }
        if fault == "write" {
            storage.connect().unwrap().execute_batch("CREATE TRIGGER fail_c2 BEFORE INSERT ON report_rows BEGIN SELECT RAISE(ABORT,'write failure'); END").unwrap();
        }
        let job = ImportJob::new();
        assert!(
            storage
                .apply_import(
                    &p,
                    &preview,
                    ImportConfirmation {
                        replace_source: true,
                        ..Default::default()
                    },
                    &job,
                    |progress| {
                        if fault == "cancel" && progress.phase == "copy" {
                            job.cancel();
                        }
                    }
                )
                .is_err(),
            "{fault}"
        );
        assert_eq!(storage.load().unwrap(), before, "{fault}");
        assert_eq!(storage.revision().unwrap(), revision, "{fault}");
        assert!(storage
            .committed_import(p.operation_id())
            .unwrap()
            .is_none());
    }
}
#[test]
fn legacy_folder_ownership_is_explicit_and_unknown_identity_blocks() {
    for classification in [
        "legacy",
        "unknown",
        "manual-c1",
        "other-folder",
        "malformed",
    ] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("data.xlsx");
        fixture(&source, 2);
        let area = StageArea::open(root.path().join("stage")).unwrap();
        let p = folder_prepared(&area, &source, 2);
        let storage = Storage::new(root.path().join("live")).unwrap();
        let mut data = original(&storage);
        let mut old = json!({"id":"owned","type":"day","start":"2026-11-03","end":"2026-11-03","desktopSource":"data.xlsx","desktopFolder":p.source_folder(),"rows":[]});
        match classification {
            "unknown" => {
                old.as_object_mut().unwrap().remove("desktopFolder");
            }
            "manual-c1" => old["sourceHash"] = "c1-hash".into(),
            "other-folder" => old["desktopFolder"] = "/different".into(),
            "malformed" => old["desktopFolder"] = json!(["wrong"]),
            _ => {}
        }
        data["reports"].as_array_mut().unwrap().push(old);
        storage.save(data.clone()).unwrap();
        let preview = storage.preview_import(&p);
        if classification == "unknown" || classification == "malformed" {
            assert!(preview.is_err(), "{classification}");
            continue;
        }
        let preview = preview.unwrap();
        assert_eq!(
            preview.counts().replacements,
            if classification == "legacy" { 1 } else { 0 },
            "{classification}"
        );
        assert_eq!(storage.load().unwrap(), data);
        storage
            .apply_import(
                &p,
                &preview,
                ImportConfirmation {
                    replace_source: true,
                    ..Default::default()
                },
                &ImportJob::new(),
                |_| {},
            )
            .unwrap();
        let conn = storage.connect().unwrap();
        let old: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM reports WHERE id='owned')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old, classification != "legacy");
    }
}

#[test]
fn large_legacy_source_metadata_streams_and_cancels_without_loading_json() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 2);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = folder_prepared(&area, &source, 2);
    let storage = Storage::new(root.path().join("live")).unwrap();
    original(&storage);
    let conn = storage.connect().unwrap();
    let prefix = b"{\"unrelatedMetadata\":[";
    let tail = format!(
        "0],\"desktopSource\":\"data.xlsx\",\"desktopFolder\":{},\"rows\":[]}}",
        serde_json::to_string(p.source_folder()).unwrap()
    );
    let block = b"0,".repeat(32768);
    let blocks = 512;
    let length = prefix.len() + block.len() * blocks + tail.len();
    conn.execute("INSERT INTO reports VALUES('large','day','2026-11-03','2026-11-03','data.xlsx',zeroblob(?1))",[length]).unwrap();
    let rowid = conn.last_insert_rowid();
    {
        let mut blob = conn
            .blob_open("main", "reports", "json", rowid, false)
            .unwrap();
        blob.write_all(prefix).unwrap();
        for _ in 0..blocks {
            blob.write_all(&block).unwrap();
        }
        blob.write_all(tail.as_bytes()).unwrap();
    }
    let memory = || {
        fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .filter(|l| l.starts_with("VmRSS:") || l.starts_with("VmHWM:"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    eprintln!("c2 legacy metadata bytes={length} before {}", memory());
    let started = std::time::Instant::now();
    let preview = storage.preview_import(&p).unwrap();
    eprintln!(
        "c2 legacy metadata preview_ms={} after {}",
        started.elapsed().as_millis(),
        memory()
    );
    assert_eq!(preview.counts().replacements, 1);
    let cancel = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(30));
            cancel.store(true, std::sync::atomic::Ordering::Release);
        });
        let started = std::time::Instant::now();
        assert!(storage.preview_import_with_cancel(&p, &cancel).is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        eprintln!(
            "c2 legacy metadata cancellation_ms={}",
            started.elapsed().as_millis()
        );
    });
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM report_source_ownership", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0,
        "preview cannot persist ownership"
    );
    conn.execute(
        "UPDATE reports SET json=?1 WHERE id='large'",
        [format!(
            "{{\"unknown\":\"{}\",\"desktopSource\":\"data.xlsx\",\"desktopFolder\":\"/x\"}}",
            "x".repeat(65537)
        )],
    )
    .unwrap();
    assert!(
        storage.preview_import(&p).is_err(),
        "ignored string tokens are bounded too"
    );
}
#[test]
fn folder_mode_never_adopts_another_source_overlapping_period() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 2);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = folder_prepared(&area, &source, 2);
    let storage = Storage::new(root.path().join("live")).unwrap();
    let mut data = original(&storage);
    data["reports"].as_array_mut().unwrap().push(json!({"id":"other","type":"day","start":"2026-11-01","end":"2026-11-01","desktopSource":"data.xlsx","desktopFolder":"/different","rows":[]}));
    storage.save(data.clone()).unwrap();
    assert!(storage
        .preview_import(&p)
        .unwrap_err()
        .contains("another source"));
    assert_eq!(storage.load().unwrap(), data);
}

#[test]
fn source_ownership_index_is_invalidated_by_replace_writers_but_not_settings_reads() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 2);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = folder_prepared(&area, &source, 3);
    let storage = Storage::new(root.path().join("live")).unwrap();
    original(&storage);
    let preview = storage.preview_import(&p).unwrap();
    storage
        .apply_import(
            &p,
            &preview,
            ImportConfirmation::default(),
            &ImportJob::new(),
            |_| {},
        )
        .unwrap();
    let conn = storage.connect().unwrap();
    storage.revision().unwrap();
    storage.load().unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM report_source_ownership", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        3
    );
    let id: String = conn
        .query_row(
            "SELECT id FROM reports WHERE period_start='2026-11-03'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let other=json!({"id":id,"type":"day","start":"2026-11-03","end":"2026-11-03","desktopSource":"data.xlsx","desktopFolder":"/different","rows":[]}).to_string();
    conn.execute(
        "INSERT OR REPLACE INTO reports VALUES(?1,'day','2026-11-03','2026-11-03','data.xlsx',?2)",
        params![id, other],
    )
    .unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM report_source_ownership", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        2
    );
    let smaller = folder_prepared(&area, &source, 2);
    let preview = storage.preview_import(&smaller).unwrap();
    assert_eq!(preview.counts().replacements, 2);
}

#[test]
fn cancellation_during_large_source_set_does_not_return_a_preview() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 2);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = folder_prepared(&area, &source, 2);
    let storage = Storage::new(root.path().join("live")).unwrap();
    original(&storage);
    let conn = storage.connect().unwrap();
    conn.execute("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<50000) INSERT INTO reports(id,report_type,period_start,period_end,source_name,json) SELECT 'history-'||x,'day','2020-01-01','2020-01-01','data.xlsx','{\"desktopSource\":\"data.xlsx\",\"desktopFolder\":\"/different\",\"rows\":[]}' FROM n",[]).unwrap();
    let revision = storage.revision().unwrap();
    let cancel = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(30));
            cancel.store(true, std::sync::atomic::Ordering::Release);
        });
        let start = std::time::Instant::now();
        assert!(storage.preview_import_with_cancel(&p, &cancel).is_err());
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
        eprintln!(
            "c2 50000-source-set cancellation_ms={}",
            start.elapsed().as_millis()
        );
    });
    assert_eq!(storage.revision().unwrap(), revision);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM reports", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        50001
    );
}

#[test]
fn complete_conflict_and_source_scans_include_minimum_sqlite_rowid() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 2);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = prepared(&area, &source, "day", "2026-11-01");
    let storage = Storage::new(root.path().join("live")).unwrap();
    original(&storage);
    storage.connect().unwrap().execute("INSERT INTO reports(rowid,id,report_type,period_start,period_end,source_name,json) VALUES(-9223372036854775808,'minimum','day','2026-11-01','2026-11-01','data.xlsx','{\"desktopSource\":\"data.xlsx\",\"desktopFolder\":\"/different\",\"rows\":[]}')",[]).unwrap();
    assert_eq!(storage.preview_import(&p).unwrap().counts().replacements, 1);
    let folder = folder_prepared(&area, &source, 2);
    assert!(storage
        .preview_import(&folder)
        .unwrap_err()
        .contains("another source"));
}
