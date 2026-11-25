use super::*;
use analiz_import_core::{ImportMode, Limits, PreparedImport, Stage, StageArea, WideOptions};
use std::{
    io::{Read, Write},
    sync::{atomic::AtomicBool, Arc},
};
use zip::{write::SimpleFileOptions, ZipWriter};

// Real two-header, shared-work XLSX. Values vary independently of source controls.
fn fixture(path: &Path, days: usize, zero: bool, blank_last: bool, bad_source: bool) {
    fixture_at(
        path,
        days,
        zero,
        blank_last,
        bad_source,
        "2024-02-28",
        "Работа",
    )
}
fn fixture_at(
    path: &Path,
    days: usize,
    zero: bool,
    blank_last: bool,
    bad_source: bool,
    start: &str,
    title: &str,
) {
    let mut z = ZipWriter::new(fs::File::create(path).unwrap());
    let mut sheet = String::from(
        "<worksheet><sheetData><row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>Работа</t></is></c>",
    );
    fn cell(sheet: &mut String, col: usize, row: usize, text: &str, numeric: bool) {
        let mut n = col;
        let mut letters = String::new();
        while n > 0 {
            n -= 1;
            letters.insert(0, (b'A' + (n % 26) as u8) as char);
            n /= 26;
        }
        let address = format!("{letters}{row}");
        if numeric {
            sheet.push_str(&format!("<c r=\"{address}\"><v>{text}</v></c>"));
        } else {
            sheet.push_str(&format!(
                "<c r=\"{address}\" t=\"inlineStr\"><is><t>{text}</t></is></c>"
            ));
        }
    }
    sheet = sheet.replace("<t>Работа</t>", &format!("<t>{title}</t>"));
    let start: chrono::NaiveDate = start.parse().unwrap();
    for day in 0..days {
        let date = (start + chrono::Duration::days(day as i64)).to_string();
        cell(&mut sheet, day * 2 + 2, 1, &date, false);
        cell(&mut sheet, day * 2 + 3, 1, &date, false);
    }
    cell(&mut sheet, days * 2 + 2, 1, "Итого за период", false);
    cell(&mut sheet, days * 2 + 3, 1, "Итого за период", false);
    sheet.push_str("</row><row r=\"2\">");
    for day in 0..=days {
        cell(&mut sheet, day * 2 + 2, 2, "Количество", false);
        cell(&mut sheet, day * 2 + 3, 2, "Сумма", false);
    }
    sheet.push_str("</row>");
    for (row, label) in [(3, "СМУ16"), (4, "Work"), (5, "Итого")] {
        sheet.push_str(&format!("<row r=\"{row}\">"));
        cell(&mut sheet, 1, row, label, false);
        for day in 0..days {
            if blank_last && day == days - 1 || zero && row == 4 {
                continue;
            }
            cell(
                &mut sheet,
                day * 2 + 2,
                row,
                if zero { "0" } else { "1" },
                true,
            );
            cell(
                &mut sheet,
                day * 2 + 3,
                row,
                if zero { "0" } else { "2" },
                true,
            );
        }
        let available = days - usize::from(blank_last);
        let count = if zero { 0 } else { available };
        if bad_source && row == 4 {
            cell(&mut sheet, days * 2 + 2, row, "bad", false);
        } else {
            cell(
                &mut sheet,
                days * 2 + 2,
                row,
                &(count + usize::from(bad_source)).to_string(),
                true,
            );
        }
        cell(
            &mut sheet,
            days * 2 + 3,
            row,
            &(count * 2).to_string(),
            true,
        );
        sheet.push_str("</row>");
    }
    sheet.push_str("</sheetData></worksheet>");
    for (name,data) in [("xl/worksheets/sheet1.xml",sheet),("xl/workbook.xml","<workbook xmlns:r=\"r\"><sheets><sheet name=\"Data\" r:id=\"r1\"/></sheets></workbook>".into()),("xl/_rels/workbook.xml.rels","<Relationships><Relationship Id=\"r1\" Target=\"worksheets/sheet1.xml\" Type=\"worksheet\"/></Relationships>".into())] {
        z.start_file(name,SimpleFileOptions::default()).unwrap();z.write_all(data.as_bytes()).unwrap();
    }
    z.finish().unwrap();
}
fn converted<'a>(area: &'a StageArea, source: &Path, today: &str) -> Stage<'a> {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut stage = area
        .ingest(source, Limits::default(), &cancel, |_| {})
        .unwrap();
    stage
        .convert_wide_as_of(
            &WideOptions::default(),
            cancel,
            today.parse().unwrap(),
            |_| {},
        )
        .unwrap();
    stage
}
fn prepared<'a>(area: &'a StageArea, source: &Path, today: &str) -> PreparedImport<'a> {
    converted(area, source, today)
        .prepare_with_mode(ImportMode::Folder, &AtomicBool::new(false), |_| {})
        .unwrap()
}
#[test]
fn undated_quarter_prepare_apply_preserves_source_dates_values_and_idempotent_replay() {
    let root=tempfile::tempdir().unwrap();
    let source=root.path().join("undated.xlsx");
    fixture_at(&source,91,false,false,false,"2024-01-01","Работа Q1 2024");
    let mut archive=zip::ZipArchive::new(fs::File::open(&source).unwrap()).unwrap();
    let mut entries=Vec::new();
    for index in 0..archive.len(){
        let mut entry=archive.by_index(index).unwrap();
        let name=entry.name().to_owned();let mut data=String::new();entry.read_to_string(&mut data).unwrap();
        if name.starts_with("xl/worksheets/"){
            for day in 0..91{
                let date=chrono::NaiveDate::from_ymd_opt(2024,1,1).unwrap()+chrono::Duration::days(day);
                data=data.replace(&format!("<t>{date}</t>"),"<t></t>");
            }
        }
        entries.push((name,data));
    }
    drop(archive);
    let mut zip=ZipWriter::new(fs::File::create(&source).unwrap());
    for (name,data) in entries{zip.start_file(name,SimpleFileOptions::default()).unwrap();zip.write_all(data.as_bytes()).unwrap();}zip.finish().unwrap();
    let original_source=fs::read(&source).unwrap();
    let area=StageArea::open(root.path().join("stage")).unwrap();let cancel=Arc::new(AtomicBool::new(false));
    let mut stage=area.ingest(&source,Limits::default(),&cancel,|_|{}).unwrap();
    let mut options=WideOptions::default();options.year=Some(2024);
    options.sequential_periods.insert("Data".into(),analiz_import_core::WideSequentialPeriod{year:2024,quarter:Some(1)});
    stage.convert_wide_as_of(&options,cancel,"2024-12-31".parse().unwrap(),|_|{}).unwrap();
    let prepared=stage.prepare_with_mode(ImportMode::Folder,&AtomicBool::new(false),|_|{}).unwrap();
    assert_eq!(prepared.report_count(),91);assert_eq!(prepared.row_count(),91);
    let storage=Storage::new(root.path().join("live")).unwrap();storage.revision().unwrap();
    let before=storage.load().unwrap();let preview=storage.preview_import(&prepared).unwrap();
    assert_eq!(storage.load().unwrap(),before,"preview must not write live reports");
    let receipt=storage.apply_import(&prepared,&preview,ImportConfirmation::default(),&ImportJob::new(),|_|{}).unwrap();
    let loaded=storage.load().unwrap();let reports=loaded["reports"].as_array().unwrap();assert_eq!(reports.len(),91);
    let mut starts=reports.iter().map(|doc|doc["start"].as_str().unwrap()).collect::<Vec<_>>();starts.sort_unstable();
    assert_eq!(starts.first().copied(),Some("2024-01-01"));assert_eq!(starts.last().copied(),Some("2024-03-31"));
    for report in reports{assert_eq!(report["count"],1.0);assert_eq!(report["amount"],2.0);assert_eq!(report["sourceCoverage"],json!({"start":"2024-01-01","end":"2024-03-31"}));}
    assert_eq!(storage.apply_import(&prepared,&preview,ImportConfirmation::default(),&ImportJob::new(),|_|{}).unwrap(),receipt);
    assert_eq!(storage.load().unwrap(),loaded);assert_eq!(fs::read(&source).unwrap(),original_source);
}
#[test]
fn wide_zero_day_prepares_applies_and_backfills_without_fabricated_rows() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("wide.xlsx");
    fixture(&source, 2, true, true, true);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = prepared(&area, &source, "2024-03-01");
    assert_eq!(p.report_count(), 1);
    assert_eq!(p.row_count(), 0);
    let report = p.next_report(0).unwrap().unwrap();
    let doc: Value = serde_json::from_reader(p.open_document(&report).unwrap()).unwrap();
    assert_eq!(doc["sheetName"], "Data");
    assert_eq!(doc["sourceFormat"], "daily-wide");
    assert_eq!(doc["sourceBlock"], 1);
    assert_eq!(
        doc["sourceCoverage"],
        json!({"start":"2024-02-28","end":"2024-02-29"})
    );
    assert_eq!(doc["sourceControlDate"], "2024-02-28");
    assert_eq!(doc["explicitZero"], true);
    assert_eq!(doc["organizations"], json!(["СМУ16"]));
    assert_eq!(doc["rows"], json!([]));
    assert_eq!(doc["totals"][0]["type"], "org");
    assert_eq!(doc["totals"][0]["name"], "СМУ16");
    assert_eq!(doc["totals"][0]["row"], 3);
    let issues = doc["issues"].as_array().unwrap();
    let missing = issues.iter().find(|i| i["type"] == "missing-day").unwrap();
    assert_eq!(missing["scope"], "source-period");
    assert_eq!(missing["date"], "2024-02-29");
    assert_eq!(missing["cells"], json!(["D1"]));
    let storage = Storage::new(root.path().join("live")).unwrap();
    super::import_apply_tests::original(&storage);
    let preview = storage.preview_import(&p).unwrap();
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
                accept_quality: true,
                ..Default::default()
            },
            &ImportJob::new(),
            |_| {},
        )
        .unwrap();
    assert_eq!(receipt.rows, 0);
    let report_notes = serde_json::to_value(storage.report_quality_page(
        receipt.revision, &report.id, QualityScope::Report, 0, 50,
    ).unwrap()).unwrap();
    assert_eq!(report_notes["evidence"]["document_id"], report.id);
    assert_eq!(report_notes["evidence"]["explicit_zero"], true);
    assert_eq!(report_notes["evidence"]["sheet_name"], "Data");
    assert_eq!(report_notes["evidence"]["coverage"], json!({"start":"2024-02-28","end":"2024-02-29"}));
    let source_notes = serde_json::to_value(storage.report_quality_page(
        receipt.revision, &report.id, QualityScope::Source, 0, 50,
    ).unwrap()).unwrap();
    assert_eq!(source_notes["evidence"]["missing_day_entries"], 1);
    assert_eq!(source_notes["evidence"]["deferred_day_entries"], 0);
    assert_eq!(source_notes["evidence"]["omitted_row_entries"], 0);
    assert_eq!(storage.revision().unwrap(), receipt.revision);
    let loaded = storage.load().unwrap();
    assert_eq!(
        loaded["reports"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == report.id)
            .unwrap(),
        &doc
    );
    let conn = storage.connect().unwrap();
    conn.execute(
        "DELETE FROM report_row_index WHERE report_id=?1",
        [&report.id],
    )
    .unwrap();
    assert_eq!(
        storage.index_report(&report.id, receipt.revision).unwrap(),
        0
    );
    assert_eq!(
        storage
            .aggregate(receipt.revision, &ReadFilter::default())
            .unwrap()
            .rows,
        1
    );
}

