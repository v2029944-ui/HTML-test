use analiz_import_core::{Limits, StageArea};
use std::{fs, io::Write, sync::atomic::AtomicBool};
use zip::{write::SimpleFileOptions, ZipWriter};
fn book(path: &std::path::Path, sheets: &[(&str, Vec<Vec<&str>>)]) {
    let mut z = ZipWriter::new(fs::File::create(path).unwrap());
    let mut workbook=String::from("<workbook xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets>");
    let mut rels = String::from("<Relationships>");
    for (i, (name, rows)) in sheets.iter().enumerate() {
        workbook.push_str(&format!("<sheet name=\"{name}\" r:id=\"r{}\"/>", i + 1));
        rels.push_str(&format!(
            "<Relationship Id=\"r{}\" Target=\"worksheets/sheet{}.xml\"/>",
            i + 1,
            i + 1
        ));
        let mut xml = String::from("<worksheet><sheetData>");
        for (r, row) in rows.iter().enumerate() {
            xml.push_str(&format!("<row r=\"{}\">", r + 1));
            for (c, value) in row.iter().enumerate() {
                if !value.is_empty() {
                    xml.push_str(&format!(
                        "<c r=\"{}{}\" t=\"inlineStr\"><is><t>{value}</t></is></c>",
                        (b'A' + c as u8) as char,
                        r + 1
                    ));
                }
            }
            xml.push_str("</row>");
        }
        xml.push_str("</sheetData></worksheet>");
        z.start_file(
            format!("xl/worksheets/sheet{}.xml", i + 1),
            SimpleFileOptions::default(),
        )
        .unwrap();
        z.write_all(xml.as_bytes()).unwrap();
    }
    workbook.push_str("</sheets></workbook>");
    rels.push_str("</Relationships>");
    z.start_file("xl/workbook.xml", SimpleFileOptions::default())
        .unwrap();
    z.write_all(workbook.as_bytes()).unwrap();
    z.start_file("xl/_rels/workbook.xml.rels", SimpleFileOptions::default())
        .unwrap();
    z.write_all(rels.as_bytes()).unwrap();
    z.finish().unwrap();
}
#[test]
fn catalog_detects_four_layouts_skips_reports_and_deduplicates() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("catalog.xlsx");
    book(
        &path,
        &[
            (
                "Таблица",
                vec![
                    vec!["Подразделение", "Услуга"],
                    vec!["СМУ16", "Ремонт блока"],
                    vec!["", "Замена колеса"],
                ],
            ),
            (
                "Колонки",
                vec![
                    vec!["СМУ16", "АВТО"],
                    vec!["Ремонт блока", "Ремонт двигателя"],
                    vec!["Всего", "Итого"],
                ],
            ),
            (
                "Блоки",
                vec![vec!["ТРАК"], vec!["Ремонт кабины"], vec!["Замена масла"]],
            ),
            (
                "ЦЕНТР",
                vec![
                    vec!["Список услуг"],
                    vec!["Ремонт насоса"],
                    vec!["Ремонт моста"],
                ],
            ),
            (
                "Отчет",
                vec![
                    vec!["Работа", "Количество", "Сумма"],
                    vec!["СМУ16", "1", "100"],
                    vec!["НЕ справочник", "1", "100"],
                ],
            ),
        ],
    );
    let area = StageArea::open(tmp.path().join("stages")).unwrap();
    let cancel = AtomicBool::new(false);
    let stage = area
        .ingest(&path, Limits::default(), &cancel, |_| {})
        .unwrap();
    let mut found = Vec::new();
    let count = stage
        .visit_catalog(
            &["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
            &cancel,
            |org, work| {
                found.push((org.to_string(), work.to_string()));
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(count, 7);
    assert_eq!(found.len(), 7);
    assert!(!found.iter().any(|(_, work)| work == "НЕ справочник"));
    cancel.store(true, std::sync::atomic::Ordering::Release);
    assert!(stage
        .visit_catalog(
            &["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
            &cancel,
            |_, _| Ok(())
        )
        .is_err());
}
