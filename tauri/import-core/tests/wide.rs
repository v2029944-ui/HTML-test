use analiz_import_core::{ImportMode, Limits, StageArea, WideMapping, WideOptions};
use std::{
    fs,
    io::Write,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use zip::{write::SimpleFileOptions, ZipWriter};
fn book(path: &std::path::Path, rows: &[Vec<&str>], extra: &str, target: &str) {
    const SHEETS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    const DOC_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    const PACKAGE: &str = "http://schemas.openxmlformats.org/package/2006";
    let mut xml = format!(r#"<worksheet xmlns="{SHEETS}"><sheetData>"#);
    for (r, row) in rows.iter().enumerate() {
        xml.push_str(&format!("<row r=\"{}\">", r + 1));
        for (c, v) in row.iter().enumerate() {
            if !v.is_empty() {
                xml.push_str(&format!(
                    "<c r=\"{}{}\" t=\"inlineStr\"><is><t>{v}</t></is></c>",
                    (b'A' + c as u8) as char,
                    r + 1
                ));
            }
        }
        xml.push_str("</row>");
    }
    xml.push_str("</sheetData>");
    xml.push_str(extra);
    xml.push_str("</worksheet>");
    let mut z = ZipWriter::new(fs::File::create(path).unwrap());
    let parts = [
        (target, xml),
        ("[Content_Types].xml", format!(r#"<Types xmlns="{PACKAGE}/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/{target}" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#)),
        ("_rels/.rels", format!(r#"<Relationships xmlns="{PACKAGE}/relationships"><Relationship Id="r1" Type="{DOC_REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#)),
        ("xl/workbook.xml", format!(r#"<workbook xmlns="{SHEETS}" xmlns:r="{DOC_REL}"><sheets><sheet name="Data" sheetId="1" r:id="r1"/></sheets></workbook>"#)),
        ("xl/_rels/workbook.xml.rels", format!(r#"<Relationships xmlns="{PACKAGE}/relationships"><Relationship Id="r1" Target="/{target}" Type="{DOC_REL}/worksheet"/></Relationships>"#)),
    ];
    for (name, content) in parts {
        z.start_file(name, SimpleFileOptions::default()).unwrap();
        z.write_all(content.as_bytes()).unwrap();
    }
    z.finish().unwrap();
}
fn rows() -> Vec<Vec<&'static str>> {
    vec![
        vec![
            "Работа",
            "2024-02-28",
            "",
            "2024-02-29",
            "",
            "Итого за период",
            "",
        ],
        vec![
            "",
            "Количество",
            "Сумма",
            "Количество",
            "Сумма",
            "Количество",
            "Сумма",
        ],
        vec!["СМУ16", "1", "10", "2", "20", "3", "30"],
        vec!["Work", "1", "10", "2", "20", "3", "30"],
        vec!["Итого", "1", "10", "2", "20", "3", "30"],
    ]
}
const MERGES:&str="<mergeCells><mergeCell ref=\"A1:A2\"/><mergeCell ref=\"B1:C1\"/><mergeCell ref=\"D1:E1\"/><mergeCell ref=\"F1:G1\"/></mergeCells>";
#[test]
fn detects_converts_merges_and_nonstandard_relationship_target() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("w.xlsx");
    book(&p, &rows(), MERGES, "data/sheet.xml");
    let a = StageArea::open(t.path().join("stage")).unwrap();
    let c = Arc::new(AtomicBool::new(false));
    let mut s = a.ingest(&p, Limits::default(), &c, |_| {}).unwrap();
    let cfg = WideOptions::default();
    let sheets = s.detect_wide(&cfg, c.clone()).unwrap();
    assert_eq!(sheets.len(), 1);
    assert_eq!(sheets[0].name, "Data");
    let summary = s.convert_wide(&cfg, c.clone(), |_| {}).unwrap();
    assert_eq!((summary.reports, summary.rows, summary.issues), (2, 2, 0));
    let r = s.report_page(0, 100).unwrap();
    assert_eq!(
        (r[0].start.as_str(), r[1].start.as_str()),
        ("2024-02-28", "2024-02-29")
    );
    assert_eq!(s.report_rows(&r[1].id, 0, 100).unwrap()[0].count_cell, "D4");
    let p = s.report_provenance(&r[0].id).unwrap().unwrap();
    assert!(!p.explicit_zero);
    assert_eq!(p.coverage.start, "2024-02-28");
    assert_eq!(s.report_totals(&r[0].id, 0, 100).unwrap().len(), 2);
    // C3 prepares the wide reports with their shared diagnostic anchor intact.
    let manual = s.prepare(c.as_ref(), |_| {}).unwrap();
    assert_eq!(manual.report_count(), 2);
    let first = manual.next_report(0).unwrap().unwrap();
    let second = manual.next_report(first.ordinal).unwrap().unwrap();
    assert!(first.provenance.is_some());
    assert!(second.provenance.is_some());
    let doc: serde_json::Value =
        serde_json::from_reader(manual.open_document(&second).unwrap()).unwrap();
    assert_eq!(doc["sourceDiagnosticsReport"], first.id);
    let mut folder_stage = a
        .ingest(&t.path().join("w.xlsx"), Limits::default(), c.as_ref(), |_| {})
        .unwrap();
    folder_stage.convert_wide(&cfg, c.clone(), |_| {}).unwrap();
    let folder = folder_stage
        .prepare_with_mode(ImportMode::Folder, c.as_ref(), |_| {})
        .unwrap();
    assert_eq!(folder.report_count(), 2);
    assert!(folder.next_report(0).unwrap().unwrap().provenance.is_some());
}
#[test]
fn blank_days_source_totals_and_zero_are_distinct() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("w.xlsx");
    let mut v = rows();
    for row in &mut v[2..] {
        row[3] = "";
        row[4] = "";
    }
    v[3][1] = "";
    v[3][2] = "";
    v[2][1] = "0";
    v[2][2] = "0";
    v[4][1] = "0";
    v[4][2] = "0";
    book(&p, &v, MERGES, "xl/worksheets/sheet1.xml");
    let a = StageArea::open(t.path().join("stage")).unwrap();
    let c = Arc::new(AtomicBool::new(false));
    let mut s = a.ingest(&p, Limits::default(), &c, |_| {}).unwrap();
    s.convert_wide(&WideOptions::default(), c, |_| {}).unwrap();
    let r = s.report_page(0, 100).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].row_count, 0);
    let p = s.report_provenance(&r[0].id).unwrap().unwrap();
    assert!(p.explicit_zero);
    assert_eq!(p.source_control_date.as_deref(), Some("2024-02-28"));
    let issues = s.source_issues("Data", 0, 100).unwrap();
    assert_eq!(issues.len(), 4);
    assert!(issues
        .iter()
        .any(|i| i.date.as_deref() == Some("2024-02-29") && i.kind == "missing-day"));
    assert!(issues
        .iter()
        .any(|i| i.count_cell.as_deref() == Some("F4") && i.actual_count == Some(0.0)));
}
#[test]
fn duplicate_dates_manual_mapping_and_atomic_cancellation() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("w.xlsx");
    book(&p, &rows(), MERGES, "xl/worksheets/sheet1.xml");
    let a = StageArea::open(t.path().join("stage")).unwrap();
    let c = Arc::new(AtomicBool::new(false));
    let mut s = a.ingest(&p, Limits::default(), &c, |_| {}).unwrap();
    let mut cfg = WideOptions::default();
    cfg.sheets = vec!["Data".into()];
    cfg.mappings.insert(
        "Data".into(),
        WideMapping {
            work_col: 1,
            header_row: 2,
            first_day_col: 2,
            total_count_col: Some(6),
            total_amount_col: Some(7),
        },
    );
    s.convert_wide(&cfg, c.clone(), |_| {}).unwrap();
    let previous = s.report_page(0, 100).unwrap();
    assert!(s
        .convert_wide(&cfg, c.clone(), |x| {
            if x.reports == 1 {
                c.store(true, Ordering::Release)
            }
        })
        .is_err());
    c.store(false, Ordering::Release);
    assert_eq!(s.report_page(0, 100).unwrap()[1].start, previous[1].start);
    cfg.year = Some(2025);
    assert!(s.convert_wide(&cfg, c.clone(), |_| {}).is_err());
    assert_eq!(s.report_page(0, 100).unwrap().len(), 2);
}
#[test]
fn daily_mappings_cannot_silently_drop_a_worksheet_or_an_amount_total() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("days.xlsx");
    book(&path, &rows(), MERGES, "xl/worksheets/sheet1.xml");
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut stage = area.ingest(&path, Limits::default(), &cancel, |_| {}).unwrap();
    let mut options = WideOptions::default();
    stage.convert_wide(&options, cancel.clone(), |_| {}).unwrap();
    let reports = |stage: &analiz_import_core::Stage<'_>| {
        stage.report_page(0, 10).unwrap().into_iter()
            .map(|r| (r.start, r.count, r.amount)).collect::<Vec<_>>()
    };
    let prior = reports(&stage);
    assert_eq!(prior.len(), 2);
    let mut mapping = WideMapping {
        header_row: 2, work_col: 1, first_day_col: 2,
        total_count_col: Some(6), total_amount_col: Some(7),
    };
    options.mappings.insert("Старое имя".into(), mapping.clone());
    let error = stage.convert_wide(&options, cancel.clone(), |_| {}).unwrap_err().to_string();
    assert!(error.contains("Старое имя"), "{error}");
    assert_eq!(reports(&stage), prior);
    options.mappings.clear();
    mapping.total_count_col = None;
    options.mappings.insert("Data".into(), mapping);
    let error = stage.convert_wide(&options, cancel, |_| {}).unwrap_err().to_string();
    assert!(error.contains("итогового количества"), "{error}");
    assert_eq!(reports(&stage), prior);
}

#[test]
fn ambiguous_short_year_and_conflicting_pair_reject() {
    for (first, second) in [
        ("28.02", "29.02"),
        ("2024-02-28", "2024-02-28"),
        ("2023-02-29", "2023-03-01"),
    ] {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("w.xlsx");
        let mut v = rows();
        v[0][1] = first;
        v[0][3] = second;
        book(&p, &v, MERGES, "xl/worksheets/sheet1.xml");
        let a = StageArea::open(t.path().join("stage")).unwrap();
        let c = Arc::new(AtomicBool::new(false));
        let mut s = a.ingest(&p, Limits::default(), &c, |_| {}).unwrap();
        assert!(s.convert_wide(&WideOptions::default(), c, |_| {}).is_err());
    }
}

#[test]
fn joined_header_coverage_and_js_year_suffix_whitespace() {
    for (title, end) in [
        (vec!["Q", "1"], "2024-03-31"),
        (vec!["за", "2024", "год"], "2024-12-31"),
    ] {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("w.xlsx");
        let mut v = vec![title];
        let mut data = rows();
        data[0][1] = "28 февраля 2024\u{feff}г.";
        data[0][3] = "29 февраля 2024\u{a0}  г.";
        v.extend(data);
        // Shift the two header rows and their merged ranges down one row.
        let merges = MERGES.replace("A1:A2", "A2:A3").replace('1', "2");
        book(&p, &v, &merges, "xl/worksheets/sheet1.xml");
        let a = StageArea::open(t.path().join("stage")).unwrap();
        let c = Arc::new(AtomicBool::new(false));
        let mut s = a.ingest(&p, Limits::default(), &c, |_| {}).unwrap();
        s.convert_wide(&WideOptions::default(), c, |_| {}).unwrap();
        let r = s.report_page(0, 100).unwrap();
        let coverage = s.report_provenance(&r[0].id).unwrap().unwrap().coverage;
        assert_eq!(coverage.start, "2024-01-01");
        assert_eq!(coverage.end, end);
        assert!(s.source_issues("Data", 0, 100).unwrap().len() >= 89);
    }
}

#[test]
fn coverage_suffix_retains_word_boundary_context() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("w.xlsx");
    let title = format!("aQ1 {}", "x".repeat(60));
    let mut v = vec![vec![title.as_str(), "next"]];
    v.extend(rows());
    let merges = MERGES.replace("A1:A2", "A2:A3").replace('1', "2");
    book(&p, &v, &merges, "xl/worksheets/sheet1.xml");
    let a = StageArea::open(t.path().join("stage")).unwrap();
    let c = Arc::new(AtomicBool::new(false));
    let mut s = a.ingest(&p, Limits::default(), &c, |_| {}).unwrap();
    s.convert_wide(&WideOptions::default(), c, |_| {}).unwrap();
    let r = s.report_page(0, 100).unwrap();
    let coverage = s.report_provenance(&r[0].id).unwrap().unwrap().coverage;
    assert_eq!(
        (coverage.start.as_str(), coverage.end.as_str()),
        ("2024-02-28", "2024-02-29")
    );
    assert!(s.source_issues("Data", 0, 100).unwrap().is_empty());
}

#[test]
fn wide_omitted_detail_is_tagged_but_shared_source_controls_are_not() {
    let t=tempfile::tempdir().unwrap();let path=t.path().join("omitted.xlsx");
    let mut input=rows();input[3][1]="bad";input.insert(4,vec!["Included","1","10","0","0","1","10"]);
    book(&path,&input,MERGES,"xl/worksheets/sheet1.xml");
    let area=StageArea::open(t.path().join("stage")).unwrap();let cancel=Arc::new(AtomicBool::new(false));
    let mut stage=area.ingest(&path,Limits::default(),&cancel,|_|{}).unwrap();
    stage.convert_wide(&WideOptions::default(),cancel.clone(),|_|{}).unwrap();
    let prepared=stage.prepare(cancel.as_ref(),|_|{}).unwrap();let report=prepared.next_report(0).unwrap().unwrap();
    let doc:serde_json::Value=serde_json::from_reader(prepared.open_document(&report).unwrap()).unwrap();
    assert_eq!(doc["count"],1.);assert_eq!(doc["amount"],10.);assert_eq!(doc["rows"].as_array().unwrap().len(),1);
    let issues=doc["issues"].as_array().unwrap();let omitted=issues.iter().find(|i|i["kind"]=="invalid"&&i["source_row"]==4&&i["scope"].is_null()).unwrap();
    assert_eq!(omitted["skipped"],true);assert_eq!(omitted["organization"],"СМУ16");
    let source_issues=doc["sourceIssues"].as_array().unwrap();assert!(!source_issues.is_empty());
    assert!(source_issues.iter().all(|i|i.get("skipped").is_none()));
    let source_copies:Vec<_>=issues.iter().filter(|i|i["scope"]=="source-period").collect();assert!(!source_copies.is_empty());
    assert!(source_copies.iter().all(|i|i.get("skipped").is_none()));
}

fn excel_column(mut col: usize) -> String {
    let mut out = String::new();
    while col > 0 {
        col -= 1;
        out.insert(0, (b'A' + (col % 26) as u8) as char);
        col /= 26;
    }
    out
}

// Real XLSX worksheets with one existing quantity/amount pair per day.
fn sequential_book(
    path: &std::path::Path,
    sheets: &[(String, String, usize)],
    date_headers: &[(usize, usize, String)],
    blank_day: Option<(usize, usize)>,
) {
    let mut zip = ZipWriter::new(fs::File::create(path).unwrap());
    let mut workbook = String::from("<workbook xmlns:r=\"r\"><sheets>");
    let mut relationships = String::from("<Relationships>");
    for (index, (name, title, pairs)) in sheets.iter().enumerate() {
        workbook.push_str(&format!("<sheet name=\"{name}\" r:id=\"r{index}\"/>"));
        relationships.push_str(&format!("<Relationship Id=\"r{index}\" Target=\"worksheets/sheet{index}.xml\" Type=\"worksheet\"/>"));
        let mut xml = String::from("<worksheet><sheetData>");
        for row in 1..=5 {
            xml.push_str(&format!("<row r=\"{row}\">"));
            for col in 1..=2 * pairs + 3 {
                let value = if col == 1 {
                    [title.as_str(), "Работа", "СМУ16", "Work", "Итого"][row - 1].to_owned()
                } else if row == 1 {
                    if col == 2 * pairs + 2 {
                        "Итого за период".into()
                    } else {
                        date_headers
                            .iter()
                            .find(|(s, day, _)| *s == index && col == day * 2 + 2)
                            .map(|(_, _, value)| value.clone())
                            .unwrap_or_default()
                    }
                } else if row == 2 {
                    if col % 2 == 0 {
                        "Количество".into()
                    } else {
                        "Сумма".into()
                    }
                } else if col >= 2 * pairs + 2 {
                    (pairs * if col % 2 == 0 { 1 } else { 10 }).to_string()
                } else if blank_day == Some((index, (col - 2) / 2)) {
                    String::new()
                } else if col % 2 == 0 {
                    "1".into()
                } else {
                    "10".into()
                };
                if !value.is_empty() {
                    xml.push_str(&format!(
                        "<c r=\"{}{row}\" t=\"inlineStr\"><is><t>{value}</t></is></c>",
                        excel_column(col)
                    ));
                }
            }
            xml.push_str("</row>");
        }
        xml.push_str("</sheetData></worksheet>");
        zip.start_file(
            format!("xl/worksheets/sheet{index}.xml"),
            SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    workbook.push_str("</sheets></workbook>");
    relationships.push_str("</Relationships>");
    for (name, xml) in [
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", relationships),
    ] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

fn sequential_options(year: i32, quarters: &[Option<u32>]) -> WideOptions {
    let mut options = WideOptions::default();
    for (index, quarter) in quarters.iter().enumerate() {
        let name = format!("Sheet{index}");
        options.sheets.push(name.clone());
        options.sequential_periods.insert(
            name,
            analiz_import_core::WideSequentialPeriod {
                year,
                quarter: *quarter,
            },
        );
    }
    options
}

#[test]
fn sequential_four_quarters_and_whole_year_preserve_daily_values() {
    for year in [2023, 2024] {
        for quarters in [vec![Some(1), Some(2), Some(3), Some(4)], vec![None]] {
            let cfg = sequential_options(year, &quarters);
            let counts = if quarters.len() == 4 {
                vec![if year == 2024 { 91 } else { 90 }, 91, 92, 92]
            } else {
                vec![if year == 2024 { 366 } else { 365 }]
            };
            let sheets: Vec<_> = counts
                .iter()
                .enumerate()
                .map(|(index, count)| (format!("Sheet{index}"), year.to_string(), *count))
                .collect();
            let t = tempfile::tempdir().unwrap();
            let p = t.path().join("sequential.xlsx");
            sequential_book(&p, &sheets, &[], None);
            let area = StageArea::open(t.path().join("stage")).unwrap();
            let cancel = Arc::new(AtomicBool::new(false));
            let mut stage = area.ingest(&p, Limits::default(), &cancel, |_| {}).unwrap();
            let summary = stage.convert_wide(&cfg, cancel, |_| {}).unwrap();
            assert_eq!(
                (summary.reports, summary.rows, summary.issues),
                (
                    counts.iter().sum::<usize>() as u64,
                    counts.iter().sum::<usize>() as u64,
                    0
                )
            );
            let mut reports = Vec::new();
            loop {
                let page = stage.report_page(reports.len() as u64, 100).unwrap();
                if page.is_empty() {
                    break;
                }
                reports.extend(page);
            }
            assert_eq!(reports.first().unwrap().start, format!("{year}-01-01"));
            assert_eq!(reports.last().unwrap().start, format!("{year}-12-31"));
            let mut offset = 0;
            for count in counts {
                let coverage = stage
                    .report_provenance(&reports[offset].id)
                    .unwrap()
                    .unwrap()
                    .coverage;
                assert_eq!(coverage.start, reports[offset].start);
                assert_eq!(coverage.end, reports[offset + count - 1].start);
                offset += count;
            }
            assert_eq!(
                stage.report_rows(&reports[0].id, 0, 100).unwrap()[0].count_cell,
                "B4"
            );
            assert!(stage.source_issue_page(0, 100).unwrap().is_empty());
        }
    }
}

#[test]
fn sequential_rejects_bad_counts_years_headers_and_unused_metadata() {
    for (pairs, title, date, configured_year, quarter, extra) in [
        (90, "2024", None, None, Some(1), false),
        (92, "2024", None, None, Some(1), false),
        (91, "2023", None, None, Some(1), false),
        (91, "2024 Q2", None, None, Some(1), false),
        (91, "2024 Q1 Q2", None, None, Some(1), false),
        (91, "2024 Q5", None, None, Some(1), false),
        (91, "2024 V квартал", None, None, Some(1), false),
        (91, "2024", Some("2024-01-02"), None, Some(1), false),
        (91, "2024", Some("2023-01-01"), None, Some(1), false),
        (91, "2024", Some("01.01.23"), None, Some(1), false),
        (91, "2024", None, Some(2023), Some(1), false),
        (91, "2024", None, None, Some(0), false),
        (91, "2024", None, None, Some(5), false),
        (91, "2024", None, None, Some(1), true),
        (365, "2024", None, None, None, false),
        (366, "2024 Q1", None, None, None, false),
    ] {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("sequential.xlsx");
        let headers = date.map(|d| vec![(0, 0, d.to_owned())]).unwrap_or_default();
        sequential_book(
            &p,
            &[("Sheet0".into(), title.into(), pairs)],
            &headers,
            None,
        );
        let area = StageArea::open(t.path().join("stage")).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let mut stage = area.ingest(&p, Limits::default(), &cancel, |_| {}).unwrap();
        let mut cfg = sequential_options(2024, &[quarter]);
        cfg.year = configured_year;
        if extra {
            cfg.sequential_periods.insert(
                "Unknown".into(),
                analiz_import_core::WideSequentialPeriod {
                    year: 2024,
                    quarter: Some(1),
                },
            );
        }
        assert!(
            stage.convert_wide(&cfg, cancel, |_| {}).is_err(),
            "accepted {pairs} / {title} / {date:?} / {configured_year:?} / {quarter:?} / {extra}"
        );
        assert!(stage.report_page(0, 100).is_err());
    }
}

#[test]
fn sequential_validates_dates_and_keeps_blank_days_missing_despite_nonzero_totals() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("sequential.xlsx");
    sequential_book(
        &p,
        &[("Sheet0".into(), "2024 Q1".into(), 91)],
        &[(0, 0, "2024-01-01".into()), (0, 90, "31.03".into())],
        Some((0, 1)),
    );
    let area = StageArea::open(t.path().join("stage")).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut stage = area.ingest(&p, Limits::default(), &cancel, |_| {}).unwrap();
    let cfg = sequential_options(2024, &[Some(1)]);
    assert_eq!(
        stage
            .convert_wide(&cfg, cancel.clone(), |_| {})
            .unwrap()
            .reports,
        90
    );
    let issues = stage.source_issue_page(0, 100).unwrap();
    assert!(issues
        .iter()
        .any(|i| i.kind == "missing-day" && i.date.as_deref() == Some("2024-01-02")));
    assert!(issues.iter().any(|i| i.kind == "mismatch"));
    assert!(stage
        .report_page(0, 100)
        .unwrap()
        .iter()
        .all(|r| r.start != "2024-01-02"));
    // Opt-in must not change the original date-header parser.
    assert!(stage
        .convert_wide(&WideOptions::default(), cancel, |_| {})
        .is_err());
    assert_eq!(stage.report_page(0, 100).unwrap().len(), 90);
}

#[test]
fn sequential_cancellation_and_unused_known_sheet_keep_previous_conversion() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("sequential.xlsx");
    sequential_book(
        &p,
        &[
            ("Sheet0".into(), "Year 2024 Q1".into(), 91),
            ("Sheet1".into(), "2024 Q2".into(), 91),
        ],
        &[],
        None,
    );
    let area = StageArea::open(t.path().join("stage")).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut stage = area.ingest(&p, Limits::default(), &cancel, |_| {}).unwrap();
    let mut cfg = sequential_options(2024, &[Some(1), Some(2)]);
    assert_eq!(
        stage
            .convert_wide(&cfg, cancel.clone(), |_| {})
            .unwrap()
            .reports,
        182
    );
    assert!(stage
        .convert_wide(&cfg, cancel.clone(), |progress| {
            if progress.reports == 92 {
                cancel.store(true, Ordering::Release);
            }
        })
        .is_err());
    cancel.store(false, Ordering::Release);
    assert_eq!(stage.report_page(100, 100).unwrap().len(), 82);
    cfg.sheets.pop();
    assert!(stage.detect_wide(&cfg, cancel.clone()).is_err());
    assert!(stage.convert_wide(&cfg, cancel, |_| {}).is_err());
    assert_eq!(stage.report_page(100, 100).unwrap().len(), 82);
}

#[test]
fn either_column_of_a_day_pair_can_supply_the_two_digit_year() {
    let t = tempfile::tempdir().unwrap();
    let file = t.path().join("years.xlsx");
    let area = StageArea::open(t.path().join("stage")).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    for (count, amount) in [("28.02", "28.02.24"), ("28.02.24", "28.02")] {
        let mut sheet = rows();
        sheet[0][1] = count;
        sheet[0][2] = amount;
        sheet[0][3] = "29.02";
        book(&file, &sheet, "", "xl/worksheets/sheet1.xml");
        let mut stage = area.ingest(&file, Limits::default(), &cancel, |_| {}).unwrap();
        let mut cfg = WideOptions { year: Some(2024), ..Default::default() };
        let result = stage.convert_wide(&cfg, cancel.clone(), |_| {}).unwrap();
        assert_eq!(result.reports, 2);
        cfg.year = Some(2020);
        let err = stage.convert_wide(&cfg, cancel.clone(), |_| {}).unwrap_err();
        assert!(err.to_string().contains("year"), "{err}");
        assert_eq!(stage.report_page(0, 2).unwrap()[0].start, "2024-02-28");
    }
}

#[test]
fn empty_grand_total_reports_the_last_daily_section_mismatch_once() {
    let t = tempfile::tempdir().unwrap();
    let mut sheet = rows();
    sheet[2][1] = "4";
    sheet[2][2] = "40";
    sheet[4][1] = "";
    sheet[4][2] = "";
    let file = t.path().join("total.xlsx");
    book(&file, &sheet, MERGES, "xl/worksheets/sheet1.xml");
    let area = StageArea::open(t.path().join("stage")).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut stage = area.ingest(&file, Limits::default(), &cancel, |_| {}).unwrap();
    stage.convert_wide(&WideOptions::default(), cancel, |_| {}).unwrap();
    let report = stage.report_page(0, 1).unwrap().remove(0);
    let issues = stage.report_issues(&report.id, 0, 100).unwrap();
    assert_eq!(issues.iter().filter(|i| i.kind == "mismatch" && i.scope.as_deref() != Some("source-period")).count(), 1);
}

#[test]
fn each_column_pair_is_one_week_or_month_in_manual_and_folder_import() {
    let temp = tempfile::tempdir().unwrap();
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let path = temp.path().join("periods.xlsx");
    let cancel = Arc::new(AtomicBool::new(false));
    let component = "xl/worksheets/sheet1.xml";
    // Unrepresented weeks and months must not be synthesized or added to either pair.
    for (kind, starts, ends) in [
        ("week", ["2024-02-26", "2024-03-11"], ["2024-03-03", "2024-03-17"]),
        ("month", ["2024-02-01", "2024-04-01"], ["2024-02-29", "2024-04-30"]),
    ] {
        let mut data = rows();
        data[0][1] = starts[0];
        data[0][3] = starts[1];
        book(&path, &data, MERGES, component);
        let options = WideOptions { kind: kind.into(), ..Default::default() };
        for mode in [ImportMode::Manual, ImportMode::Folder] {
            let mut stage = area.ingest(&path, Limits::default(), &cancel, |_| {}).unwrap();
            assert_eq!(stage.worksheet_layout(component).unwrap(), Some("daily-wide"));
            let result = stage.convert_wide(&options, cancel.clone(), |_| {}).unwrap();
            assert_eq!((result.reports, result.rows, result.issues), (2, 2, 0));
            let reports = stage.report_page(0, 10).unwrap();
            for (i, report) in reports.iter().enumerate() {
                assert_eq!((report.kind.as_str(), report.start.as_str(), report.end.as_str()), (kind, starts[i], ends[i]));
                let row = stage.report_rows(&report.id, 0, 10).unwrap().remove(0);
                assert_eq!(row.work, "Work");
                assert_eq!((row.count, row.amount), [(1.0, 10.0), (2.0, 20.0)][i]);
                assert_eq!(row.count_cell, ["B4", "D4"][i]);
            }
            let prepared = stage.prepare_with_mode(mode, &cancel, |_| {}).unwrap();
            assert_eq!(prepared.report_count(), 2);
            let first = prepared.next_report(0).unwrap().unwrap();
            let second = prepared.next_report(first.ordinal).unwrap().unwrap();
            assert_eq!((second.kind.as_str(), second.start.as_str(), second.end.as_str()), (kind, starts[1], ends[1]));
            let source = second.provenance.unwrap();
            assert_eq!(source.source_format, "period-pairs");
            assert_eq!((source.coverage.start.as_str(), source.coverage.end.as_str()), (starts[0], ends[1]));
        }
    }
}

#[test]
fn separate_work_columns_and_ambiguous_headers_are_distinguished() {
    let temp = tempfile::tempdir().unwrap();
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let path = temp.path().join("layout.xlsx");
    let sheet = "xl/worksheets/sheet1.xml";
    let cancel = AtomicBool::new(false);
    let independent = vec![
        vec!["Работа", "Количество", "Сумма", "Работа", "Количество", "Сумма"],
        vec!["СМУ16", "1", "10", "СМУ16", "2", "20"],
        vec!["Альфа", "1", "10", "Бета", "2", "20"],
        vec!["Итого", "1", "10", "Итого", "2", "20"],
    ];
    book(&path, &independent, "", sheet);
    let mut stage = area.ingest(&path, Limits::default(), &cancel, |_| {}).unwrap();
    assert_eq!(stage.worksheet_layout(sheet).unwrap(), Some("legacy"));
    for (kind, dates, last_end) in [
        ("day", ["2024-02-28", "2024-02-29"], "2024-02-29"),
        ("week", ["2024-02-26", "2024-03-04"], "2024-03-10"),
        ("month", ["2024-02-01", "2024-03-01"], "2024-03-31"),
    ] {
        let options = analiz_import_core::LegacyOptions {
            sheet: sheet.into(), kind: kind.into(), start: dates[0].into(),
            organizations: WideOptions::default().organizations, mappings: None,
            period_dates: Some(dates.iter().map(|date| date.to_string()).collect()),
        };
        assert_eq!(stage.convert_legacy(&options, &cancel, |_| {}).unwrap().reports, 2);
        let reports = stage.report_page(0, 10).unwrap();
        let left = stage.report_rows(&reports[0].id, 0, 10).unwrap().remove(0);
        let right = stage.report_rows(&reports[1].id, 0, 10).unwrap().remove(0);
        assert_eq!((left.work.as_str(), right.work.as_str()), ("Альфа", "Бета"));
        assert_eq!(reports[1].end, last_end);
    }
    for headers in [
        vec!["Работа", "Количество", "Сумма"],
        vec!["Работа", "Количество", "Сумма", "", "Количество", "Сумма"],
        vec!["Работа", "Количество", "Сумма", "Количество", "Сумма", "Работа", "Количество", "Сумма"],
    ] {
        book(&path, &[headers], "", sheet);
        let uncertain = area.ingest(&path, Limits::default(), &cancel, |_| {}).unwrap();
        assert_eq!(uncertain.worksheet_layout(sheet).unwrap(), None);
    }
}

#[test]
fn single_undated_pair_uses_only_an_explicit_start_and_multiple_pairs_require_headers() {
    let temp = tempfile::tempdir().unwrap();
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let path = temp.path().join("undated.xlsx");
    let sheet = "xl/worksheets/sheet1.xml";
    let cancel = Arc::new(AtomicBool::new(false));
    let single: Vec<_> = rows().into_iter().map(|r| r[..3].to_vec()).collect();
    for date_in_excel in [false, true] {
        let mut data = single.clone();
        data[0][1] = if date_in_excel { "2024-02-01" } else { "" };
        book(&path, &data, "", sheet);
        let mut stage = area.ingest(&path, Limits::default(), &cancel, |_| {}).unwrap();
        let mut cfg = WideOptions { kind: "month".into(), sheets: vec!["Data".into()], ..Default::default() };
        if !date_in_excel {
            assert!(stage.convert_wide(&cfg, cancel.clone(), |_| {}).is_err());
            cfg.start = Some("2024-02-01".into());
        }
        assert_eq!(stage.convert_wide(&cfg, cancel.clone(), |_| {}).unwrap().reports, 1);
        let previous = stage.report_page(0, 1).unwrap().remove(0);
        assert_eq!((previous.start.as_str(), previous.end.as_str()), ("2024-02-01", "2024-02-29"));
        // A mismatched or invalid confirmation cannot silently replace an earlier preview.
        cfg.start = Some(if date_in_excel { "2024-03-01" } else { "2024-02-02" }.into());
        assert!(stage.convert_wide(&cfg, cancel.clone(), |_| {}).is_err());
        assert_eq!(stage.report_page(0, 1).unwrap()[0].id, previous.id);
    }

    let mut data = rows();
    data[0][1] = "";
    data[0][3] = "2024-03-01";
    book(&path, &data, MERGES, sheet);
    let mut stage = area.ingest(&path, Limits::default(), &cancel, |_| {}).unwrap();
    let cfg = WideOptions { kind: "month".into(), start: Some("2024-02-01".into()), ..Default::default() };
    assert!(stage.convert_wide(&cfg, cancel, |_| {}).is_err());
}

#[test]
fn overlapping_periods_and_cancellation_leave_the_previous_conversion_intact() {
    let temp = tempfile::tempdir().unwrap();
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let path = temp.path().join("overlap.xlsx");
    let cancel = Arc::new(AtomicBool::new(false));
    for (first, second, overlaps) in [
        ("2024-02-26", "2024-03-01", true),
        ("2024-02-26", "2024-03-04", false),
    ] {
        let mut data = rows();
        data[0][1] = first;
        data[0][3] = second;
        book(&path, &data, MERGES, "xl/worksheets/sheet1.xml");
        let mut stage = area.ingest(&path, Limits::default(), &cancel, |_| {}).unwrap();
        stage.convert_wide(&WideOptions::default(), cancel.clone(), |_| {}).unwrap();
        let cfg = WideOptions { kind: "week".into(), ..Default::default() };
        let result = stage.convert_wide(&cfg, cancel.clone(), |progress| {
            if !overlaps && progress.reports == 1 { cancel.store(true, Ordering::Release); }
        });
        cancel.store(false, Ordering::Release);
        assert!(result.is_err());
        let reports = stage.report_page(0, 10).unwrap();
        assert_eq!(reports.len(), 2);
        assert!(reports.iter().all(|report| report.kind == "day" && report.start == report.end));
    }
}

#[test]
fn month_totals_remain_controls_and_blank_pairs_are_not_zeroes() {
    let temp = tempfile::tempdir().unwrap();
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let file = temp.path().join("monthly-controls.xlsx");
    let cancel = Arc::new(AtomicBool::new(false));
    let mut data = rows();
    data[0][1] = "2024-02";
    data[0][3] = "2024-03";
    for row in &mut data[2..] {
        row[1] = "0";
        row[2] = "0";
        row[3] = "";
        row[4] = "";
    }
    book(&file, &data, MERGES, "xl/worksheets/sheet1.xml");
    let mut stage = area.ingest(&file, Limits::default(), &cancel, |_| {}).unwrap();
    let cfg = WideOptions { kind: "month".into(), ..Default::default() };
    let converted = stage.convert_wide(&cfg, cancel, |_| {}).unwrap();
    assert_eq!(converted.reports, 1);
    let report = stage.report_page(0, 10).unwrap().remove(0);
    assert_eq!((report.start.as_str(), report.end.as_str()), ("2024-02-01", "2024-02-29"));
    assert_eq!((report.count, report.amount), (0.0, 0.0));
    let issues = stage.source_issues("Data", 0, 100).unwrap();
    assert!(issues.iter().any(|i| i.kind == "missing-period" && i.date.as_deref() == Some("2024-03-01")));
    assert!(issues.iter().any(|i| i.kind == "mismatch" && i.count_cell.as_deref() == Some("F4")));
}

#[test]
fn named_month_cannot_be_replaced_by_a_manual_day_or_week() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("month.xlsx");
    let mut data: Vec<_> = rows().into_iter().map(|row| row[..3].to_vec()).collect();
    data[0][1] = "Март 2026";
    book(&path, &data, "", "xl/worksheets/sheet1.xml");
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut stage = area.ingest(&path, Limits::default(), &cancel, |_| {}).unwrap();
    let mut cfg = WideOptions { kind: "month".into(), ..Default::default() };
    assert_eq!(stage.convert_wide(&cfg, cancel.clone(), |_| {}).unwrap().reports, 1);

    for kind in ["day", "week"] {
        cfg.kind = kind.into();
        cfg.start = Some("2026-03-02".into());
        assert!(stage.convert_wide(&cfg, cancel.clone(), |_| {}).is_err(), "{kind}");
        let previous = stage.report_page(0, 1).unwrap().remove(0);
        assert_eq!((previous.kind.as_str(), previous.start.as_str(), previous.end.as_str()),
                   ("month", "2026-03-01", "2026-03-31"));
    }
}

#[test]
fn split_day_and_month_keep_the_date_under_a_merged_month_heading() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("split.xlsx");
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));

    for (month, day_row) in [("Март 2026", vec!["", "2"]), ("2026-03", vec!["", "", "2"])] {
        let mut data: Vec<_> = rows().into_iter().map(|row| row[..3].to_vec()).collect();
        data[0][1] = month;
        data.insert(1, day_row);
        book(&path, &data, r#"<mergeCells><mergeCell ref="B1:C1"/></mergeCells>"#,
             "xl/worksheets/sheet1.xml");
        let mut stage = area.ingest(&path, Limits::default(), &cancel, |_| {}).unwrap();
        let mut cfg = WideOptions::default();
        for (kind, last_day) in [("day", "2026-03-02"), ("week", "2026-03-08")] {
            cfg.kind = kind.into();
            assert_eq!(stage.convert_wide(&cfg, cancel.clone(), |_| {}).unwrap().reports, 1);
            let report = stage.report_page(0, 1).unwrap().remove(0);
            assert_eq!((report.kind.as_str(), report.start.as_str(), report.end.as_str()),
                       (kind, "2026-03-02", last_day));
        }
        cfg.kind = "month".into();
        assert!(stage.convert_wide(&cfg, cancel.clone(), |_| {}).is_err());
        assert_eq!(stage.report_page(0, 1).unwrap()[0].start, "2026-03-02");
    }
}