#[test]
fn wide_prepared_source_diagnostics_preserve_future_relevance_without_duplicate_quality() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("wide.xlsx");
    fixture(&source, 2, false, true, false);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = prepared(&area, &source, "2024-02-28");
    let summary = p.source_diagnostics();
    assert_eq!(summary.issues, 1);
    assert_eq!(summary.relevant, 0);
    assert_eq!(summary.deferred, 1);
    assert_eq!(summary.sha256.len(), 64);
    assert_eq!(p.quality().issues, 0);
    assert_eq!(p.quality().source_issues, 1);
    assert_eq!(p.quality().relevant_source_issues, 0);
    let page = p.source_issues(0, 1).unwrap();
    assert_eq!(page.len(), 1);
    assert!(!page[0].relevant);
    assert_eq!(page[0].scope, "source-period");
    assert_eq!(page[0].date.as_deref(), Some("2024-02-29"));
    let report = p.next_report(0).unwrap().unwrap();
    assert_eq!(report.provenance.as_ref().unwrap().sheet_name, "Data");
    let doc: Value = serde_json::from_reader(p.open_document(&report).unwrap()).unwrap();
    assert_eq!(doc["sourceIssues"].as_array().unwrap().len(), 1);
    assert_eq!(doc["sourceIssues"][0]["relevant"], false);
    assert_eq!(doc["sourceDiagnostics"]["sha256"], summary.sha256);
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
    assert_eq!(receipt.rows, 1);
    assert_eq!(
        storage.load().unwrap()["reports"][0]["sourceIssues"],
        doc["sourceIssues"]
    );
}
#[test]
fn wide_source_attached_issues_keep_scope_date_link_and_nullable_comparisons() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("wide.xlsx");
    fixture(&source, 2, false, true, true);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = prepared(&area, &source, "2024-03-01");
    let report = p.next_report(0).unwrap().unwrap();
    let issues = p.issues(&report, 0, 100).unwrap();
    let missing = issues.iter().find(|i| i.kind == "missing-day").unwrap();
    assert_eq!(missing.scope.as_deref(), Some("source-period"));
    assert_eq!(missing.date.as_deref(), Some("2024-02-29"));
    assert!(missing.source_issue_sequence.is_some());
    assert_eq!(p.quality().issues, p.source_diagnostics().relevant);
    assert_eq!(p.source_issues(0, 1).unwrap().len(), 1);
    assert!(p.source_diagnostics().issues > 1);
    let invalid = issues
        .iter()
        .find(|i| i.kind == "invalid" && i.scope.as_deref() == Some("source-period"))
        .unwrap();
    let serialized = serde_json::to_value(invalid).unwrap();
    assert_eq!(serialized["expected"], json!({"count":null,"amount":2.0}));
    assert_eq!(serialized["actual"], json!({"count":1.0,"amount":2.0}));
    assert_eq!(serialized["cells"], json!(["F4", "G4"]));
    assert!(p.source_issues(0, 101).is_err());
}

