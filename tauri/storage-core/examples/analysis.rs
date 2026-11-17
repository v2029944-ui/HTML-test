//! Runs independently after fixture generation. Linux high-water RSS is in-child.
use analiz_storage_core::storage::*;
use std::{fs, path::PathBuf, time::Instant};
fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("analysis ROOT"));
    let input: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("probe.json")).unwrap()).unwrap();
    let started = Instant::now();
    let store = Storage::new(root.clone()).unwrap();
    let rev = store.revision().unwrap();
    let queries_only = std::env::args().nth(2).as_deref() == Some("--queries-only");
    let mut cursor = None;
    let mut reports = 0;
    if !queries_only {
        loop {
            let page = store
                .report_page(
                    rev,
                    &ReadFilter::default(),
                    cursor
                        .as_ref()
                        .map(|(d, id): &(String, String)| (d.as_str(), id.as_str())),
                    100,
                )
                .unwrap();
            if page.is_empty() {
                break;
            }
            for r in &page {
                store.analysis_index_report(&r.id, rev).unwrap();
                reports += 1;
            }
            let r = page.last().unwrap();
            cursor = Some((r.period_start.clone(), r.id.clone()));
        }
        store.analysis_index_settings(rev).unwrap();
    }
    let filter = store.analysis_filter_create(rev, "organizations").unwrap();
    store
        .analysis_filter_append(rev, &filter, &["A".into(), "B".into()])
        .unwrap();
    store.analysis_filter_seal(rev, &filter).unwrap();
    let scope = AnalysisScope {
        report_type: "day".into(),
        organizations: filter,
        names: None,
    };
    let q = RangeQuery {
        from: input["from"].as_str().unwrap().into(),
        to: input["to"].as_str().unwrap().into(),
    };
    let range = store.analysis_range_summary(rev, &scope, &q).unwrap();
    assert!(range.complete);
    let n = input["rows"].as_i64().unwrap();
    let h = input["history"].as_i64().unwrap();
    assert_eq!(range.values, n);
    assert_eq!(range.total.amount, Some((2 * n + h) as f64));
    assert_eq!(
        store
            .analysis_range_page(rev, &scope, &q, None, None, 1)
            .unwrap()
            .len(),
        1
    );
    let changes = ChangeQuery {
        method: "previous".into(),
        from: q.to.clone(),
        to: q.to.clone(),
        percent: 0.,
        min_count: 0.,
        min_amount: 0.,
    };
    let totals = store
        .analysis_changes_summary(rev, &scope, &changes)
        .unwrap();
    assert_eq!(totals.rows, n);
    assert_eq!(totals.amount.current, (2 * n) as f64);
    assert_eq!(
        store
            .analysis_changes_page(rev, &scope, &changes, None, 1)
            .unwrap()
            .len(),
        1
    );
    let destination = root.join(if queries_only {
        "recheck-range.csv"
    } else {
        "full-range.csv"
    });
    store
        .analysis_export_range(rev, &scope, &q, None, &destination)
        .unwrap();
    let changes_path = root.join(if queries_only {
        "recheck-changes.csv"
    } else {
        "full-changes.csv"
    });
    store
        .analysis_export_changes(rev, &scope, &changes, &changes_path)
        .unwrap();
    let rss = fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find(|l| l.starts_with("VmHWM:"))
        .unwrap()
        .to_string();
    println!("queries_only={queries_only} reports={reports} values={} amount={} change_rows={} range_csv_bytes={} changes_csv_bytes={} elapsed_seconds={:.3} {rss}",range.values,range.total.amount.unwrap(),totals.rows,fs::metadata(destination).unwrap().len(),fs::metadata(changes_path).unwrap().len(),started.elapsed().as_secs_f64());
}
