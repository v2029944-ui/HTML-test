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
        period_dates: None,
        sheet: "xl/worksheets/sheet1.xml".into(),
        kind: kind.into(),
        start: start.into(),
        organizations: ["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
        mappings: None,
    }
}

#[test]
fn legacy_period_dates_keep_independent_set_dates_for_day_week_and_month() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("dates.xlsx");
    fixture_rows(
        &source,
        &[
            vec![
                "Работа",
                "Количество",
                "Сумма",
                "Работа",
                "Количество",
                "Сумма",
            ],
            vec!["СМУ16", "1", "2", "СМУ16", "1", "2"],
            vec!["Альфа", "1", "2", "Бета", "1", "2"],
            vec!["Итого", "1", "2", "Итого", "1", "2"],
        ],
    );
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    for (kind, start, dates, ends) in [
        (
            "day",
            "2026-11-01",
            ["2026-11-11", "2026-11-04"],
            ["2026-11-11", "2026-11-04"],
        ),
        (
            "week",
            "2026-11-01",
            ["2026-11-11", "2026-12-01"],
            ["2026-11-17", "2026-12-07"],
        ),
        (
            "month",
            "2026-01-01",
            ["2026-01-01", "2026-03-01"],
            ["2026-01-31", "2026-03-31"],
        ),
    ] {
        let mut stage = area
            .ingest(&source, Limits::default(), &cancel, |_| {})
            .unwrap();
        let mut cfg = options(kind, start);
        cfg.period_dates = Some(dates.iter().map(|s| s.to_string()).collect());
        assert_eq!(
            stage.convert_legacy(&cfg, &cancel, |_| {}).unwrap().reports,
            2
        );
        let reports = stage.report_page(0, 100).unwrap();
        assert_eq!(
            reports.iter().map(|r| r.start.as_str()).collect::<Vec<_>>(),
            dates
        );
        assert_eq!(
            reports.iter().map(|r| r.end.as_str()).collect::<Vec<_>>(),
            ends
        );
        assert_eq!(
            stage.report_rows(&reports[0].id, 0, 100).unwrap()[0].work,
            "Альфа"
        );
        assert_eq!(
            stage.report_rows(&reports[1].id, 0, 100).unwrap()[0].work,
            "Бета"
        );
    }
}