#[test]
fn wide_complete_quarter_and_year_prepare_review_apply_and_decode() {
    for (days, title, end) in [
        (91, "Работа Q1 2024", "2024-03-31"),
        (366, "Работа за 2024 год", "2024-12-31"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("wide.xlsx");
        fixture_at(&source, days, false, false, false, "2024-01-01", title);
        let area = StageArea::open(root.path().join("stage")).unwrap();
        let p = prepared(&area, &source, "2024-12-31");
        assert_eq!(p.report_count(), days as u64);
        assert_eq!(p.row_count(), days as u64);
        assert_eq!(p.quality().issues, 0);
        assert_eq!(p.source_diagnostics().issues, 0);
        assert_eq!(p.reports(0, 1).unwrap().len(), 1);
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
        let loaded = storage.load().unwrap();
        assert_eq!(loaded["reports"].as_array().unwrap().len(), days);
        for (index, doc) in loaded["reports"].as_array().unwrap().iter().enumerate() {
            assert_eq!(doc["sheetName"], "Data");
            assert_eq!(doc["sourceFormat"], "daily-wide");
            assert_eq!(doc["sourceBlock"], index + 1);
            assert_eq!(
                doc["sourceCoverage"],
                json!({"start":"2024-01-01","end":end})
            );
            assert_eq!(doc["count"], 1.0);
            assert_eq!(doc["amount"], 2.0);
            assert_eq!(doc["rows"].as_array().unwrap().len(), 1);
        }
        assert_eq!(
            storage
                .aggregate(receipt.revision, &ReadFilter::default())
                .unwrap()
                .rows,
            days as i64
        );
        let first = p.next_report(0).unwrap().unwrap();
        let conn = storage.connect().unwrap();
        conn.execute(
            "DELETE FROM report_row_index WHERE report_id=?1",
            [&first.id],
        )
        .unwrap();
        conn.execute("DELETE FROM report_rows WHERE report_id=?1", [&first.id])
            .unwrap();
        assert_eq!(
            storage.index_report(&first.id, receipt.revision).unwrap(),
            1
        );
    }
}
#[test]
fn wide_complete_source_quality_exceeds_page_one_and_acknowledgment_uses_all_issues() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("wide.xlsx");
    fixture_at(
        &source,
        2,
        false,
        false,
        false,
        "2024-02-28",
        "Работа за 2024 год",
    );
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = prepared(&area, &source, "2024-12-31");
    assert_eq!(p.source_issues(0, 1).unwrap().len(), 1);
    assert_eq!(p.source_diagnostics().issues, 364);
    assert_eq!(p.source_diagnostics().relevant, 364);
    assert_eq!(p.quality().issues, 364);
    let first = p.next_report(0).unwrap().unwrap();
    let second = p.next_report(first.ordinal).unwrap().unwrap();
    assert_eq!(p.issues(&first, 0, 1).unwrap().len(), 1);
    assert!(p.issues(&second, 0, 100).unwrap().is_empty());
    let mut digest = sha2::Sha256::new();
    let mut cursor = 0;
    let mut count = 0;
    use sha2::Digest;
    loop {
        let page = p.source_issues(cursor, 100).unwrap();
        if page.is_empty() {
            break;
        }
        for issue in page {
            cursor = issue.sequence;
            count += 1;
            digest.update(serde_json::to_vec(&issue).unwrap());
            digest.update(b"\n");
        }
    }
    assert_eq!(count, 364);
    assert_eq!(
        format!("{:x}", digest.finalize()),
        p.source_diagnostics().sha256
    );
    let storage = Storage::new(root.path().join("live")).unwrap();
    storage.revision().unwrap();
    let preview = storage.preview_import(&p).unwrap();
    assert_eq!(preview.quality().issues, 364);
    assert!(storage
        .apply_import(
            &p,
            &preview,
            ImportConfirmation::default(),
            &ImportJob::new(),
            |_| {}
        )
        .is_err());
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
    let loaded = storage.load().unwrap();
    assert_eq!(
        loaded["reports"][0]["sourceIssues"]
            .as_array()
            .unwrap()
            .len(),
        364
    );
    assert_eq!(loaded["reports"][1]["sourceDiagnosticsReport"], first.id);
}
fn staging_database(root: &Path) -> PathBuf {
    fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.is_dir())
        .unwrap()
        .join("stage.sqlite3")
}
#[test]
fn wide_zero_proof_and_complete_metadata_fail_closed_before_publication() {
    for sql in [
        "UPDATE wide_provenance SET explicit_zero=0",
        "UPDATE converted_reports SET count=1",
        "UPDATE converted_totals SET amount=NULL WHERE kind='org'",
        "UPDATE converted_totals SET count=1 WHERE kind='org'",
        "DELETE FROM converted_organizations",
        "DELETE FROM wide_provenance",
        "UPDATE wide_provenance SET coverage_start='2024-03-01'",
        "DELETE FROM converted_issues WHERE scope='source-period'",
        "UPDATE converted_issues SET skipped=1 WHERE scope='source-period'",
        "UPDATE converted_issues SET source_issue_seq=(SELECT MIN(seq) FROM wide_source_issues WHERE kind='mismatch') WHERE scope='source-period' AND kind='mismatch'",
    ] {
        let root=tempfile::tempdir().unwrap();let source=root.path().join("wide.xlsx");fixture(&source,2,true,true,true);
        let area=StageArea::open(root.path().join("stage")).unwrap();let stage=converted(&area,&source,"2024-03-01");let conn=Connection::open(staging_database(&root.path().join("stage"))).unwrap();conn.execute(sql,[]).unwrap();drop(conn);
        assert!(stage.prepare(&AtomicBool::new(false),|_|{}).is_err(),"{sql}");
    }
}
#[test]
fn wide_folder_shrink_repeat_other_folder_and_failure_rollbacks_preserve_c2() {
    for fault in ["none", "cancel", "metadata", "write", "revision"] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("wide.xlsx");
        fixture(&source, 3, false, false, false);
        let area = StageArea::open(root.path().join("stage")).unwrap();
        let p = prepared(&area, &source, "2024-03-01");
        let storage = Storage::new(root.path().join("live")).unwrap();
        let mut data = super::import_apply_tests::original(&storage);
        data["reports"].as_array_mut().unwrap().push(json!({"id":"other-folder","type":"day","start":"2024-03-05","end":"2024-03-05","desktopSource":"wide.xlsx","desktopFolder":root.path().join("different").to_string_lossy().to_string(),"rows":[]}));
        storage.save(data).unwrap();
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
        fixture(&source, 2, false, false, false);
        let p = prepared(&area, &source, "2024-03-01");
        let preview = storage.preview_import(&p).unwrap();
        assert_eq!(preview.counts().replacements, 3);
        if fault == "write" {
            storage.connect().unwrap().execute_batch("CREATE TRIGGER fail_wide BEFORE INSERT ON report_rows BEGIN SELECT RAISE(ABORT,'test write failure'); END").unwrap();
        }
        if fault == "revision" {
            storage.connect().unwrap().execute(
                "UPDATE kv SET json=json WHERE name='settings'", [],
            ).unwrap();
        }
        let before = storage.load().unwrap();
        let job = ImportJob::new();
        let mut changed = false;
        let result = storage.apply_import(
            &p,
            &preview,
            ImportConfirmation {
                replace_source: true,
                ..Default::default()
            },
            &job,
            |event| {
                if event.phase == "copy" && !changed {
                    changed = true;
                    if fault == "cancel" {
                        job.cancel();
                    }
                    if fault == "metadata" {
                        Connection::open(p.directory().join("stage.sqlite3"))
                            .unwrap()
                            .execute("UPDATE wide_provenance SET sheet_name='tampered'", [])
                            .unwrap();
                    }
                }
            },
        );
        if fault != "none" {
            assert!(result.is_err(), "{fault}");
            assert_eq!(storage.load().unwrap(), before, "{fault}");
            continue;
        }
        let receipt = result.unwrap();
        assert_eq!(receipt.replaced, 3);
        assert_eq!(receipt.rows, 2);
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
        // The old anchor must be gone with its days; the new source has only
        // valid references, even when a shorter book removes a date.
        let con = storage.connect().unwrap();
        let orphaned: i64 = con.query_row(
            "SELECT COUNT(*) FROM report_source_diagnostic_links l LEFT JOIN reports r ON r.id=l.anchor_id WHERE r.id IS NULL",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(orphaned, 0);
        assert_eq!(loaded["reports"].as_array().unwrap().len(), 4);
        assert!(!loaded["reports"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["start"] == "2024-03-01"));
        assert!(loaded["reports"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "other-folder"));
    }
}

