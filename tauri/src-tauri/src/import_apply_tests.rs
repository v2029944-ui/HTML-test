use super::*;
use analiz_import_core::{LegacyOptions, Limits, PreparedImport, StageArea};
use std::{io::Write, sync::atomic::AtomicBool};
use zip::{write::SimpleFileOptions, ZipWriter};

#[test]
fn legacy_period_dates_preview_apply_preserve_dates_and_reject_changed_or_stale_review() {
    let root=tempfile::tempdir().unwrap();
    let source=root.path().join("dates.xlsx");
    fixture(&source,2);
    let area=StageArea::open(root.path().join("stage")).unwrap();
    let cancel=AtomicBool::new(false);
    let make=|date:&str| {
        let mut stage=area.ingest(&source,Limits::default(),&cancel,|_|{}).unwrap();
        stage.convert_legacy(&LegacyOptions {
            period_dates:Some(vec![date.into()]),
            sheet:"xl/worksheets/sheet1.xml".into(), kind:"day".into(),start:"2026-11-01".into(),
            organizations:["СМУ16".into(),"АВТО".into(),"ТРАК".into(),"ЦЕНТР".into()],
            mappings:Some(vec![analiz_import_core::TripleMapping{header_row:1,work_col:1,count_col:2,amount_col:3}]),
        },&cancel,|_|{}).unwrap();
        stage.prepare(&cancel,|_|{}).unwrap()
    };
    let first=make("2026-11-04");
    let second=make("2026-11-05");
    let storage=Storage::new(root.path().join("live")).unwrap();
    let before=original(&storage);
    let first_preview=storage.preview_import(&first).unwrap();
    let second_preview=storage.preview_import(&second).unwrap();
    assert_eq!(storage.load().unwrap(),before);
    assert!(storage.apply_import(&second,&first_preview,ImportConfirmation::default(),&ImportJob::new(),|_|{}).is_err());
    assert_eq!(storage.load().unwrap(),before);
    storage.apply_import(&first,&first_preview,ImportConfirmation::default(),&ImportJob::new(),|_|{}).unwrap();
    let after=storage.load().unwrap();
    assert_eq!(after["settings"],before["settings"]);
    assert_eq!(after["reports"][1]["start"],"2026-11-04");
    assert_eq!(after["reports"][1]["end"],"2026-11-04");
    assert_eq!(after["reports"][1]["rows"].as_array().unwrap().len(),2);
    assert!(storage.apply_import(&second,&second_preview,ImportConfirmation::default(),&ImportJob::new(),|_|{}).is_err());
    assert_eq!(storage.load().unwrap(),after);
    let connection=storage.connect().unwrap();
    assert_eq!(connection.query_row("PRAGMA integrity_check",[],|r|r.get::<_,String>(0)).unwrap(),"ok");
}

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
                period_dates: None,
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
fn different_folder(root: &Path) -> String {
    // Use a real absolute path on every platform, not Unix-only "/different".
    // The folder need not exist: the identity must simply differ from the source.
    root.join("different").to_string_lossy().into_owned()
}

