use analiz_import_core::{Limits, StageArea};
use std::{fs, io::Write, sync::atomic::AtomicBool};
use zip::{write::SimpleFileOptions, ZipWriter};
fn book(path: &std::path::Path, xml: &str) {
    let mut z = ZipWriter::new(fs::File::create(path).unwrap());
    z.start_file("xl/worksheets/sheet1.xml", SimpleFileOptions::default())
        .unwrap();
    z.write_all(xml.as_bytes()).unwrap();
    z.finish().unwrap();
}
#[test]
fn stage_is_bounded_paged_and_never_touches_live_database() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("book.xlsx");
    book(
        &source,
        r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Работа</t></is></c><c r="B1"><v>0</v></c><c r="C1" s="2"/></row></sheetData></worksheet>"#,
    );
    let area = StageArea::open(tmp.path().join("staging")).unwrap();
    let stage = area
        .ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {})
        .unwrap();
    assert_eq!(stage.stats().cells, 3);
    assert_eq!(stage.stats().empty_cells, 1);
    assert_eq!(
        stage.page("xl/worksheets/sheet1.xml", 0, 2).unwrap().len(),
        2
    );
    assert!(stage.page("xl/worksheets/sheet1.xml", 0, 1001).is_err());
    assert_eq!(
        stage.page("xl/worksheets/sheet1.xml", 0, 2).unwrap()[0].value,
        "Работа"
    );
    assert!(!tmp.path().join("reports.sqlite3").exists());
    let dir = stage.path().to_owned();
    drop(stage);
    assert!(!dir.exists());
}
#[test]
fn cancellation_and_invalid_xml_clean_staging() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("book.xlsx");
    let area = StageArea::open(tmp.path().join("staging")).unwrap();
    for xml in [
        "<worksheet><c r=\"A1\"></worksheet>",
        "<!DOCTYPE x [<!ENTITY e SYSTEM 'https://invalid/'>]><worksheet/>",
    ] {
        book(&source, xml);
        assert!(area
            .ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {})
            .is_err());
    }
    book(&source, "<worksheet/>");
    assert!(area
        .ingest(&source, Limits::default(), &AtomicBool::new(true), |_| {})
        .is_err());
    assert_eq!(fs::read_dir(tmp.path().join("staging")).unwrap().count(), 1);
}
#[test]
fn no_sparse_allocation_and_no_unbounded_token() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("book.xlsx");
    let area = StageArea::open(tmp.path().join("staging")).unwrap();
    for xml in [
        "<worksheet><c r=\"ZZZZ9999999999\"/></worksheet>".to_owned(),
        format!("<worksheet><t>{}</t></worksheet>", "x".repeat(2_000_000)),
    ] {
        book(&source, &xml);
        assert!(area
            .ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {})
            .is_err());
    }
}
#[test]
fn shared_strings_live_on_disk_and_indices_must_exist() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("shared.xlsx");
    let area = StageArea::open(tmp.path().join("staging")).unwrap();
    for index in ["0", "9999999999999999999999999", "9", "-1"] {
        let mut z = ZipWriter::new(fs::File::create(&source).unwrap());
        for (name, xml) in [
            (
                "xl/sharedStrings.xml",
                "<sst><si><t>A &amp; B</t></si></sst>".to_owned(),
            ),
            (
                "xl/worksheets/sheet1.xml",
                format!("<worksheet><c r=\"A1\" t=\"s\"><v>{index}</v></c></worksheet>"),
            ),
        ] {
            z.start_file(name, SimpleFileOptions::default()).unwrap();
            z.write_all(xml.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        let result = area.ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {});
        if index == "0" {
            let stage = result.unwrap();
            assert_eq!(
                stage.page("xl/worksheets/sheet1.xml", 0, 1).unwrap()[0].value,
                "A & B"
            );
        } else {
            assert!(result.is_err());
        }
    }
}
#[test]
fn actual_bytes_ratio_duplicate_cells_and_paths_are_guarded() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("bomb.xlsx");
    let area = StageArea::open(tmp.path().join("staging")).unwrap();
    book(&source, "<worksheet><c r=\"A1\"/><c r=\"A1\"/></worksheet>");
    assert!(area
        .ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {})
        .is_err());
    book(
        &source,
        &format!("<worksheet>{}</worksheet>", " ".repeat(100000)),
    );
    let mut limits = Limits::default();
    limits.component_bytes = 1000;
    assert!(area
        .ingest(&source, limits, &AtomicBool::new(false), |_| {})
        .is_err());
    limits = Limits::default();
    limits.ratio = 1;
    assert!(area
        .ingest(&source, limits, &AtomicBool::new(false), |_| {})
        .is_err());
    let mut z = ZipWriter::new(fs::File::create(&source).unwrap());
    z.start_file("../outside.xml", SimpleFileOptions::default())
        .unwrap();
    z.write_all(b"<x/>").unwrap();
    z.finish().unwrap();
    assert!(area
        .ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {})
        .is_err());
}
#[test]
fn recovery_requires_exclusive_manager_and_cleans_only_owned_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("staging");
    let area = StageArea::open(root.clone()).unwrap();
    assert!(StageArea::open(root.clone()).is_err());
    fs::create_dir(root.join("import-crashed")).unwrap();
    fs::write(root.join("import-crashed/partial"), "partial").unwrap();
    fs::write(root.join("keep"), "keep").unwrap();
    drop(area);
    let _recovered = StageArea::open(root.clone()).unwrap();
    assert!(!root.join("import-crashed").exists());
    assert!(root.join("keep").exists());
}
#[test]
fn xml_requires_one_root_and_defined_entities_everywhere() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("bad.xlsx");
    let area = StageArea::open(tmp.path().join("staging")).unwrap();
    for xml in [
        "",
        "<worksheet/><worksheet/>",
        "<worksheet/>junk",
        "<worksheet>&bogus;</worksheet>",
        "<worksheet><!-- bad -- comment --></worksheet>",
    ] {
        book(&source, xml);
        assert!(
            area.ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {})
                .is_err(),
            "accepted {xml}"
        );
    }
}
#[test]
fn mid_import_cancellation_and_changed_source_remove_all_partial_data() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.xlsx");
    let area = StageArea::open(tmp.path().join("staging")).unwrap();
    let xml = format!(
        "<worksheet>{}</worksheet>",
        (1..=5000)
            .map(|r| format!("<c r=\"A{r}\"><v>1</v></c>"))
            .collect::<String>()
    );
    book(&source, &xml);
    let cancel = AtomicBool::new(false);
    assert!(area
        .ingest(&source, Limits::default(), &cancel, |_| {
            cancel.store(true, std::sync::atomic::Ordering::Release);
        })
        .is_err());
    book(&source, &xml);
    let mut changed = false;
    assert!(area
        .ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {
            if !changed {
                fs::write(&source, b"replaced").unwrap();
                changed = true;
            }
        })
        .is_err());
    assert_eq!(fs::read_dir(tmp.path().join("staging")).unwrap().count(), 1);
}
#[test]
fn sqlite_full_is_an_error_and_partial_stage_is_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.xlsx");
    let area = StageArea::open(tmp.path().join("staging")).unwrap();
    let xml = format!(
        "<worksheet>{}</worksheet>",
        (1..=10000)
            .map(|r| format!("<c r=\"A{r}\"><v>1</v></c>"))
            .collect::<String>()
    );
    book(&source, &xml);
    let mut limits = Limits::default();
    limits.stage_bytes = 65536;
    assert!(area
        .ingest(&source, limits, &AtomicBool::new(false), |_| {})
        .is_err());
    assert_eq!(fs::read_dir(tmp.path().join("staging")).unwrap().count(), 1);
}
#[test]
fn zip64_markers_are_rejected_before_archive_construction() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("zip64.xlsx");
    let area = StageArea::open(tmp.path().join("staging")).unwrap();
    for field in [12usize, 16] {
        book(&source, "<worksheet/>");
        let mut bytes = fs::read(&source).unwrap();
        let eocd = bytes.len() - 22;
        bytes[eocd + field..eocd + field + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        fs::write(&source, bytes).unwrap();
        let error = area
            .ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {})
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains("ZIP64"),
            "preflight should reject ZIP64, got {error}"
        );
    }
}
#[test]
fn preview_cursor_covers_row_boundaries_without_dropping_cells() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("page.xlsx");
    let area = StageArea::open(tmp.path().join("staging")).unwrap();
    book(&source,"<worksheet><c r=\"A1\"><v>1</v></c><c r=\"B1\"><v>2</v></c><c r=\"A2\"><v>3</v></c></worksheet>");
    let stage = area
        .ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {})
        .unwrap();
    let p = stage.page("xl/worksheets/sheet1.xml", 1, 2).unwrap();
    assert_eq!(
        p.iter().map(|c| c.address.as_str()).collect::<Vec<_>>(),
        ["B1", "A2"]
    );
    assert_eq!(
        stage.page("xl/worksheets/sheet1.xml", 2048, 1).unwrap()[0].address,
        "A2"
    );
    assert!(stage.page("xl/worksheets/sheet1.xml", 0, 101).is_err());
}
#[test]
fn a_forged_terminal_footer_must_not_trigger_library_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("fallback.xlsx");
    let area = StageArea::open(tmp.path().join("staging")).unwrap();
    let mut z = ZipWriter::new(fs::File::create(&source).unwrap());
    for n in 0..5000 {
        z.start_file(format!("{n}.bin"), SimpleFileOptions::default())
            .unwrap();
    }
    z.finish().unwrap();
    let mut footer = [0u8; 22];
    footer[..4].copy_from_slice(b"PK\x05\x06");
    footer[8..10].copy_from_slice(&1u16.to_le_bytes());
    footer[10..12].copy_from_slice(&1u16.to_le_bytes());
    footer[16..20].copy_from_slice(&0xfffffffeu32.to_le_bytes());
    fs::OpenOptions::new()
        .append(true)
        .open(&source)
        .unwrap()
        .write_all(&footer)
        .unwrap();
    let error = area
        .ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {})
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("ZIP preflight"),
        "must reject before library metadata parsing: {error}"
    );
}
#[test]
fn crc_and_actual_decompressed_size_must_match_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("corrupt.xlsx");
    let area = StageArea::open(tmp.path().join("staging")).unwrap();
    book(&source, "<worksheet/>");
    let mut bytes = fs::read(&source).unwrap();
    let central = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
    // Keep local/central lengths mutually consistent, but lie about actual output.
    bytes[22..26].copy_from_slice(&1u32.to_le_bytes());
    bytes[central + 24..central + 28].copy_from_slice(&1u32.to_le_bytes());
    fs::write(&source, bytes).unwrap();
    assert!(area
        .ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {})
        .is_err());
}
#[test]
fn descriptor_archives_are_supported_but_bad_descriptors_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("descriptor.xlsx");
    let area = StageArea::open(tmp.path().join("staging")).unwrap();
    for corrupt in [false, true] {
        book(&source, "<worksheet><c r=\"A1\"><v>1</v></c></worksheet>");
        let mut bytes = fs::read(&source).unwrap();
        let central = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
        let flags = u16::from_le_bytes(bytes[6..8].try_into().unwrap()) | 8;
        bytes[6..8].copy_from_slice(&flags.to_le_bytes());
        bytes[central + 8..central + 10].copy_from_slice(&flags.to_le_bytes());
        let mut descriptor = b"PK\x07\x08".to_vec();
        descriptor.extend_from_slice(&bytes[central + 16..central + 28]);
        if corrupt {
            descriptor[4] ^= 1;
        }
        bytes[14..26].fill(0);
        bytes.splice(central..central, descriptor);
        let eocd = bytes.len() - 22;
        bytes[eocd + 16..eocd + 20].copy_from_slice(&((central + 16) as u32).to_le_bytes());
        fs::write(&source, bytes).unwrap();
        let result = area.ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {});
        if corrupt {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap().stats().cells, 1);
        }
    }
}
#[test]
fn undefined_namespace_prefixes_in_cells_and_elements_are_not_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("ns.xlsx");
    let area = StageArea::open(tmp.path().join("stage")).unwrap();
    for xml in [
        "<x:worksheet/>",
        "<worksheet><c r=\"A1\" bad:q=\"1\"><v>0</v></c></worksheet>",
    ] {
        book(&source, xml);
        assert!(
            area.ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {})
                .is_err(),
            "accepted malformed namespace {xml}"
        );
    }
}