#[test]
fn wide_future_only_sheet_diagnostics_are_frozen_even_without_a_report_on_that_sheet() {
    use std::io::Read;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("wide.xlsx");
    let future = root.path().join("future.xlsx");
    fixture(&source, 2, false, false, false);
    fixture_at(&future, 2, true, false, false, "2024-03-01", "Работа");
    let sheet = |path: &Path| {
        let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut text = String::new();
        zip.by_name("xl/worksheets/sheet1.xml")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        text
    };
    let first = sheet(&source);
    let mut second = sheet(&future);
    for row in 3..=5 {
        for col in ["B", "C", "D", "E"] {
            second = second.replace(&format!("<c r=\"{col}{row}\"><v>0</v></c>"), "");
        }
    }
    let mut zip = ZipWriter::new(fs::File::create(&source).unwrap());
    for (name,data) in [("xl/worksheets/sheet1.xml",first),("xl/worksheets/sheet2.xml",second),("xl/workbook.xml","<workbook xmlns:r=\"r\"><sheets><sheet name=\"Data\" r:id=\"r1\"/><sheet name=\"Future\" r:id=\"r2\"/></sheets></workbook>".into()),("xl/_rels/workbook.xml.rels","<Relationships><Relationship Id=\"r1\" Target=\"worksheets/sheet1.xml\" Type=\"worksheet\"/><Relationship Id=\"r2\" Target=\"worksheets/sheet2.xml\" Type=\"worksheet\"/></Relationships>".into())] {zip.start_file(name,SimpleFileOptions::default()).unwrap();zip.write_all(data.as_bytes()).unwrap();}
    zip.finish().unwrap();
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = prepared(&area, &source, "2024-02-28");
    assert_eq!(p.report_count(), 2);
    assert_eq!(p.source_diagnostics().issues, 2);
    assert_eq!(p.source_diagnostics().relevant, 0);
    assert_eq!(p.quality().issues, 0);
    let issues = p.source_issues(0, 100).unwrap();
    assert!(issues
        .iter()
        .all(|i| i.sheet == "Future" && !i.relevant && i.report_id.is_none()));
    let storage = Storage::new(root.path().join("live")).unwrap();
    storage.revision().unwrap();
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
    let loaded = storage.load().unwrap();
    assert_eq!(
        loaded["reports"][0]["sourceIssues"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(loaded["reports"][0]["sourceIssues"][0]["sheet"], "Future");
}
#[test]
fn wide_frozen_source_pages_and_apply_reject_metadata_tampering() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("wide.xlsx");
    fixture(&source, 2, false, true, true);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let p = prepared(&area, &source, "2024-03-01");
    let storage = Storage::new(root.path().join("live")).unwrap();
    let before = super::import_apply_tests::original(&storage);
    let preview = storage.preview_import(&p).unwrap();
    Connection::open(p.directory().join("stage.sqlite3"))
        .unwrap()
        .execute("UPDATE wide_source_issues SET relevant=0", [])
        .unwrap();
    assert!(p.source_issues(0, 1).is_err());
    assert!(storage
        .apply_import(
            &p,
            &preview,
            ImportConfirmation {
                accept_quality: true,
                ..Default::default()
            },
            &ImportJob::new(),
            |_| {}
        )
        .is_err());
    assert_eq!(storage.load().unwrap(), before);
}

#[test]
fn wide_other_source_conflict_and_preparation_cancel_leave_live_reports_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("wide.xlsx");
    fixture_at(
        &source,
        2,
        false,
        false,
        false,
        "2024-02-28",
        "Работа за 2024 год",
    );
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let storage = Storage::new(root.path().join("live")).unwrap();
    let mut before = super::import_apply_tests::original(&storage);
    before["reports"].as_array_mut().unwrap().push(json!({"id":"foreign","type":"day","start":"2024-02-28","end":"2024-02-28","desktopSource":"wide.xlsx","desktopFolder":root.path().join("different").to_string_lossy().to_string(),"rows":[]}));
    storage.save(before.clone()).unwrap();
    let before = storage.load().unwrap();
    let cancel = AtomicBool::new(false);
    let stage = converted(&area, &source, "2024-12-31");
    assert!(stage
        .prepare(&cancel, |p| {
            if p.phase == "prepare" {
                cancel.store(true, std::sync::atomic::Ordering::Release);
            }
        })
        .is_err());
    assert_eq!(storage.load().unwrap(), before);
    let p = prepared(&area, &source, "2024-12-31");
    assert!(storage
        .preview_import(&p)
        .unwrap_err()
        .contains("another source"));
    assert_eq!(storage.load().unwrap(), before);
}

