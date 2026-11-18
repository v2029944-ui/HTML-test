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
    let mut xml = String::from("<worksheet><sheetData>");
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
    for(name,content) in [(target,xml),("xl/workbook.xml","<workbook xmlns:r=\"r\"><sheets><sheet name=\"Data\" r:id=\"r1\"/></sheets></workbook>".into()),("xl/_rels/workbook.xml.rels",format!("<Relationships><Relationship Id=\"r1\" Target=\"/{}\" Type=\"worksheet\"/></Relationships>",target))]{z.start_file(name,SimpleFileOptions::default()).unwrap();z.write_all(content.as_bytes()).unwrap();}
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
