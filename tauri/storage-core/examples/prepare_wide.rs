//! Bounded Linux backend probe and small-fixture document sink; never full-history load.
use analiz_import_core::{ImportMode, Limits, StageArea, WideOptions};
use analiz_storage_core::storage::{ImportConfirmation, ImportJob, ReadFilter, Storage};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
    time::Instant,
};
fn memory() -> String {
    fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .filter(|s| s.starts_with("VmRSS:") || s.starts_with("VmHWM:"))
        .collect::<Vec<_>>()
        .join(" ")
}
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let source = PathBuf::from(std::env::args().nth(1).ok_or("source XLSX")?);
    let root = PathBuf::from(std::env::args().nth(2).ok_or("fresh probe directory")?);
    let emit = std::env::args().nth(3).as_deref() == Some("emit");
    let today = std::env::args()
        .nth(4)
        .unwrap_or_else(|| "2026-11-17".into())
        .parse()?;
    let options: WideOptions = std::env::args()
        .nth(5)
        .map(|text| serde_json::from_str(&text))
        .transpose()?
        .unwrap_or_default();
    let area = StageArea::open(root.join("stage"))?;
    let cancel = Arc::new(AtomicBool::new(false));
    let started = Instant::now();
    let mut stage = area.ingest(&source, Limits::default(), &cancel, |_| {})?;
    stage.convert_wide_as_of(&options, cancel.clone(), today, |_| {})?;
    eprintln!("convert_ms={} {}", started.elapsed().as_millis(), memory());
    let started = Instant::now();
    let prepared = stage.prepare_with_mode(ImportMode::Folder, &cancel, |_| {})?;
    let disk = fs::read_dir(prepared.directory())?
        .try_fold(0u64, |sum, entry| -> std::io::Result<u64> {
            Ok(sum + entry?.metadata()?.len())
        })?;
    eprintln!(
        "prepare_ms={} stage_bytes={disk} {}",
        started.elapsed().as_millis(),
        memory()
    );
    if emit {
        fs::create_dir_all(root.join("documents"))?;
        let mut cursor = 0;
        while let Some(report) = prepared.next_report(cursor)? {
            cursor = report.ordinal;
            let mut file = fs::File::create(
                root.join("documents")
                    .join(format!("report-{}.json", report.ordinal)),
            )?;
            std::io::copy(&mut prepared.open_document(&report)?, &mut file)?;
            file.flush()?;
        }
    }
    let storage = Storage::new(root.join("live"))?;
    storage.revision()?;
    let started = Instant::now();
    let preview = storage.preview_import(&prepared)?;
    eprintln!("preview_ms={} {}", started.elapsed().as_millis(), memory());
    let started = Instant::now();
    let receipt = storage.apply_import(
        &prepared,
        &preview,
        ImportConfirmation {
            replace_source: true,
            accept_quality: true,
            ..Default::default()
        },
        &ImportJob::new(),
        |_| {},
    )?;
    eprintln!("apply_ms={} {}", started.elapsed().as_millis(), memory());
    let total = storage.aggregate(receipt.revision, &ReadFilter::default())?;
    assert_eq!(total.rows as u64, prepared.row_count());
    // Every emitted document has already been length/hash verified while applying.
    // A first-report backfill independently exercises legacy JSON streaming, including zero rows.
    if emit {
        let report = prepared.next_report(0)?.ok_or("missing first report")?;
        let conn = Connection::open(root.join("live/reports.sqlite3"))?;
        let rowid: i64 =
            conn.query_row("SELECT rowid FROM reports WHERE id=?1", [&report.id], |r| {
                r.get(0)
            })?;
        let mut blob = conn.blob_open("main", "reports", "json", rowid, true)?;
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let n = blob.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        drop(blob);
        assert_eq!(format!("{:x}", hash.finalize()), report.sha256);
        conn.execute(
            "DELETE FROM report_row_index WHERE report_id=?1",
            [&report.id],
        )?;
        conn.execute("DELETE FROM report_rows WHERE report_id=?1", [&report.id])?;
        assert_eq!(
            storage.index_report(&report.id, receipt.revision)? as u64,
            report.rows
        );
    }
    println!(
        "{}",
        serde_json::to_string(
            &serde_json::json!({"receipt":receipt,"quality":prepared.quality(),"source_diagnostics":prepared.source_diagnostics(),"page_one_source_issues":prepared.source_issues(0,1)?.len(),"stage_bytes":disk})
        )?
    );
    Ok(())
}
