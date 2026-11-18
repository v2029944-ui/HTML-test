//! Linux/local cancellation probe, not production IPC or a latency guarantee.
use analiz_import_core::{Limits, StageArea, WideOptions};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

fn request(cancel: Arc<AtomicBool>) -> mpsc::Receiver<Instant> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        let at = Instant::now();
        cancel.store(true, Ordering::Release);
        let _ = tx.send(at);
    });
    rx
}
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        return Err("cancel_wide source staging-root read|conversion".into());
    }
    let root = PathBuf::from(&args[2]);
    let area = StageArea::open(root.clone())?;
    let cancel = Arc::new(AtomicBool::new(false));
    let (rx, error) = match args[3].as_str() {
        "read" => {
            let rx = request(cancel.clone());
            let error = match area.ingest(
                PathBuf::from(&args[1]).as_path(),
                Limits::default(),
                &cancel,
                |_| {},
            ) {
                Ok(_) => {
                    return Err("read completed before cancellation; increase fixture size".into())
                }
                Err(e) => e.to_string(),
            };
            (rx, error)
        }
        "conversion" => {
            let mut stage = area.ingest(
                PathBuf::from(&args[1]).as_path(),
                Limits::default(),
                &cancel,
                |_| {},
            )?;
            let rx = request(cancel.clone());
            let error = match stage.convert_wide(&WideOptions::default(), cancel.clone(), |_| {}) {
                Ok(_) => {
                    return Err(
                        "conversion completed before cancellation; increase fixture size".into(),
                    )
                }
                Err(e) => e.to_string(),
            };
            drop(stage);
            (rx, error)
        }
        _ => return Err("invalid phase".into()),
    };
    let requested = rx.recv()?;
    let response_us = requested.elapsed().as_micros();
    drop(area);
    if !cancel.load(Ordering::Acquire) || !(error.contains("cancel") || error.contains("interrupt"))
    {
        return Err(format!("unexpected cancellation result: {error}").into());
    }
    let leftovers = std::fs::read_dir(&root)?
        .filter_map(Result::ok)
        .filter(|x| x.file_name().to_string_lossy().starts_with("import-"))
        .count();
    if leftovers != 0 {
        return Err("private operation directories remain".into());
    }
    println!(
        "{}",
        serde_json::json!({"phase":args[3],"request_after_ms":30,"response_and_stage_cleanup_us":response_us,"cleanup_checked_us":requested.elapsed().as_micros(),"leftovers":leftovers,"error":error})
    );
    Ok(())
}