#[test]
fn wide_declared_quarter_preserves_valid_actual_days_outside_coverage_with_quality_ack() {
    for zero in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("wide.xlsx");
        fixture_at(
            &source,
            2,
            zero,
            false,
            false,
            "2024-04-01",
            "Работа Q1 2024",
        );
        let area = StageArea::open(root.path().join("stage")).unwrap();
        let p = prepared(&area, &source, "2024-12-31");
        assert_eq!(p.report_count(), 2);
        assert_eq!(p.source_diagnostics().relevant, 91);
        assert_eq!(p.quality().issues, 91);
        let first = p.next_report(0).unwrap().unwrap();
        assert_eq!(first.start, "2024-04-01");
        assert_eq!(first.is_explicit_zero(), zero);
        let provenance = first.provenance.as_ref().unwrap();
        assert_eq!(provenance.coverage.start, "2024-01-01");
        assert_eq!(provenance.coverage.end, "2024-03-31");
        assert_eq!(
            provenance.source_control_date.as_deref(),
            Some("2024-04-01")
        );
        let storage = Storage::new(root.path().join("live")).unwrap();
        storage.revision().unwrap();
        let preview = storage.preview_import(&p).unwrap();
        assert!(storage
            .apply_import(
                &p,
                &preview,
                ImportConfirmation::default(),
                &ImportJob::new(),
                |_| {}
            )
            .is_err());
        assert!(storage.load().unwrap()["reports"]
            .as_array()
            .unwrap()
            .is_empty());
        let receipt = storage
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
        assert_eq!(receipt.rows, if zero { 0 } else { 2 });
        let loaded = storage.load().unwrap();
        for (index, report) in loaded["reports"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                report["start"],
                if index == 0 {
                    "2024-04-01"
                } else {
                    "2024-04-02"
                }
            );
            assert_eq!(
                report["sourceCoverage"],
                json!({"start":"2024-01-01","end":"2024-03-31"})
            );
            assert_eq!(report["explicitZero"], zero);
        }
        assert_eq!(
            storage
                .aggregate(receipt.revision, &ReadFilter::default())
                .unwrap()
                .rows,
            if zero { 0 } else { 2 }
        );
    }
}