pub(super) fn original(storage: &Storage) -> Value {
    let data = json!({"reports":[{"id":"old","type":"day","start":"2025-01-01","end":"2025-01-01","rows":[{"organization":"A","work":"old","count":3,"amount":4}]}],"settings":{"custom":"keep","orgNames":["A","B","C","D"],"groups":[],"catalog":[]}});
    storage.save(data.clone()).unwrap();
    data
}
#[test]
fn confirmed_reimport_recovers_pre_fix_count_without_inventing_an_amount() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("blank.xlsx");
    let mut zip = ZipWriter::new(fs::File::create(&source).unwrap());
    zip.start_file("xl/worksheets/sheet1.xml", SimpleFileOptions::default()).unwrap();
    zip.write_all(r#"<worksheet><sheetData>
      <row r="2"><c r="A2" t="inlineStr"><is><t>СМУ16</t></is></c><c r="B2"><v>9</v></c></row>
      <row r="3"><c r="A3" t="inlineStr"><is><t>Без суммы</t></is></c><c r="B3"><v>7</v></c></row>
      <row r="4"><c r="A4" t="inlineStr"><is><t>Нулевая сумма</t></is></c><c r="B4"><v>2</v></c><c r="C4"><v>0</v></c></row>
      </sheetData></worksheet>"#.as_bytes()).unwrap();
    zip.finish().unwrap();
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let input = prepared(&area, &source, "day", "2026-11-01");
    let store = Storage::new(root.path().join("live")).unwrap();
    let mut previous = original(&store);
    previous["reports"].as_array_mut().unwrap().push(json!({
        "id": "before-fix", "type": "day", "start": "2026-11-01", "end": "2026-11-01",
        "desktopSource": "blank.xlsx", "hasMismatch": true,
        "rows": [{"organization": "СМУ16", "work": "Нулевая сумма", "count": 2.0, "amount": 0.0}],
        "issues": [{"type": "invalid", "reason": "Пропущена строка: нет количества или суммы",
            "row": 3, "amount_cell": "C3", "count_cell": null, "skipped": true}]
    }));
    store.save(previous.clone()).unwrap();
    let filter = ReadFilter {
        start: Some("2026-11-01".into()),
        end: Some("2026-11-01".into()),
        ..Default::default()
    };
    let revision = store.revision().unwrap();
    store.index_report("before-fix", revision).unwrap();
    let before = store.aggregate(revision, &filter).unwrap();
    assert_eq!((before.rows, before.count, before.amount), (1, 2.0, Some(0.0)));
    let preview = store.preview_import(&input).unwrap();
    assert_eq!(
        (preview.counts().new_reports, preview.counts().replacements),
        (0, 1)
    );
    let refusal = store.apply_import(
        &input,
        &preview,
        ImportConfirmation { accept_quality: true, ..Default::default() },
        &ImportJob::new(),
        |_| {},
    ).unwrap_err();
    assert_eq!(refusal, "Exact-period replacement needs explicit confirmation.");
    assert_eq!(store.revision().unwrap(), revision);
    let old_backup = root.path().join("before-reimport.sqlite3");
    assert_eq!(store.backup_sqlite_verified(&old_backup).unwrap(), revision);
    assert_eq!(store.load().unwrap(), previous);
    let confirmation = ImportConfirmation {
        replace_exact: true,
        accept_quality: true,
        ..Default::default()
    };
    let receipt = store
        .apply_import(&input, &preview, confirmation, &ImportJob::new(), |_| {})
        .unwrap();
    assert_eq!((receipt.rows, receipt.replaced), (2, 1));
    let data = store.load().unwrap();
    assert_eq!(data["reports"].as_array().unwrap().len(), 2);
    assert_eq!(data["reports"][0], previous["reports"][0]);
    let report = &data["reports"][1];
    let id = report["id"].as_str().unwrap();
    assert_eq!(report["rows"][0]["count"], 7.0);
    assert!(report["rows"][0]["amount"].is_null());
    assert_eq!(report["rows"][1]["amount"], 0.0);

    let verify = |store: &Storage, revision| {
        let sum = store.aggregate(revision, &filter).unwrap();
        assert_eq!((sum.rows, sum.count, sum.amount, sum.weighted_average), (2, 9.0, None, None));
    };
    verify(&store, receipt.revision);
    store.analysis_index_report(id, receipt.revision).unwrap();
    store.index_report(id, receipt.revision).unwrap();
    verify(&store, receipt.revision);
    let copy = root.path().join("backup.sqlite3");
    store.backup_sqlite(&copy).unwrap();
    let db = Connection::open(copy).unwrap();
    let amounts: (Option<f64>, Option<f64>) = db.query_row(
        "SELECT MIN(amount), MAX(CASE WHEN count=7 THEN amount END) FROM report_rows WHERE report_id=?1",
        [id], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert_eq!(amounts, (Some(0.0), None));
    let old = Connection::open_with_flags(old_backup, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap();
    let integrity: String = old.query_row("PRAGMA integrity_check", [], |row| row.get(0)).unwrap();
    assert_eq!(integrity, "ok");
    let original_count: f64 = old.query_row(
        "SELECT SUM(count) FROM report_rows WHERE report_id='before-fix'",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(original_count, 2.0);
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
        period_dates: None,
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
    multi_period_fixture(&source, false);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let mut stage = area
        .ingest(&source, Limits::default(), &cancel, |_| {})
        .unwrap();
    stage
        .convert_legacy(
            &LegacyOptions {
                period_dates: None,
                sheet: "xl/worksheets/sheet1.xml".into(),
                kind: "day".into(),
                start: "2026-11-01".into(),
                organizations: ["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
                mappings: Some(separate_blocks(3)),
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
                period_dates: None,
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
        Some(2000.0)
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
    // Unix allows renaming with an open SQLite handle; Windows does not.
    #[cfg(not(windows))]
    {
        fs::rename(&path, path.with_extension("orphan")).unwrap();
        fs::write(&path, b"blocks cleanup").unwrap();
    }
    #[cfg(windows)]
    let blocker = {
        use std::os::windows::fs::OpenOptionsExt;
        let file = path.join("cleanup-blocker");
        fs::write(&file, b"locked").unwrap();
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&file)
            .unwrap()
    };
    assert!(p.discard().is_err());
    #[cfg(windows)]
    {
        drop(blocker);
        fs::remove_dir_all(&path).unwrap();
    }
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
        period_dates: None,
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
                period_dates: None,
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

fn separate_blocks(days: usize) -> Vec<analiz_import_core::TripleMapping> {
    (0..days as u32).map(|day| analiz_import_core::TripleMapping {
        header_row: 1, work_col: day*3+1, count_col: day*3+2, amount_col: day*3+3,
    }).collect()
}

fn multi_period_fixture(path: &Path, invalid: bool) {
    let mut zip = ZipWriter::new(fs::File::create(path).unwrap());
    zip.start_file("xl/worksheets/sheet1.xml", SimpleFileOptions::default()).unwrap();
    write!(zip, "<worksheet><sheetData>").unwrap();
    for (row, name, count, amount) in [(2,"СМУ16",2,4), (3,"Работа 3",1,2), (4,"Работа 4",1,2)] {
        write!(zip, r#"<row r="{row}">"#).unwrap();
        for (work, qty, sum) in [('A','B','C'), ('D','E','F'), ('G','H','I')] {
            write!(zip, r#"<c r="{work}{row}" t="inlineStr"><is><t>{name}</t></is></c>"#).unwrap();
            if invalid && row==4 && work=='A' {
                write!(zip, r#"<c r="{qty}{row}" t="inlineStr"><is><t>bad</t></is></c>"#).unwrap();
            } else {
                write!(zip, r#"<c r="{qty}{row}"><v>{count}</v></c>"#).unwrap();
            }
            write!(zip, r#"<c r="{sum}{row}"><v>{amount}</v></c>"#).unwrap();
        }
        write!(zip, "</row>").unwrap();
    }
    write!(zip, "</sheetData></worksheet>").unwrap();
    zip.finish().unwrap();
}

fn folder_prepared<'a>(area: &'a StageArea, source: &Path, days: usize) -> PreparedImport<'a> {
    let cancel = AtomicBool::new(false);
    let mut stage = area
        .ingest(source, Limits::default(), &cancel, |_| {})
        .unwrap();
    stage
        .convert_legacy(
            &LegacyOptions {
                period_dates: None,
                sheet: "xl/worksheets/sheet1.xml".into(),
                kind: "day".into(),
                start: "2026-11-01".into(),
                organizations: ["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
                mappings: Some(separate_blocks(days)),
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
    multi_period_fixture(&source, false);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let storage = Storage::new(root.path().join("live")).unwrap();
    let mut data = original(&storage);
    data["reports"].as_array_mut().unwrap().push(json!({"id":"different-folder","type":"day","start":"2026-11-04","end":"2026-11-04","desktopSource":"data.xlsx","desktopFolder":different_folder(root.path()),"rows":[]}));
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
        multi_period_fixture(&source, false);
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
            multi_period_fixture(&source, true);
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
        multi_period_fixture(&source, false);
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
            "other-folder" => old["desktopFolder"] = different_folder(root.path()).into(),
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
    multi_period_fixture(&source, false);
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
            .unwrap_or_default()
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
    multi_period_fixture(&source, false);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = folder_prepared(&area, &source, 2);
    let storage = Storage::new(root.path().join("live")).unwrap();
    let mut data = original(&storage);
    data["reports"].as_array_mut().unwrap().push(json!({"id":"other","type":"day","start":"2026-11-01","end":"2026-11-01","desktopSource":"data.xlsx","desktopFolder":different_folder(root.path()),"rows":[]}));
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
    multi_period_fixture(&source, false);
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
    let other=json!({"id":id,"type":"day","start":"2026-11-03","end":"2026-11-03","desktopSource":"data.xlsx","desktopFolder":different_folder(root.path()),"rows":[]}).to_string();
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
    multi_period_fixture(&source, false);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = folder_prepared(&area, &source, 2);
    let storage = Storage::new(root.path().join("live")).unwrap();
    original(&storage);
    let conn = storage.connect().unwrap();
    let other_document = json!({"desktopSource":"data.xlsx","desktopFolder":different_folder(root.path()),"rows":[]}).to_string();
    conn.execute("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<50000) INSERT INTO reports(id,report_type,period_start,period_end,source_name,json) SELECT 'history-'||x,'day','2020-01-01','2020-01-01','data.xlsx',?1 FROM n", [&other_document]).unwrap();
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
    multi_period_fixture(&source, false);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = prepared(&area, &source, "day", "2026-11-01");
    let storage = Storage::new(root.path().join("live")).unwrap();
    original(&storage);
    let other_document = json!({"desktopSource":"data.xlsx","desktopFolder":different_folder(root.path()),"rows":[]}).to_string();
    storage.connect().unwrap().execute("INSERT INTO reports(rowid,id,report_type,period_start,period_end,source_name,json) VALUES(-9223372036854775808,'minimum','day','2026-11-01','2026-11-01','data.xlsx',?1)", [&other_document]).unwrap();
    assert_eq!(storage.preview_import(&p).unwrap().counts().replacements, 1);
    let folder = folder_prepared(&area, &source, 2);
    assert!(storage
        .preview_import(&folder)
        .unwrap_err()
        .contains("another source"));
}

#[test]
fn native_omitted_detail_evidence_survives_apply_without_changing_recognized_totals() {
    let root=tempfile::tempdir().unwrap();let source=root.path().join("evidence.xlsx");
    let rows=[vec!["Работа","Количество","Сумма"],vec!["СМУ16","3","8"],vec!["Included","1","5"],vec!["Rejected","bad","99"],vec!["","2","3"],vec!["АВТО","bad","7"],vec!["Unknown organization","4","9"],vec!["Итого","3","8"]];
    let mut xml=String::from("<worksheet><sheetData>");
    for (row,values) in rows.iter().enumerate(){xml.push_str(&format!("<row r=\"{}\">",row+1));for (col,value) in values.iter().enumerate(){if !value.is_empty(){xml.push_str(&format!("<c r=\"{}{}\" t=\"inlineStr\"><is><t>{value}</t></is></c>",(b'A'+col as u8) as char,row+1));}}xml.push_str("</row>");}xml.push_str("</sheetData></worksheet>");
    let mut archive=ZipWriter::new(fs::File::create(&source).unwrap());archive.start_file("xl/worksheets/sheet1.xml",SimpleFileOptions::default()).unwrap();archive.write_all(xml.as_bytes()).unwrap();archive.finish().unwrap();
    let original_bytes=fs::read(&source).unwrap();let area=StageArea::open(root.path().join("stage")).unwrap();let cancel=AtomicBool::new(false);
    let mut stage=area.ingest(&source,Limits::default(),&cancel,|_|{}).unwrap();stage.convert_legacy(&LegacyOptions{period_dates:None,sheet:"xl/worksheets/sheet1.xml".into(),kind:"day".into(),start:"2020-03-01".into(),organizations:["СМУ16".into(),"АВТО".into(),"ТРАК".into(),"ЦЕНТР".into()],mappings:None},&cancel,|_|{}).unwrap();
    let prepared=stage.prepare(&cancel,|_|{}).unwrap();let report=prepared.next_report(0).unwrap().unwrap();
    let store=Storage::new(root.path().join("live")).unwrap();let revision=store.revision().unwrap();let preview=store.preview_import(&prepared).unwrap();
    assert!(store.apply_import(&prepared,&preview,ImportConfirmation::default(),&ImportJob::new(),|_|{}).is_err());assert_eq!(store.revision().unwrap(),revision);
    let receipt=store.apply_import(&prepared,&preview,ImportConfirmation{accept_quality:true,..Default::default()},&ImportJob::new(),|_|{}).unwrap();assert_eq!(receipt.rows,2);
    let quality=serde_json::to_value(store.report_quality_page(receipt.revision,&report.id,QualityScope::Report,0,50).unwrap()).unwrap();assert_eq!(quality["evidence"]["omitted_row_entries"],2);assert_eq!(quality["evidence"]["explicit_zero"],Value::Null);
    let issues=quality["issues"].as_array().unwrap();let unknown=issues.iter().find(|i|i["kind"]=="skipped").unwrap();assert_eq!(unknown["skipped"],true);assert!(unknown["organization"].is_null());
    let invalid_control=issues.iter().find(|i|i["source_row"]==6).unwrap();assert!(invalid_control["skipped"].is_null());let included=issues.iter().find(|i|i["kind"]=="empty-name").unwrap();assert!(included["skipped"].is_null());
    store.analysis_index_report(&report.id,receipt.revision).unwrap();let matrix=store.quality_matrix_page(receipt.revision,"day","2020-03-01","2020-03-01",&["СМУ16".into()]).unwrap();let cell=&matrix.rows[0].cells[0];assert!(cell.available);assert_eq!(cell.count,Some(3.));assert_eq!(cell.amount,Some(8.));assert_eq!(cell.warned_reports,1);
    assert_eq!(fs::read(&source).unwrap(),original_bytes);assert_eq!(store.revision().unwrap(),receipt.revision);
}

#[test]
fn staged_quality_page_and_csv_keep_all_filtered_rows_without_importing_the_workbook() {
    let root=tempfile::tempdir().unwrap();
    let path=root.path().join("review.xlsx");
    let mut xml=String::from(r#"<worksheet><sheetData><row r="2"><c r="A2" t="inlineStr"><is><t>СМУ16</t></is></c><c r="B2"><v>1</v></c><c r="C2"><v>2</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>Работа</t></is></c><c r="B3"><v>1</v></c><c r="C3"><v>2</v></c></row>"#);
    for n in 4..68 {
        xml.push_str(&format!(r#"<row r="{n}"><c r="A{n}" t="inlineStr"><is><t>Работа {n}</t></is></c><c r="B{n}" t="inlineStr"><is><t>bad</t></is></c><c r="C{n}"><v>2</v></c></row>"#));
    }
    xml.push_str("</sheetData></worksheet>");
    let mut zip=ZipWriter::new(fs::File::create(&path).unwrap());
    zip.start_file("xl/worksheets/sheet1.xml",SimpleFileOptions::default()).unwrap();
    zip.write_all(xml.as_bytes()).unwrap();zip.finish().unwrap();
    let original=fs::read(&path).unwrap();
    let area=StageArea::open(root.path().join("stage")).unwrap();
    let stage=prepared(&area,&path,"day","2024-01-01");
    let store=Storage::new(root.path().join("db")).unwrap();
    let rev=store.revision().unwrap();
    let filter=UnifiedQualityFilter{kind:Some("invalid".into()),source:Some("review".into()),..Default::default()};
    let next=store.prepared_quality_page(rev,&stage,&filter,50).unwrap();
    assert_eq!(next.total,64);assert_eq!(next.issues.len(),14);
    let target=root.path().join("pending.csv");store.prepared_quality_csv(rev,&stage,&filter,&target).unwrap();
    let out=fs::read_to_string(&target).unwrap();assert_eq!(out.lines().count(),65);assert!(out.contains("B67"));
    let stale=root.path().join("stale.csv");
    assert!(store.prepared_quality_csv(rev+1,&stage,&filter,&stale).is_err());assert!(!stale.exists());
    let other=UnifiedQualityFilter{source:Some("different.xlsx".into()),..Default::default()};
    assert_eq!(store.prepared_quality_page(rev,&stage,&other,0).unwrap().total,0);
    assert_eq!(fs::read(&path).unwrap(),original);
    assert!(store.load().unwrap()["reports"].as_array().unwrap().is_empty());
}

#[test]
fn ordinary_header_conflict_blocks_folder_write_and_duplicate_columns_never_double_a_week() {
    use analiz_import_core::{ImportMode, TripleMapping};
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("отчёт.xlsx");
    let mut workbook = ZipWriter::new(fs::File::create(&source).unwrap());
    workbook.start_file("xl/worksheets/sheet1.xml", SimpleFileOptions::default()).unwrap();
    workbook.write_all(r#"<worksheet><sheetData>
      <row r="1"><c r="A1" t="inlineStr"><is><t>Начало периода: 09.11.2026</t></is></c></row>
      <row r="2"><c r="A2" t="inlineStr"><is><t>Работа</t></is></c><c r="B2" t="inlineStr"><is><t>Количество</t></is></c><c r="C2" t="inlineStr"><is><t>Сумма</t></is></c></row>
      <row r="3"><c r="A3" t="inlineStr"><is><t>A</t></is></c><c r="B3"><v>2</v></c><c r="C3"><v>125</v></c></row>
      <row r="4"><c r="A4" t="inlineStr"><is><t>Работа А</t></is></c><c r="B4"><v>2</v></c><c r="C4"><v>125</v></c></row>
      </sheetData></worksheet>"#.as_bytes()).unwrap();
    workbook.finish().unwrap();
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let cancel=AtomicBool::new(false);
    let mut cfg=LegacyOptions{period_dates:None,sheet:"xl/worksheets/sheet1.xml".into(),kind:"week".into(),
        start:"2026-10-19".into(),organizations:["A".into(),"B".into(),"C".into(),"D".into()],mappings:None};
    let storage=Storage::new(root.path().join("store")).unwrap();
    let before=original(&storage);
    let mut stage=area.ingest(&source,Limits::default(),&cancel,|_|{}).unwrap();
    let columns=TripleMapping{header_row:2,work_col:1,count_col:2,amount_col:3};
    cfg.mappings=Some(vec![columns.clone(),columns]);
    assert!(stage.convert_legacy(&cfg,&cancel,|_|{}).unwrap_err().to_string().contains("нескольких наборах"));
    cfg.mappings=None;
    let summary=stage.convert_legacy(&cfg,&cancel,|_|{}).unwrap();
    assert_eq!((summary.reports,summary.rows,summary.issues),(1,1,1));
    let p=stage.prepare_with_mode(ImportMode::Folder,&cancel,|_|{}).unwrap();
    let preview=storage.preview_import(&p).unwrap();
    assert!(storage.apply_import(&p,&preview,ImportConfirmation::default(),&ImportJob::new(),|_|{}).is_err());
    assert_eq!(storage.load().unwrap(),before);
    for (date,accept) in [("2026-10-19",true),("2026-11-09",false)] {
        cfg.start=date.into();
        let mut stage=area.ingest(&source,Limits::default(),&cancel,|_|{}).unwrap();
        let summary=stage.convert_legacy(&cfg,&cancel,|_|{}).unwrap();
        assert_eq!(summary.issues,if accept {1} else {0});
        let p=stage.prepare(&cancel,|_|{}).unwrap();
        let preview=storage.preview_import(&p).unwrap();
        let flags=ImportConfirmation{accept_quality:accept,..Default::default()};
        let receipt=storage.apply_import(&p,&preview,flags,&ImportJob::new(),|_|{}).unwrap();
        assert_eq!((receipt.reports,receipt.rows),(1,1));
        let state=storage.load().unwrap();
        let report=state["reports"].as_array().unwrap().iter().find(|r|r["start"]==date).unwrap();
        assert_eq!(report["rows"][0]["amount"].as_f64(),Some(125.));
        if accept {assert!(report["issues"][0]["reason"].as_str().unwrap().contains("A1"));}
    }
}