#[test]
fn week_range_cannot_be_shortened_or_silently_accepted_as_a_month() {
    let temp = tempfile::tempdir().unwrap();
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let path = temp.path().join("range.xlsx");
    let cancel = Arc::new(AtomicBool::new(false));
    for second in ["с 04.03.2024 по 10.03.2024", "с 04.03.2024 по 08.03.2024"] {
        let mut data = rows();
        data[0][1] = "с 26.02.2024 по 03.03.2024";
        data[0][3] = second;
        book(&path, &data, MERGES, "xl/worksheets/sheet1.xml");
        let mut stage = area.ingest(&path, Limits::default(), &cancel, |_| {}).unwrap();
        let cfg = WideOptions { kind: "week".into(), ..Default::default() };
        let result = stage.convert_wide(&cfg, cancel.clone(), |_| {});
        if second.contains("10.03") {
            assert_eq!(result.unwrap().reports, 2);
            let first = stage.report_page(0, 1).unwrap().remove(0);
            assert_eq!((first.start.as_str(), first.end.as_str()), ("2024-02-26", "2024-03-03"));
        } else {
            assert!(result.is_err());
        }
        let cfg = WideOptions { kind: "month".into(), ..Default::default() };
        assert!(stage.convert_wide(&cfg, cancel.clone(), |_| {}).is_err());
    }
}