#[test]
fn legacy_period_dates_reject_partial_invalid_overlap_and_overflow_without_losing_conversion() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("dates.xlsx");
    fixture_rows(
        &source,
        &[
            vec![
                "Работа",
                "Количество",
                "Сумма",
                "Работа",
                "Количество",
                "Сумма",
            ],
            vec!["СМУ16", "1", "2", "СМУ16", "1", "2"],
            vec!["Альфа", "1", "2", "Бета", "1", "2"],
        ],
    );
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let mut stage = area
        .ingest(&source, Limits::default(), &cancel, |_| {})
        .unwrap();
    stage
        .convert_legacy(&options("day", "2026-11-01"), &cancel, |_| {})
        .unwrap();
    let original = stage
        .report_page(0, 100)
        .unwrap()
        .iter()
        .map(|r| r.start.clone())
        .collect::<Vec<_>>();
    for (kind, dates) in [
        ("day", vec![]),
        ("day", vec!["2026-11-01"]),
        ("day", vec!["2026-11-01", ""]),
        ("day", vec!["2026-11-01", "2026-02-30"]),
        ("day", vec!["2026-11-01", "2026-11-01"]),
        ("week", vec!["2026-11-01", "2026-11-07"]),
        ("month", vec!["2026-11-01", "2026-12-02"]),
        ("week", vec!["2026-11-01", "9999-12-31"]),
        ("day", vec!["2026-11-01"; 685]),
    ] {
        let mut cfg = options(kind, "2026-11-01");
        cfg.period_dates = Some(dates.into_iter().map(String::from).collect());
        assert!(stage.convert_legacy(&cfg, &cancel, |_| {}).is_err());
        assert_eq!(
            stage
                .report_page(0, 100)
                .unwrap()
                .iter()
                .map(|r| r.start.clone())
                .collect::<Vec<_>>(),
            original
        );
    }
    let mut cfg = options("day", "2026-11-01");
    cfg.period_dates = Some(vec!["2026-11-03".into(), "2026-11-05".into()]);
    let json = serde_json::to_value(&cfg).unwrap();
    assert_eq!(
        json["periodDates"],
        serde_json::json!(["2026-11-03", "2026-11-05"])
    );
    let mut omitted = json;
    omitted.as_object_mut().unwrap().remove("periodDates");
    assert!(serde_json::from_value::<LegacyOptions>(omitted)
        .unwrap()
        .period_dates
        .is_none());
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
    let invalid =
        serde_json::to_value(issues.iter().find(|i| i.kind == "invalid").unwrap()).unwrap();
    assert_eq!(invalid["expected"], serde_json::Value::Null);
    assert_eq!(invalid["actual"], serde_json::Value::Null);
    assert_eq!(invalid["cells"], serde_json::json!(["B4"]));
    assert_eq!(invalid["skipped"], true);
    assert_eq!(invalid["type"], "invalid");
    assert_eq!(invalid["row"], 4);
    assert_eq!(invalid["count_cell"], "B4");
    assert_eq!(invalid["work_cell"], serde_json::Value::Null);
    let mismatch = serde_json::to_value(
        issues
            .iter()
            .find(|i| i.kind == "mismatch" && i.source_row == Some(2))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(mismatch["cells"], serde_json::json!(["B2", "C2"]));
    assert_eq!(
        mismatch["expected"],
        serde_json::json!({"count":3.0,"amount":12.3})
    );
    assert_eq!(
        mismatch["actual"],
        serde_json::json!({"count":1.2,"amount":5.3})
    );
    assert_eq!(mismatch["expected_count"], 3.0);
    assert_eq!(mismatch["expected_amount"], 12.3);
    assert_eq!(mismatch["actual_count"], 1.2);
    assert_eq!(mismatch["actual_amount"], 5.3);
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
    fixture_rows(&source, &[
        vec!["Работа","Количество","Сумма","Работа","Количество","Сумма"],
        vec!["СМУ16","3","12.30","АВТО","3","12.30"],
        vec!["Альфа","1.2","5.30","Альфа","1.2","5.30"],
        vec!["Бета","1.8","7","Бета","1.8","7"],
        vec!["Итого","3","12.30","Итого","3","12.30"],
    ]);
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
    cfg.mappings = Some(vec![mapping, TripleMapping {header_row:1, work_col:4, count_col:5, amount_col:6}]);
    let cancel = AtomicBool::new(false);
    assert!(stage
        .convert_legacy(&cfg, &cancel, |summary| {
            if summary.reports == 1 {
                cancel.store(true, std::sync::atomic::Ordering::Release);
            }
        })
        .is_err());
    assert!(cancel.load(std::sync::atomic::Ordering::Acquire), "Cancel was requested after the first converted report");
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
        let serialized = serde_json::to_value(&issue).unwrap();
        let expected_cells: Vec<_> = [Some("A3"), count_cell, amount_cell]
            .into_iter()
            .flatten()
            .collect();
        assert_eq!(serialized["cells"], serde_json::json!(expected_cells));
        assert_eq!(serialized["expected"], serde_json::Value::Null);
        assert_eq!(serialized["actual"], serde_json::Value::Null);
        assert_eq!(serialized["skipped"], true);
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
            vec!["Работа", "Количество", "Сумма", "Работа", "Количество", "", "", "Сумма"],
            vec!["", "", "", "ТРАК", "0", "", "", "0"],
            vec!["СМУ16", "1", "2", "СМУ16", "1", "", "", "2"],
            vec!["Работа1", "1", "2", "Работа1", "1", "", "", "2"],
            vec!["Итого", "1", "2", "Итого", "1", "", "", "2"],
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
        },
        TripleMapping { header_row: 1, work_col: 1, count_col: 2, amount_col: 3 }
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