#[test]
fn wide_manual_exact_anchor_replacement_and_legacy_save_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("wide.xlsx");
    fixture(&source, 2, false, false, false);
    let area = StageArea::open(root.path().join("stage")).unwrap();
    let original_wide = prepared(&area, &source, "2024-03-01");
    assert_eq!(original_wide.report_count(), 2);
    let anchor = original_wide.next_report(0).unwrap().unwrap();
    let dependent = original_wide.next_report(anchor.ordinal).unwrap().unwrap();
    let storage = Storage::new(root.path().join("live")).unwrap();
    storage.revision().unwrap();
    let preview = storage.preview_import(&original_wide).unwrap();
    storage.apply_import(
        &original_wide, &preview,
        ImportConfirmation { accept_quality: true, ..Default::default() },
        &ImportJob::new(), |_| {},
    ).unwrap();
    let before = storage.load().unwrap();
    assert_eq!(before["reports"].as_array().unwrap().len(), 2);
    assert_eq!(before["reports"][1]["sourceDiagnosticsReport"], anchor.id);

    let manual_source = root.path().join("manual.xlsx");
    fixture_at(&manual_source, 1, false, false, false, "2024-02-28", "Работа");
    let manual_area = StageArea::open(root.path().join("manual-stage")).unwrap();
    let manual = converted(&manual_area, &manual_source, "2024-03-01")
        .prepare_with_mode(ImportMode::Manual, &AtomicBool::new(false), |_| {})
        .unwrap();
    let manual_preview = storage.preview_import(&manual).unwrap();
    assert_eq!(manual_preview.counts().replacements, 1);
    let failed = storage.apply_import(
        &manual, &manual_preview,
        ImportConfirmation { replace_exact: true, accept_quality: true, ..Default::default() },
        &ImportJob::new(), |_| {},
    ).unwrap_err();
    assert!(failed.contains("Shared source diagnostics"), "{failed}");
    assert_eq!(storage.load().unwrap(), before);

    let conn = storage.connect().unwrap();
    let protected: i64 = conn.query_row(
        "SELECT COUNT(*) FROM report_source_diagnostic_links WHERE anchor_id=?1",
        [&anchor.id], |r| r.get(0),
    ).unwrap();
    assert_eq!(protected, 2);
    let direct = conn.execute("DELETE FROM reports WHERE id=?1", [&anchor.id]).unwrap_err();
    assert!(direct.to_string().contains("Shared source diagnostics"));
    let legacy = storage.save(before.clone()).unwrap_err();
    assert!(legacy.contains("shared source diagnostics"), "{legacy}");
    assert_eq!(storage.load().unwrap(), before);
    assert!(storage.load().unwrap()["reports"].as_array().unwrap()
        .iter().any(|r| r["id"]==dependent.id));
}

