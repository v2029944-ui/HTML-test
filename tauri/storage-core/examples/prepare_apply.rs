//! Fixture creation is a separate Python process. This worker never constructs report-sized JSON.
use analiz_import_core::{LegacyOptions, Limits, StageArea, TripleMapping};
use analiz_storage_core::storage::{ImportConfirmation, ImportJob, ReadFilter, Storage};
use std::{path::PathBuf, sync::atomic::AtomicBool, time::Instant};
fn memory() -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("VmHWM:") || l.starts_with("VmRSS:"))
        .collect::<Vec<_>>()
        .join(" ")
}
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("fixture directory")?);
    let phase = std::env::args().nth(2).unwrap_or_else(|| "apply".into());
    let area = StageArea::open(root.join("stage"))?;
    let cancel = AtomicBool::new(false);
    let before = Instant::now();
    let mut stage = area.ingest(
        &root.join("source.xlsx"),
        Limits::default(),
        &cancel,
        |_| {},
    )?;
    stage.convert_legacy(
        &LegacyOptions {
            period_dates: None,
            sheet: "xl/worksheets/sheet1.xml".into(),
            kind: "day".into(),
            start: "2026-11-01".into(),
            organizations: ["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
            mappings: Some(vec![TripleMapping {
                header_row: 1,
                work_col: 1,
                count_col: 2,
                amount_col: 3,
            }]),
        },
        &cancel,
        |_| {},
    )?;
    println!(
        "ingest_convert_ms={} {}",
        before.elapsed().as_millis(),
        memory()
    );
    let before = Instant::now();
    let prepared = stage.prepare(&cancel, |_| {})?;
    let report = prepared.next_report(0)?.ok_or("missing report")?;
    println!(
        "prepare_ms={} document_bytes={} rows={} {}",
        before.elapsed().as_millis(),
        report.bytes,
        report.rows,
        memory()
    );
    if phase == "prepare" {
        return Ok(());
    }
    let storage = Storage::new(root.join("live"))?;
    storage.revision()?;
    let preview_started = Instant::now();
    let preview = storage.preview_import(&prepared)?;
    println!(
        "preview_ms={} {}",
        preview_started.elapsed().as_millis(),
        memory()
    );
    let apply_started = Instant::now();
    let mut lock_before_commit = 0;
    let receipt = storage.apply_import(
        &prepared,
        &preview,
        ImportConfirmation::default(),
        &ImportJob::new(),
        |event| {
            if event.phase == "before_commit" {
                lock_before_commit = event.elapsed_millis;
            }
        },
    )?;
    let elapsed = apply_started.elapsed().as_millis();
    let total = storage.aggregate(receipt.revision, &ReadFilter::default())?;
    assert_eq!(total.rows as u64, report.rows);
    assert_eq!(total.count, report.rows as f64);
    assert_eq!(total.amount, report.rows as f64 * 2.0);
    assert_eq!(
        storage
            .row_page(receipt.revision, &ReadFilter::default(), None, 1)?
            .len(),
        1
    );
    println!("apply_ms={elapsed} lock_before_commit_ms={lock_before_commit} total_rows={} total_amount={} {}",total.rows,total.amount,memory());
    Ok(())
}