#[test]
fn classic_auto_rule_requires_daily_evidence_and_accepts_late_headers() {
    let temp = tempfile::tempdir().unwrap();
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let path = temp.path().join("calendar.xlsx");
    let component = "xl/worksheets/sheet1.xml";
    let cancel = AtomicBool::new(false);
    let organizations = WideOptions::default().organizations;
    for (next, classic) in [("2024-02-29", true), ("2024-03-06", false)] {
        let mut data = vec![vec!["Служебный заголовок"]; 17];
        let mut metrics = rows();
        metrics[0][3] = next;
        metrics[1][0] = "Работа";
        data.extend(metrics);
        book(&path, &data, "", component);
        let stage = area.ingest(&path, Limits::default(), &cancel, |_| {}).unwrap();
        assert_eq!(stage.worksheet_layout(component).unwrap(), Some("daily-wide"));
        assert_eq!(stage.classic_daily_workbook(&stage.sheet_names().unwrap(), &organizations, &cancel).unwrap(), classic);
    }
}

#[test]
fn dated_quantity_only_columns_are_not_treated_as_amount_columns() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("quantities.xlsx");
    let rows = vec![
        vec!["Подразделение", "05.01.2026", "06.01.2026", "07.01.2026", "Итого"],
        vec!["Работа, № по каталогу", "Количество", "Количество", "Количество", "Количество"],
        vec!["СМУ16", "1", "2", "3", "6"],
        vec!["Работа A", "1", "2", "3", "6"],
    ];
    book(&path, &rows, "", "xl/worksheets/sheet1.xml");
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut stage = area.ingest(&path, Limits::default(), &cancel, |_| {}).unwrap();
    let sheet = stage.sheet_names().unwrap().pop().unwrap().1;
    // Format discovery must work before wide_setup has created date_styles.
    assert_eq!(stage.worksheet_layout(&sheet).unwrap(), Some("count-only"));
    let error = stage.convert_wide(&WideOptions::default(), cancel, |_| {}).unwrap_err().to_string();
    assert!(error.contains("только «Количество»"), "{error}");
    assert!(error.contains("Данные не сохранены"), "{error}");
}

#[test]
fn paired_quantity_and_amount_headers_keep_the_existing_daily_layout() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("paired.xlsx");
    book(&path, &rows(), MERGES, "xl/worksheets/sheet1.xml");
    let area = StageArea::open(temp.path().join("stage")).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let stage = area.ingest(&path, Limits::default(), &cancel, |_| {}).unwrap();
    let sheet = stage.sheet_names().unwrap().pop().unwrap().1;
    assert_eq!(stage.worksheet_layout(&sheet).unwrap(), Some("daily-wide"));
}
