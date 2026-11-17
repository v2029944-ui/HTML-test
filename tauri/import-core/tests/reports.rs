use analiz_import_core::{LegacyOptions, Limits, StageArea, TripleMapping};
use std::{fs, io::Write, sync::atomic::AtomicBool};
use zip::{write::SimpleFileOptions, ZipWriter};
fn fixture(path: &std::path::Path, invalid: bool) {
    let rows = [
        vec!["Работа", "Количество", "Сумма"],
        vec!["СМУ16", "3", "12.30"],
        vec!["Альфа", "1.2", "5.30"],
        vec!["Бета", if invalid { "oops" } else { "1.8" }, "7"],
        vec!["Итого", "3", "12.30"],
    ];
    let mut xml = String::from("<worksheet><sheetData>");
    for (r, row) in rows.iter().enumerate() {
        xml.push_str(&format!("<row r=\"{}\">", r + 1));
        for (c, v) in row.iter().enumerate() {
            xml.push_str(&format!(
                "<c r=\"{}{}\" t=\"inlineStr\"><is><t>{v}</t></is></c>",
                (b'A' + c as u8) as char,
                r + 1
            ));
        }
        xml.push_str("</row>");
    }
    xml.push_str("</sheetData></worksheet>");
    let mut zip = ZipWriter::new(fs::File::create(path).unwrap());
    zip.start_file("xl/worksheets/sheet1.xml", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(xml.as_bytes()).unwrap();
    zip.finish().unwrap();
}
fn options(kind: &str, start: &str) -> LegacyOptions {
    LegacyOptions {
        sheet: "xl/worksheets/sheet1.xml".into(),
        kind: kind.into(),
        start: start.into(),
        organizations: ["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
        mappings: None,
    }
}
#[test]
fn legacy_reports_keep_all_rows_and_complete_totals_beyond_page_one() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("data.xlsx");
    fixture(&source, false);
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let mut stage = area
        .ingest(&source, Limits::default(), &cancel, |_| {})
        .unwrap();
    let summary = stage
        .convert_legacy(&options("week", "2026-11-09"), &cancel, |_| {})
        .unwrap();
    assert_eq!(summary.reports, 1);
    assert_eq!(summary.rows, 2);
    assert_eq!(summary.issues, 0);
    let reports = stage.report_page(0, 1).unwrap();
    assert_eq!(reports[0].start, "2026-11-09");
    assert_eq!(reports[0].end, "2026-11-15");
    assert_eq!(reports[0].count, 3.0);
    assert_eq!(reports[0].amount, 12.3);
    let p = stage.report_rows(&reports[0].id, 0, 1).unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].work, "Альфа");
    assert_eq!(p[0].count_cell, "B3");
    assert_eq!(
        stage.report_rows(&reports[0].id, 1, 1).unwrap()[0].work,
        "Бета"
    );
}
#[test]
fn invalid_values_keep_source_coordinates_and_do_not_invent_zero() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("data.xlsx");
    fixture(&source, true);
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let mut stage = area
        .ingest(&source, Limits::default(), &cancel, |_| {})
        .unwrap();
    let s = stage
        .convert_legacy(&options("day", "2026-11-13"), &cancel, |_| {})
        .unwrap();
    assert_eq!(s.rows, 1);
    assert!(s.issues >= 2);
    let r = stage.report_page(0, 1).unwrap();
    let issues = stage.report_issues(&r[0].id, 0, 100).unwrap();
    assert!(issues.iter().any(|i| i.kind == "invalid"
        && i.source_row == Some(4)
        && i.count_cell.as_deref() == Some("B4")));
    assert_eq!(r[0].count, 1.2);
    assert!(r[0].has_mismatch);
}
#[test]
fn unknown_dates_and_cancellation_cannot_publish_partial_reports() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("data.xlsx");
    fixture(&source, false);
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let mut stage = area
        .ingest(&source, Limits::default(), &cancel, |_| {})
        .unwrap();
    assert!(stage
        .convert_legacy(&options("week", ""), &cancel, |_| {})
        .is_err());
    assert!(stage.report_page(0, 10).is_err());
    let mut cfg = options("month", "2024-02-01");
    cfg.mappings = Some(vec![TripleMapping {
        header_row: 1,
        work_col: 1,
        count_col: 2,
        amount_col: 3,
    }]);
    stage.convert_legacy(&cfg, &cancel, |_| {}).unwrap();
    assert_eq!(stage.report_page(0, 1).unwrap()[0].end, "2024-02-29");
    assert!(stage
        .convert_legacy(&cfg, &AtomicBool::new(true), |_| {})
        .is_err());
    assert_eq!(stage.report_page(0, 1).unwrap()[0].end, "2024-02-29");
}
#[test]
fn failed_or_cancelled_reconversion_preserves_last_complete_preview() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("data.xlsx");
    fixture(&source, false);
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let no = AtomicBool::new(false);
    let mut stage = area
        .ingest(&source, Limits::default(), &no, |_| {})
        .unwrap();
    stage
        .convert_legacy(&options("week", "2026-11-09"), &no, |_| {})
        .unwrap();
    let before = stage.report_page(0, 100).unwrap();
    let mut cfg = options("month", "2024-02-01");
    let mapping = TripleMapping {
        header_row: 1,
        work_col: 1,
        count_col: 2,
        amount_col: 3,
    };
    cfg.mappings = Some(vec![mapping.clone(), mapping]);
    let cancel = AtomicBool::new(false);
    assert!(stage
        .convert_legacy(&cfg, &cancel, |summary| {
            if summary.reports == 1 {
                cancel.store(true, std::sync::atomic::Ordering::Release);
            }
        })
        .is_err());
    let after = stage.report_page(0, 100).unwrap();
    assert_eq!(before.len(), after.len());
    assert_eq!(before[0].start, after[0].start);
    assert_eq!(stage.report_rows(&after[0].id, 0, 100).unwrap().len(), 2);
}
