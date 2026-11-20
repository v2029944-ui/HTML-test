use analiz_import_core::{LegacyOptions, Limits, Stage, StageArea, WideOptions};
use std::{
    collections::BTreeSet,
    io::Write,
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
    time::Instant,
};
fn output_reports(
    stage: &Stage<'_>,
    wide: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut output = std::io::stdout().lock();
    let mut after_report = 0;
    let mut sheets = BTreeSet::new();
    loop {
        let reports = stage.report_page(after_report, 100)?;
        if reports.is_empty() {
            break;
        }
        for report in reports {
            after_report = report.ordinal;
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
            let mut after = 0;
            loop {
                let totals = stage.report_totals(&report.id, after, 100)?;
                if totals.is_empty() {
                    break;
                }
                for total in totals {
                    after = total.sequence;
                    serde_json::to_writer(
                        &mut output,
                        &serde_json::json!({"record":"total","reportId":report.id,"data":total}),
                    )?;
                    writeln!(&mut output)?;
                }
            }
            for org in stage.report_organizations(&report.id)? {
                serde_json::to_writer(
                    &mut output,
                    &serde_json::json!({"record":"organization","reportId":report.id,"data":org}),
                )?;
                writeln!(&mut output)?;
            }
            if wide {
                if let Some(p) = stage.report_provenance(&report.id)? {
                    sheets.insert(p.sheet_name.clone());
                    serde_json::to_writer(
                        &mut output,
                        &serde_json::json!({"record":"provenance","reportId":report.id,"data":p}),
                    )?;
                    writeln!(&mut output)?;
                }
            }
        }
    }
    if wide {
        for sheet in sheets {
            let mut after = 0;
            loop {
                let issues = stage.source_issues(&sheet, after, 100)?;
                if issues.is_empty() {
                    break;
                }
                for issue in issues {
                    after = issue.sequence;
                    serde_json::to_writer(
                        &mut output,
                        &serde_json::json!({"record":"sourceIssue","data":issue}),
                    )?;
                    writeln!(&mut output)?;
                }
            }
        }
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let source =
        PathBuf::from(std::env::args().nth(1).ok_or(
            "usage: stage-xlsx file.xlsx [staging-root] [wide JSON|kind start component]",
        )?);
    let root = std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("analiz-xlsx-profile"));
    let start = Instant::now();
    let area = StageArea::open(root)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let mut stage = area.ingest(&source, Limits::default(), &cancel, |_| {})?;
    let ingest = start.elapsed();
    let read_peak = peak_kib();
    if let Some(kind) = std::env::args().nth(3) {
        let convert_start = Instant::now();
        let summary = if kind == "wide" {
            let raw = std::env::args().nth(4).unwrap_or_else(|| "{}".into());
            if raw.len() > 65536 {
                return Err("wide options exceed64KiB".into());
            }
            let cfg: WideOptions = serde_json::from_str(&raw)?;
            stage.convert_wide(&cfg, cancel.clone(), |_| {})?
        } else {
            let first = std::env::args()
                .nth(4)
                .ok_or("conversion requires start date")?;
            let sheet = std::env::args()
                .nth(5)
                .unwrap_or_else(|| "xl/worksheets/sheet1.xml".into());
            let cfg = LegacyOptions {
                period_dates: None,
                sheet,
                kind: kind.clone(),
                start: first,
                organizations: ["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
                mappings: None,
            };
            stage.convert_legacy(&cfg, &cancel, |_| {})?
        };
        eprintln!(
            "{}",
            serde_json::json!({"ingest_ms":ingest.as_millis(),"conversion_ms":convert_start.elapsed().as_millis(),"cells":stage.stats.cells,"reports":summary.reports,"rows":summary.rows,"issues":summary.issues,"stage_bytes":std::fs::metadata(stage.path().join("stage.sqlite3"))?.len(),"peak_after_read_kib":read_peak,"peak_after_convert_kib":peak_kib()})
        );
        if std::env::args().nth(5).as_deref() != Some("--summary-only") {
            output_reports(&stage, kind == "wide")?;
        }
        return Ok(());
    }
    println!("source_bytes={} cells={} empty_cells={} shared_strings={} unpacked_bytes={} elapsed_ms={} sha256={}",std::fs::metadata(source)?.len(),stage.stats.cells,stage.stats.empty_cells,stage.stats.strings,stage.stats.unpacked_bytes,start.elapsed().as_millis(),stage.source_hash);
    for (name, bytes) in &stage.stats.components {
        println!("component={name} bytes={bytes}");
    }
    Ok(())
}

fn peak_kib() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find(|s| s.starts_with("VmHWM:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