#[test]
fn omitted_detail_tag_does_not_mark_invalid_controls_or_included_unnamed_rows() {
    let temp=tempfile::tempdir().unwrap();let source=temp.path().join("evidence.xlsx");
    fixture_rows(&source,&[
        vec!["Работа","Количество","Сумма"],
        vec!["СМУ16","3","8"],
        vec!["Included","1","5"],
        vec!["Rejected","bad","99"],
        vec!["","2","3"],
        vec!["АВТО","bad","7"],
        vec!["Unknown organization","4","9"],
        vec!["Итого","bad","8"],
    ]);
    let area=StageArea::open(temp.path().join("stage")).unwrap();let cancel=AtomicBool::new(false);
    let mut stage=area.ingest(&source,Limits::default(),&cancel,|_|{}).unwrap();
    stage.convert_legacy(&options("day","2020-03-01"),&cancel,|_|{}).unwrap();
    let report=stage.report_page(0,1).unwrap().remove(0);assert_eq!(report.row_count,2);assert_eq!(report.count,3.);assert_eq!(report.amount,8.);
    let notes=stage.report_issues(&report.id,0,50).unwrap();
    let find=|row|serde_json::to_value(notes.iter().find(|n|n.source_row==Some(row)).unwrap()).unwrap();
    assert_eq!(find(4)["skipped"],true);
    assert_eq!(find(4)["organization"],"СМУ16");
    assert_eq!(find(5)["kind"],"empty-name");assert!(find(5).get("skipped").is_none());
    assert_eq!(find(6)["kind"],"invalid");assert!(find(6).get("skipped").is_none());
    assert_eq!(find(7)["kind"],"skipped");assert_eq!(find(7)["skipped"],true);assert!(find(7)["organization"].is_null());
    assert_eq!(find(8)["kind"],"invalid");assert!(find(8).get("skipped").is_none());
    let prepared=stage.prepare(&cancel,|_|{}).unwrap();let saved=prepared.next_report(0).unwrap().unwrap();
    let document:serde_json::Value=serde_json::from_reader(prepared.open_document(&saved).unwrap()).unwrap();
    let persisted=document["issues"].as_array().unwrap();assert_eq!(persisted.iter().filter(|i|i["skipped"]==true).count(),2);
    assert_eq!(document["rows"].as_array().unwrap().len(),2);assert_eq!(document["count"],3.);assert_eq!(document["amount"],8.);
}

