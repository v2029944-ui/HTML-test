use analiz_import_core::{Limits, StageArea};
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
    let stage = area.ingest(&source, Limits::default(), &AtomicBool::new(false), |_| {})?;
    println!("source_bytes={} cells={} empty_cells={} shared_strings={} unpacked_bytes={} elapsed_ms={} sha256={}",std::fs::metadata(source)?.len(),stage.stats.cells,stage.stats.empty_cells,stage.stats.strings,stage.stats.unpacked_bytes,start.elapsed().as_millis(),stage.source_hash);
    for (name, bytes) in &stage.stats.components {
        println!("component={name} bytes={bytes}");
    }
    Ok(())
}