#[test]
fn expanded_native_limits_keep_anti_bomb_guards_and_support_older_row_gap() {
    let l=Limits::default();
    assert_eq!(l.archive_bytes,analiz_import_core::NATIVE_XLSX_ARCHIVE_LIMIT);
    assert_eq!(l.archive_bytes,100*1024*1024);
    assert_eq!(l.component_bytes,128*1024*1024);
    assert_eq!(l.total_bytes,768*1024*1024);
    assert_eq!(l.stage_bytes,4*1024*1024*1024);
    assert_eq!(l.cells,15_000_000);
    assert_eq!(l.rows,100_000);
    assert_eq!(l.columns,2048);
    assert_eq!(l.ratio,1000);
    assert_eq!(l.token_bytes,1024*1024);

    let tmp=tempfile::tempdir().unwrap();
    let source=tmp.path().join("large-row.xlsx");
    let area=StageArea::open(tmp.path().join("stage")).unwrap();
    book(&source,"<worksheet><sheetData><row r=\"60001\"><c r=\"A60001\"><v>42</v></c></row></sheetData></worksheet>");
    let result=area.ingest(&source,l,&AtomicBool::new(false),|_|{}).unwrap();
    assert_eq!(result.stats().cells,1);
    drop(result);
    book(&source,"<worksheet><sheetData><row r=\"100001\"><c r=\"A100001\"><v>1</v></c></row></sheetData></worksheet>");
    assert!(area.ingest(&source,l,&AtomicBool::new(false),|_|{}).is_err());
    let mut constrained=l;
    constrained.cells=1;
    book(&source,"<worksheet><c r=\"A1\"><v>1</v></c><c r=\"B1\"><v>2</v></c></worksheet>");
    assert!(area.ingest(&source,constrained,&AtomicBool::new(false),|_|{}).is_err());
    let large_source=tmp.path().join("over-archive-limit.xlsx");
    let f=fs::File::create(&large_source).unwrap();
    f.set_len(analiz_import_core::NATIVE_XLSX_ARCHIVE_LIMIT+1).unwrap();
    let error=area.ingest(&large_source,l,&AtomicBool::new(false),|_|{})
        .err().unwrap().to_string();
    assert!(error.contains("XLSX archive byte limit"),"unexpected error: {error}");
    assert!(!tmp.path().join("reports.sqlite3").exists());
}
