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
    fixture_rows(path, &rows);
}
fn fixture_rows(path: &std::path::Path, rows: &[Vec<&str>]) {
    let mut xml = String::from("<worksheet><sheetData>");
    for (r, row) in rows.iter().enumerate() {
        xml.push_str(&format!("<row r=\"{}\">", r + 1));
        for (c, v) in row.iter().enumerate() {
            if v.is_empty() {
                continue;
            }
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
    let totals_before =
        serde_json::to_value(stage.report_totals(&before[0].id, 0, 100).unwrap()).unwrap();
    let orgs_before =
        serde_json::to_value(stage.report_organizations(&before[0].id).unwrap()).unwrap();
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
    assert_eq!(before[0].mapping.header_row, after[0].mapping.header_row);
    assert_eq!(
        totals_before,
        serde_json::to_value(stage.report_totals(&after[0].id, 0, 100).unwrap()).unwrap()
    );
    assert_eq!(
        orgs_before,
        serde_json::to_value(stage.report_organizations(&after[0].id).unwrap()).unwrap()
    );
    assert_eq!(stage.report_rows(&after[0].id, 0, 100).unwrap().len(), 2);
}

#[test]
fn total_labels_use_ecmascript_whitespace() {
    for (separator, expected_rows, expected_count) in [("\u{feff}", 1, 1.0), ("\u{0085}", 2, 2.0)] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("data.xlsx");
        let label = format!("Итого{separator}период");
        fixture_rows(
            &source,
            &[
                vec!["Работа", "Количество", "Сумма"],
                vec!["СМУ16", "1", "1"],
                vec!["Работа1", "1", "1"],
                vec![&label, "1", "1"],
            ],
        );
        let area = StageArea::open(temp.path().join("stage")).unwrap();
        let cancel = AtomicBool::new(false);
        let mut stage = area
            .ingest(&source, Limits::default(), &cancel, |_| {})
            .unwrap();
        stage
            .convert_legacy(&options("day", "2026-11-13"), &cancel, |_| {})
            .unwrap();
        let report = stage.report_page(0, 1).unwrap().remove(0);
        assert_eq!(report.row_count, expected_rows, "separator {separator:?}");
        assert_eq!(report.count, expected_count);
        assert_eq!(report.amount, expected_count);
        assert_eq!(report.has_mismatch, separator == "\u{0085}");
    }
}
#[test]
fn large_finite_rounded_totals_keep_matching_controls() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("data.xlsx");
    fixture_rows(
        &source,
        &[
            vec!["Работа", "Количество", "Сумма"],
            vec!["СМУ16", "4503599627.370497", "45035996273704.97"],
            vec!["Работа1", "4503599627.370497", "45035996273704.97"],
            vec!["Итого", "4503599627.370497", "45035996273704.97"],
        ],
    );
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let mut stage = area
        .ingest(&source, Limits::default(), &cancel, |_| {})
        .unwrap();
    stage
        .convert_legacy(&options("day", "2026-11-13"), &cancel, |_| {})
        .unwrap();
    let report = stage.report_page(0, 1).unwrap().remove(0);
    assert_eq!(report.count, 4503599627.370497);
    assert_eq!(report.amount, 45035996273704.97);
    assert!(!report.has_mismatch);
    assert!(stage.report_issues(&report.id, 0, 100).unwrap().is_empty());
}
#[test]
fn unnamed_invalid_cells_match_raw_presence_and_parsed_numbers() {
    for (raw_count, raw_amount, count_cell, amount_cell) in [
        ("oops", "", Some("B3"), None),
        ("", "oops", None, Some("C3")),
        ("oops", "bad", Some("B3"), Some("C3")),
        ("1", "", None, Some("C3")),
        ("", "1", Some("B3"), None),
        ("oops", "1", Some("B3"), None),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("data.xlsx");
        fixture_rows(
            &source,
            &[
                vec!["Работа", "Количество", "Сумма"],
                vec!["СМУ16", "0", "0"],
                vec!["", raw_count, raw_amount],
            ],
        );
        let area = StageArea::open(temp.path().join("stage")).unwrap();
        let cancel = AtomicBool::new(false);
        let mut stage = area
            .ingest(&source, Limits::default(), &cancel, |_| {})
            .unwrap();
        stage
            .convert_legacy(&options("day", "2026-11-13"), &cancel, |_| {})
            .unwrap();
        let issue = stage.report_issues("legacy-1", 0, 1).unwrap().remove(0);
        assert_eq!(issue.kind, "invalid");
        assert_eq!(issue.source_row, Some(3));
        assert_eq!(issue.organization.as_deref(), Some("СМУ16"));
        assert_eq!(issue.work_cell.as_deref(), Some("A3"));
        assert_eq!(issue.count_cell.as_deref(), count_cell);
        assert_eq!(issue.amount_cell.as_deref(), amount_cell);
    }
}
#[test]
fn scaled_total_overflow_rejects_without_publishing() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("data.xlsx");
    fixture_rows(
        &source,
        &[
            vec!["Работа", "Количество", "Сумма"],
            vec!["СМУ16", "1e308", "1e308"],
            vec!["Работа1", "1e308", "1e308"],
        ],
    );
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let mut stage = area
        .ingest(&source, Limits::default(), &cancel, |_| {})
        .unwrap();
    assert!(stage
        .convert_legacy(&options("day", "2026-11-13"), &cancel, |_| {})
        .is_err());
    assert!(stage.report_page(0, 1).is_err());
}
#[test]
fn control_apis_expose_mapping_exact_cells_and_first_seen_organizations() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("data.xlsx");
    fixture_rows(
        &source,
        &[
            vec!["", "", "", "Работа", "Количество", "", "", "Сумма"],
            vec!["", "", "", "ТРАК", "0", "", "", "0"],
            vec!["", "", "", "СМУ16", "1", "", "", "2"],
            vec!["", "", "", "Работа1", "1", "", "", "2"],
            vec!["", "", "", "Итого", "1", "", "", "2"],
        ],
    );
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let mut stage = area
        .ingest(&source, Limits::default(), &cancel, |_| {})
        .unwrap();
    assert!(stage.report_totals("legacy-1", 0, 100).is_err());
    assert!(stage.report_organizations("legacy-1").is_err());
    let mut cfg = options("day", "2026-11-13");
    cfg.mappings = Some(vec![
        TripleMapping {
            header_row: 1,
            work_col: 4,
            count_col: 5,
            amount_col: 8
        };
        2
    ]);
    stage.convert_legacy(&cfg, &cancel, |_| {}).unwrap();
    let report = stage.report_page(0, 1).unwrap().remove(0);
    assert_eq!(report.ordinal, 1);
    assert_eq!(report.sheet, cfg.sheet);
    assert_eq!(
        (
            report.mapping.header_row,
            report.mapping.work_col,
            report.mapping.count_col,
            report.mapping.amount_col
        ),
        (1, 4, 5, 8)
    );
    assert_eq!(stage.report_page(report.ordinal, 1).unwrap()[0].ordinal, 2);
    let totals = stage.report_totals(&report.id, 0, 2).unwrap();
    assert_eq!(totals.len(), 2);
    assert_eq!(totals[0].kind, "org");
    assert_eq!(totals[0].organization.as_deref(), Some("ТРАК"));
    assert_eq!((totals[0].count, totals[0].amount), (Some(0.0), Some(0.0)));
    assert_eq!(totals[0].source_row, 2);
    assert_eq!(
        (
            &*totals[0].work_cell,
            &*totals[0].count_cell,
            &*totals[0].amount_cell
        ),
        ("D2", "E2", "H2")
    );
    let last = stage
        .report_totals(&report.id, totals[1].sequence, 100)
        .unwrap();
    assert_eq!(last.len(), 1);
    assert_eq!(last[0].kind, "all");
    assert_eq!(last[0].organization, None);
    assert_eq!(
        (
            &*last[0].work_cell,
            &*last[0].count_cell,
            &*last[0].amount_cell
        ),
        ("D5", "E5", "H5")
    );
    assert_eq!((last[0].count, last[0].amount), (Some(1.0), Some(2.0)));
    assert!(stage
        .report_totals(&report.id, last[0].sequence, 1)
        .unwrap()
        .is_empty());
    let orgs = stage.report_organizations(&report.id).unwrap();
    assert_eq!(orgs.len(), 2);
    assert_eq!(orgs[0].organization, "ТРАК");
    assert_eq!((orgs[0].count, orgs[0].amount), (0.0, 0.0));
    assert_eq!(orgs[1].organization, "СМУ16");
    assert_eq!((orgs[1].count, orgs[1].amount), (1.0, 2.0));
    assert!(stage.report_totals(&report.id, 0, 0).is_err());
    assert!(stage.report_totals(&report.id, 0, 101).is_err());
}
#[test]
fn totals_api_pages_many_controls_and_preserves_nullable_totals() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("data.xlsx");
    let mut rows = vec![vec!["Работа", "Количество", "Сумма"]];
    for org in ["ТРАК", "СМУ16", "ЦЕНТР", "АВТО"] {
        for _ in 0..31 {
            rows.push(vec![org, "0", "0"]);
        }
    }
    rows.extend([vec!["Работа1", "1", "2"], vec!["Итого", "oops", "2"]]);
    fixture_rows(&source, &rows);
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let mut stage = area
        .ingest(&source, Limits::default(), &cancel, |_| {})
        .unwrap();
    stage
        .convert_legacy(&options("day", "2026-11-13"), &cancel, |_| {})
        .unwrap();
    let page = stage.report_totals("legacy-1", 0, 100).unwrap();
    assert_eq!(page.len(), 100);
    let rest = stage
        .report_totals("legacy-1", page[99].sequence, 100)
        .unwrap();
    assert_eq!(rest.len(), 25);
    assert_eq!(rest[24].sequence, 125);
    assert_eq!((rest[24].count, rest[24].amount), (None, Some(2.0)));
    let orgs = stage.report_organizations("legacy-1").unwrap();
    assert_eq!(
        orgs.iter()
            .map(|o| o.organization.as_str())
            .collect::<Vec<_>>(),
        ["ТРАК", "СМУ16", "ЦЕНТР", "АВТО"]
    );
}