#[test]
fn wide_manual_complete_source_exact_replacement_orders_old_deletions_and_rolls_back_faults() {
    for fault in ["none","cancel","write"] {
        let root=tempfile::tempdir().unwrap();
        let source=root.path().join("old-wide.xlsx");fixture(&source,2,false,false,false);
        let area=StageArea::open(root.path().join("old-stage")).unwrap();let old=prepared(&area,&source,"2024-03-01");
        let storage=Storage::new(root.path().join("live")).unwrap();let untouched=super::import_apply_tests::original(&storage);
        let preview=storage.preview_import(&old).unwrap();storage.apply_import(&old,&preview,ImportConfirmation::default(),&ImportJob::new(),|_|{}).unwrap();
        let before=storage.load().unwrap();let revision=storage.revision().unwrap();
        let manual_source=root.path().join("different-content-and-name.xlsx");fixture(&manual_source,2,true,false,false);
        let manual_area=StageArea::open(root.path().join("manual-stage")).unwrap();let manual=converted(&manual_area,&manual_source,"2024-03-01").prepare_with_mode(ImportMode::Manual,&AtomicBool::new(false),|_|{}).unwrap();
        let preview=storage.preview_import(&manual).unwrap();assert_eq!(preview.counts().replacements,2);
        if fault=="write" {storage.connect().unwrap().execute_batch("CREATE TRIGGER reject_replacement BEFORE INSERT ON reports BEGIN SELECT RAISE(ABORT,'replacement publication failure');END;").unwrap();}
        let job=ImportJob::new();let mut copied=false;
        let result=storage.apply_import(&manual,&preview,ImportConfirmation {replace_exact:true,accept_quality:true,..Default::default()},&job,|event|{if fault=="cancel"&&event.phase=="copy" {copied=true;job.cancel();}});
        if fault!="none" {
            let error=result.unwrap_err();assert!(if fault=="cancel"{copied&&error.contains("cancel")}else{error.contains("replacement publication failure")},"{fault}: {error}");
            assert_eq!(storage.load().unwrap(),before,"{fault}");assert_eq!(storage.revision().unwrap(),revision);assert!(storage.committed_import(manual.operation_id()).unwrap().is_none());
        } else {
            let receipt=result.unwrap();assert_eq!(receipt.replaced,2);assert_eq!(receipt.reports,2);assert_eq!(receipt.rows,0);
            let after=storage.load().unwrap();assert_eq!(after["settings"],before["settings"]);assert_eq!(after["reports"].as_array().unwrap().len(),3);
            assert!(after["reports"].as_array().unwrap().contains(&untouched["reports"][0]));
            for old_report in old.reports(0,2).unwrap() {assert!(!after["reports"].as_array().unwrap().iter().any(|r|r["id"]==old_report.id));}
            let conn=storage.connect().unwrap();let (links,orphans):(i64,i64)=conn.query_row("SELECT COUNT(*),SUM(CASE WHEN r.id IS NULL THEN 1 ELSE 0 END) FROM report_source_diagnostic_links l LEFT JOIN reports r ON r.id=l.anchor_id",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();assert_eq!((links,orphans),(2,0));
            assert_eq!(storage.apply_import(&manual,&preview,ImportConfirmation::default(),&ImportJob::new(),|_|{}).unwrap(),receipt);
        }
        assert!(source.exists());assert!(manual_source.exists());
    }
}

#[test]
fn wide_manual_partial_anchor_replacement_rolls_back_predeleted_nonanchor() {
    let root=tempfile::tempdir().unwrap();let source=root.path().join("wide.xlsx");fixture(&source,3,false,false,false);
    let area=StageArea::open(root.path().join("old-stage")).unwrap();let old=prepared(&area,&source,"2024-03-01");let storage=Storage::new(root.path().join("live")).unwrap();storage.revision().unwrap();
    let preview=storage.preview_import(&old).unwrap();storage.apply_import(&old,&preview,ImportConfirmation::default(),&ImportJob::new(),|_|{}).unwrap();let before=storage.load().unwrap();let revision=storage.revision().unwrap();
    let replacement=root.path().join("partial.xlsx");fixture(&replacement,2,true,false,false);let replacement_area=StageArea::open(root.path().join("partial-stage")).unwrap();let manual=converted(&replacement_area,&replacement,"2024-03-01").prepare_with_mode(ImportMode::Manual,&AtomicBool::new(false),|_|{}).unwrap();
    let preview=storage.preview_import(&manual).unwrap();assert_eq!(preview.counts().replacements,2);
    let error=storage.apply_import(&manual,&preview,ImportConfirmation {replace_exact:true,accept_quality:true,..Default::default()},&ImportJob::new(),|_|{}).unwrap_err();assert!(error.contains("Shared source diagnostics"),"{error}");
    assert_eq!(storage.load().unwrap(),before);assert_eq!(storage.revision().unwrap(),revision);assert!(storage.committed_import(manual.operation_id()).unwrap().is_none());
    assert_eq!(storage.connect().unwrap().query_row("SELECT COUNT(*) FROM report_source_diagnostic_links",[],|r|r.get::<_,i64>(0)).unwrap(),3);
}

#[test]
fn weekly_and_monthly_shared_pairs_persist_to_sqlite_after_review_only() {
    for mode in [ImportMode::Manual, ImportMode::Folder] {
        for (kind, end) in [("week", "2024-02-07"), ("month", "2024-02-29")] {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("period.xlsx");
            fixture_at(&source, 1, false, false, false, "2024-02-01", "Работа");
            let area = StageArea::open(root.path().join("stage")).unwrap();
            let cancel = Arc::new(AtomicBool::new(false));
            let mut stage = area.ingest(&source, Limits::default(), &cancel, |_| {}).unwrap();
            let options = WideOptions { kind: kind.into(), ..Default::default() };
            stage.convert_wide(&options, cancel.clone(), |_| {}).unwrap();
            let prepared = stage.prepare_with_mode(mode, &cancel, |_| {}).unwrap();
            let live = root.path().join("live");
            let store = Storage::new(live.clone()).unwrap();
            store.revision().unwrap();
            let before = store.load().unwrap();
            let review = store.preview_import(&prepared).unwrap();
            assert_eq!(store.load().unwrap(), before);
            let job = ImportJob::new();
            job.cancel();
            assert!(store.apply_import(&prepared, &review, ImportConfirmation::default(), &job, |_| {}).is_err());
            assert_eq!(store.load().unwrap(), before);
            let apply = || store.apply_import(&prepared, &review, ImportConfirmation::default(), &ImportJob::new(), |_| {}).unwrap();
            let receipt = apply();
            assert_eq!(receipt.rows, 1);
            assert_eq!(apply(), receipt);
            let reopened = Storage::new(live).unwrap();
            let saved = reopened.load().unwrap();
            let reports = saved["reports"].as_array().unwrap();
            assert_eq!(reports.len(), 1);
            let report = &reports[0];
            assert_eq!(report["type"], kind);
            assert_eq!(report["start"], "2024-02-01");
            assert_eq!(report["end"], end);
            assert_eq!(report["sourceFormat"], "period-pairs");
            assert_eq!(report["sourceCoverage"], json!({"start": "2024-02-01", "end": end}));
            assert_eq!(report["count"], 1.0);
            assert_eq!(report["amount"], 2.0);
            assert_eq!(reopened.aggregate(receipt.revision, &ReadFilter::default()).unwrap().rows, 1);
        }
    }
}
