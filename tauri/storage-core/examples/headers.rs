//! Probe first-open bounded header rejection independently of fixture construction.
use analiz_storage_core::storage::{ReadFilter, Storage};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 2, "usage: headers ROOT");
    let store = Storage::new((&args[1]).into()).unwrap();
    let revision = store.revision().unwrap();
    let filter = ReadFilter {
        report_type: Some("day".into()),
        start: Some("2027-01-01".into()),
        ..ReadFilter::default()
    };
    let errors = [
        store
            .report_page(revision, &ReadFilter::default(), None, 1)
            .unwrap_err(),
        store
            .report_page(revision, &filter, Some(("2026-01-01", "cursor")), 1)
            .unwrap_err(),
        store
            .row_page(revision, &filter, Some(("cursor", 0)), 1)
            .unwrap_err(),
        store.aggregate(revision, &filter).unwrap_err(),
        store.index_report("r1", revision).unwrap_err(),
    ];
    for error in errors {
        assert!(error.contains("64 KiB"), "{error}");
    }
    assert_eq!(store.revision().unwrap(), revision);
    println!("revision={revision} five_selective_calls_rejected_before_filters_and_key_lookups");
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status
            .lines()
            .filter(|line| line.starts_with("VmHWM:") || line.starts_with("VmRSS:"))
        {
            println!("{line}");
        }
        let hwm = status
            .lines()
            .find(|line| line.starts_with("VmHWM:"))
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert!(
            hwm < 32 * 1024,
            "header fixture rejection exceeded 32 MiB RSS: {hwm} KiB"
        );
    }
}