#[test]
fn only_verified_header_dates_create_a_manual_quality_notice() {
    for (label, adjacent, early_1904, kind, start, expected_address) in [
        ("Начало периода", r#"<c r="B1" s="1"><v>60</v></c>"#, false, "day", "2026-11-09", None),
        ("Начало периода", r#"<c r="B1" s="1"><v>60</v></c>"#, true, "day", "2026-11-09", Some("B1")),
        ("Начало периода", r#"<c r="B1"><v>46000</v></c>"#, false, "day", "2026-11-09", None),
        ("Начало периода", r#"<c r="B1" s="2"><v>46000</v></c>"#, false, "week", "2026-11-09", None),
        ("Начало периода", r#"<c r="B1" s="3"><v>46000</v></c>"#, false, "week", "2026-11-09", None),
        ("Начало периода", r#"<c r="B1" s="4"><v>46000</v></c>"#, false, "week", "2026-11-09", None),
        ("Начало периода", r#"<c r="B1" s="5"><v>46000</v></c>"#, false, "week", "2026-11-09", Some("B1")),
        ("Начало периода", r#"<c r="B1" t="d"><v>2026-11-09T00:00:00</v></c>"#, false, "week", "2026-10-19", Some("B1")),
        ("Начало периода", r#"<c r="B1" t="d"><v>2024-02-29T23:43:21.125-05:00</v></c>"#, false, "day", "2024-02-29", None),
        ("Начало периода", r#"<c r="B1" t="d"><v>2024-02-29T23:43:21.125-05:00</v></c>"#, false, "day", "2024-03-01", Some("B1")),
        ("Начало периода", r#"<c r="B1" t="d"><v>2024-02-29Tinvalid</v></c>"#, false, "day", "2024-03-01", None),
        ("Дата печати: 09.11.2026", "", false, "week", "2026-10-19", None),
        ("Период с 2024-02 по 29.02.2024", "", false, "month", "2024-03-01", None),
        ("Период с 01.02.2024 по 29.02.2024", "", false, "month", "2024-03-01", Some("A1")),
        ("Начало месяца: 2024-02", "", false, "month", "2024-02-01", None),
        ("Начало месяца: 2024-02", "", false, "month", "2024-03-01", Some("A1")),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("dates.xlsx");
        let mut workbook = ZipWriter::new(fs::File::create(&file).unwrap());
        workbook.start_file("xl/workbook.xml", SimpleFileOptions::default()).unwrap();
        write!(workbook, r#"<workbook><workbookPr date1904="{}"/></workbook>"#, early_1904 as u8).unwrap();
        workbook.start_file("xl/styles.xml", SimpleFileOptions::default()).unwrap();
        write!(workbook, r#"<styleSheet><numFmts>
            <numFmt numFmtId="164" formatCode="[Red]0.00"/><numFmt numFmtId="165" formatCode="0_d"/>
            <numFmt numFmtId="166" formatCode="[$-409]dd-mmm-yyyy"/></numFmts>
            <cellXfs><xf numFmtId="0"/><xf numFmtId="14"/><xf numFmtId="164"/><xf numFmtId="21"/>
            <xf numFmtId="165"/><xf numFmtId="166"/></cellXfs></styleSheet>"#).unwrap();
        workbook.start_file("xl/worksheets/sheet1.xml", SimpleFileOptions::default()).unwrap();
        write!(workbook, r#"<worksheet><sheetData>
        <row r="1"><c r="A1" t="inlineStr"><is><t>{label}</t></is></c>{adjacent}</row>
        <row r="2"><c r="A2" t="inlineStr"><is><t>Работа</t></is></c><c r="B2" t="inlineStr"><is><t>Количество</t></is></c><c r="C2" t="inlineStr"><is><t>Сумма</t></is></c></row>
        <row r="3"><c r="A3" t="inlineStr"><is><t>СМУ16</t></is></c><c r="B3"><v>1</v></c><c r="C3"><v>125</v></c></row>
        <row r="4"><c r="A4" t="inlineStr"><is><t>Работа А</t></is></c><c r="B4"><v>1</v></c><c r="C4"><v>125</v></c></row>
        </sheetData></worksheet>"#).unwrap();
        workbook.finish().unwrap();
        let area = StageArea::open(temp.path().join("stage")).unwrap();
        let cancel = AtomicBool::new(false);
        let mut stage = area.ingest(&file, Limits::default(), &cancel, |_| {}).unwrap();
        let summary = stage.convert_legacy(&options(kind, start), &cancel, |_| {}).unwrap();
        assert_eq!(summary.issues, if expected_address.is_some() {1} else {0}, "{label} {adjacent}");
        let report = stage.report_page(0, 1).unwrap().remove(0);
        assert_eq!(report.amount, 125.);
        if let Some(address) = expected_address {
            let issue = stage.report_issues(&report.id, 0, 1).unwrap().remove(0);
            assert_eq!(issue.work_cell.as_deref(), Some(address));
        }
    }
}

#[test]
fn reordered_manual_columns_keep_the_period_date_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("reordered.xlsx");
    fixture_rows(&file, &[
        vec!["Начало периода", "2026-11-09"],
        vec!["Количество", "Сумма", "Работа"],
        vec!["1", "125", "СМУ16"],
        vec!["1", "125", "Работа А"],
    ]);
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let mut stage = area.ingest(&file, Limits::default(), &cancel, |_| {}).unwrap();
    let mut cfg = options("week", "2026-10-19");
    cfg.mappings = Some(vec![TripleMapping { header_row: 2, work_col: 3, count_col: 1, amount_col: 2 }]);
    let summary = stage.convert_legacy(&cfg, &cancel, |_| {}).unwrap();
    assert_eq!(summary.issues, 1);
    let report = stage.report_page(0, 1).unwrap().remove(0);
    assert_eq!((report.count, report.amount), (1., 125.));
    let issue = stage.report_issues(&report.id, 0, 1).unwrap().remove(0);
    assert_eq!(issue.work_cell.as_deref(), Some("B1"));
}

#[test]
fn interleaved_manual_columns_only_accept_the_first_blocks_own_label_and_date() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("interleaved.xlsx");
    fixture_rows(&file, &[
        vec!["Начало периода", "2026-11-02", "Начало периода: 2026-11-16", "", "Начало периода", "2026-11-09"],
        vec!["", "", "", "Начало периода", "2026-11-23"],
        vec!["Количество", "Работа", "Количество", "Сумма", "Работа", "Сумма"],
        vec!["1", "СМУ16", "2", "60", "СМУ16", "125"],
        vec!["1", "Работа Б", "2", "60", "Работа А", "125"],
    ]);
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = AtomicBool::new(false);
    let mut stage = area.ingest(&file, Limits::default(), &cancel, |_| {}).unwrap();
    let mut cfg = options("week", "2026-10-19");
    cfg.mappings = Some(vec![
        TripleMapping { header_row: 3, work_col: 5, count_col: 1, amount_col: 6 },
        TripleMapping { header_row: 3, work_col: 2, count_col: 3, amount_col: 4 },
    ]);
    let summary = stage.convert_legacy(&cfg, &cancel, |_| {}).unwrap();
    assert_eq!(summary.issues, 1);
    let reports = stage.report_page(0, 2).unwrap();
    assert_eq!((reports[0].amount, reports[1].amount), (125., 60.));
    let issue = stage.report_issues(&reports[0].id, 0, 1).unwrap().remove(0);
    assert_eq!(issue.work_cell.as_deref(), Some("F1"));
}

#[test]
fn repeated_department_controls_are_checked_per_section() {
    let t = tempfile::tempdir().unwrap();
    let area = StageArea::open(t.path().join("stage")).unwrap();
    let file = t.path().join("repeat.xlsx");
    let cancel = AtomicBool::new(false);
    for (count, amount, valid) in [("2", "20", true), ("3", "30", false)] {
        fixture_rows(&file, &[
            vec!["Работа", "Количество", "Сумма"],
            vec!["СМУ16", "1", "10"], vec!["А", "1", "10"],
            vec!["СМУ16", count, amount], vec!["Б", "2", "20"], vec!["Итого", "3", "30"],
        ]);
        let mut stage = area.ingest(&file, Limits::default(), &cancel, |_| {}).unwrap();
        let s = stage.convert_legacy(&options("day", "2024-02-29"), &cancel, |_| {}).unwrap();
        let orgs = stage.report_organizations("legacy-1").unwrap();
        assert_eq!((orgs.len(), orgs[0].count, orgs[0].amount), (1, 3., 30.));
        assert_eq!(s.issues, if valid { 0 } else { 1 });
        if !valid {
            let issue = stage.report_issues("legacy-1", 0, 1).unwrap().remove(0);
            assert_eq!(issue.source_row, Some(4));
            assert_eq!(issue.expected_amount, Some(30.));
            assert_eq!(issue.actual_amount, Some(20.));
        }
    }
}
