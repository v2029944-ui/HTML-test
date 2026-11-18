//! Probe backfill RSS independently from fixture construction.
use analiz_storage_core::storage::{ReadFilter, Storage};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 3, "usage: backfill ROOT REPORT_ID");
    let store = Storage::new((&args[1]).into()).unwrap();
    let revision = store.revision().unwrap();
    let rows = store.index_report(&args[2], revision).unwrap();
    let total = store.aggregate(revision, &ReadFilter::default()).unwrap();
    println!(
        "revision={revision} indexed_rows={rows} aggregate_rows={} amount={}",
        total.rows, total.amount
    );
}
