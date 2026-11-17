use analiz_import_core::{LegacyOptions, Limits, StageArea};
use std::{path::PathBuf, sync::atomic::AtomicBool, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let source = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("usage: stage-xlsx file.xlsx [staging-root]")?,
    );
    let root = std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("analiz-xlsx-profile"));
    let start = Instant::now();
    let area = StageArea::open(root)?;
    let mut stage = area.ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {})?;
    if let Some(kind) = std::env::args().nth(3) {
        let start = std::env::args()
            .nth(4)
            .ok_or("conversion requires start date")?;
        let sheet = std::env::args()
            .nth(5)
            .unwrap_or_else(|| "xl/worksheets/sheet1.xml".into());
        let cfg = LegacyOptions {
            sheet,
            kind,
            start,
            organizations: ["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
            mappings: None,
        };
        stage.convert_legacy(&cfg, &AtomicBool::new(false), |_| {})?;
        let mut output = std::io::stdout().lock();
        use std::io::Write;
        let mut report_cursor = 0;
        loop {
            let reports = stage.report_page(report_cursor, 100)?;
            if reports.is_empty() {
                break;
            }
            for report in reports {
                serde_json::to_writer(
                    &mut output,
                    &serde_json::json!({"record":"report","data":report}),
                )?;
                writeln!(&mut output)?;
                let mut after = 0;
                loop {
                    let rows = stage.report_rows(&report.id, after, 100)?;
                    if rows.is_empty() {
                        break;
                    }
                    for row in rows {
                        after = row.row_index;
                        serde_json::to_writer(
                            &mut output,
                            &serde_json::json!({"record":"row","reportId":report.id,"data":row}),
                        )?;
                        writeln!(&mut output)?;
                    }
                }
                let mut after = 0;
                loop {
                    let issues = stage.report_issues(&report.id, after, 100)?;
                    if issues.is_empty() {
                        break;
                    }
                    for issue in issues {
                        after = issue.sequence;
                        serde_json::to_writer(
                            &mut output,
                            &serde_json::json!({"record":"issue","reportId":report.id,"data":issue}),
                        )?;
                        writeln!(&mut output)?;
                    }
                }
                report_cursor += 1;
            }
        }
        return Ok(());
    }
    println!("source_bytes={} cells={} empty_cells={} shared_strings={} unpacked_bytes={} elapsed_ms={} sha256={}",std::fs::metadata(source)?.len(),stage.stats.cells,stage.stats.empty_cells,stage.stats.strings,stage.stats.unpacked_bytes,start.elapsed().as_millis(),stage.source_hash);
    for (name, bytes) in &stage.stats.components {
        println!("component={name} bytes={bytes}");
    }
    Ok(())
}
