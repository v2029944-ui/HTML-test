use super::*;
use crate::storage::import_apply_tests::{fixture, original, prepared};
use analiz_import_core::StageArea;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Barrier},
    thread,
    time::Duration,
};

#[test]
fn sqlite_full_and_index_write_error_roll_back_replacement() {
    for fault in ["full", "index"] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("data.xlsx");
        fixture(&source, 120);
        let area = StageArea::open(root.path().join("stage")).unwrap();
        let p = prepared(&area, &source, "day", "2025-01-01");
        let storage = Storage::new(root.path().join("live")).unwrap();
        let before = original(&storage);
        let preview = storage.preview_import(&p).unwrap();
        let mut conn = storage.connect().unwrap();
        if fault == "full" {
            let pages: u64 = conn
                .query_row("PRAGMA page_count", [], |r| r.get(0))
                .unwrap();
            conn.pragma_update(None, "max_page_count", pages + 1)
                .unwrap();
        } else {
            conn.execute_batch("CREATE TRIGGER refuse_index BEFORE INSERT ON report_rows BEGIN SELECT RAISE(ABORT,'index write failure'); END;").unwrap();
        }
        let result = apply_in_connection(
            &mut conn,
            &p,
            &preview,
            ImportConfirmation {
                replace_exact: true,
                accept_cross_type: false,
                ..Default::default()
            },
            &ImportJob::new(),
            |_| {},
        )
        .unwrap_err();
        assert!(
            result.contains(if fault == "full" {
                "full"
            } else {
                "index write failure"
            }),
            "{result}"
        );
        assert_eq!(storage.load().unwrap(), before);
        assert_eq!(storage.revision().unwrap(), preview.revision());
        assert!(storage
            .committed_import(p.operation_id())
            .unwrap()
            .is_none());
        assert_eq!(
            conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
    }
}
#[test]
fn panic_and_last_boundary_cancellation_roll_back() {
    for fault in ["panic", "boundary", "truncate"] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("data.xlsx");
        fixture(&source, 1000);
        let area = StageArea::open(root.path().join("stage")).unwrap();
        let p = prepared(&area, &source, "day", "2025-01-01");
        let live = root.path().join("live");
        let storage = Storage::new(live.clone()).unwrap();
        let before = original(&storage);
        let preview = storage.preview_import(&p).unwrap();
        let job = ImportJob::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            storage.apply_import(
                &p,
                &preview,
                ImportConfirmation {
                    replace_exact: true,
                    accept_cross_type: false,
                    ..Default::default()
                },
                &job,
                |event| {
                    if event.phase == "copy" && fault == "panic" {
                        panic!("worker panic");
                    }
                    if event.phase == "copy" && fault == "truncate" {
                        fs::write(p.directory().join("report-1.json"), b"{}").unwrap();
                    }
                    if event.phase == "before_commit" && fault == "boundary" {
                        assert!(job.cancel());
                    }
                },
            )
        }));
        assert!(result.is_err() || result.unwrap().is_err());
        let reopened = Storage::new(live).unwrap();
        assert_eq!(reopened.load().unwrap(), before);
        assert_eq!(reopened.revision().unwrap(), preview.revision());
    }
}
#[test]
fn separate_storage_instances_with_one_revision_commit_at_most_once() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("data.xlsx");
    fixture(&source, 120);
    let live = root.path().join("live");
    let storage = Storage::new(live.clone()).unwrap();
    original(&storage);
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|n| {
            let source = source.clone();
            let live = live.clone();
            let stage = root.path().join(format!("stage{n}"));
            let barrier = barrier.clone();
            thread::spawn(move || {
                let area = StageArea::open(stage).unwrap();
                let p = prepared(&area, &source, "day", "2026-11-01");
                let storage = Storage::new(live).unwrap();
                let preview = storage.preview_import(&p).unwrap();
                barrier.wait();
                storage
                    .apply_import(
                        &p,
                        &preview,
                        ImportConfirmation::default(),
                        &ImportJob::new(),
                        |_| {},
                    )
                    .is_ok()
            })
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .map(|h| h.join().unwrap() as usize)
            .sum::<usize>(),
        1
    );
    assert_eq!(
        storage.load().unwrap()["reports"].as_array().unwrap().len(),
        2
    );
}
#[test]
fn changed_index_or_read_failure_rejects_before_writes() {
    for fault in ["database", "read"] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("data.xlsx");
        fixture(&source, 2);
        let area = StageArea::open(root.path().join("stage")).unwrap();
        let p = prepared(&area, &source, "day", "2026-11-01");
        let storage = Storage::new(root.path().join("live")).unwrap();
        let before = original(&storage);
        let preview = storage.preview_import(&p).unwrap();
        if fault == "database" {
            Connection::open(p.directory().join("stage.sqlite3"))
                .unwrap()
                .execute("UPDATE converted_rows SET amount=999", [])
                .unwrap();
        } else {
            let path = p.directory().join("report-1.json");
            fs::remove_file(&path).unwrap();
            fs::create_dir(path).unwrap();
        }
        assert!(storage
            .apply_import(
                &p,
                &preview,
                ImportConfirmation::default(),
                &ImportJob::new(),
                |_| {}
            )
            .is_err());
        assert_eq!(storage.load().unwrap(), before);
    }
}
#[test]
#[ignore = "subprocess helper; invoked by process_death_rolls_back_and_recovers_stage"]
fn kill_child() {
    let root = PathBuf::from(std::env::var_os("C1_KILL_ROOT").unwrap());
    let area = StageArea::open(root.join("stage")).unwrap();
    let p = prepared(&area, &root.join("data.xlsx"), "day", "2025-01-01");
    let storage = Storage::new(root.join("live")).unwrap();
    let preview = storage.preview_import(&p).unwrap();
    storage
        .apply_import(
            &p,
            &preview,
            ImportConfirmation {
                replace_exact: true,
                accept_cross_type: false,
                ..Default::default()
            },
            &ImportJob::new(),
            |event| {
                if event.phase == "copy" {
                    fs::write(root.join("in_transaction"), b"ready").unwrap();
                    loop {
                        thread::park();
                    }
                }
            },
        )
        .unwrap();
}
#[test]
fn process_death_rolls_back_and_recovers_stage() {
    let root = tempfile::tempdir().unwrap();
    fixture(&root.path().join("data.xlsx"), 120);
    let storage = Storage::new(root.path().join("live")).unwrap();
    let before = original(&storage);
    let revision = storage.revision().unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "storage::import_apply::fault_tests::kill_child",
            "--ignored",
            "--nocapture",
        ])
        .env("C1_KILL_ROOT", root.path())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !root.path().join("in_transaction").exists() && Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            panic!("child exited before transaction");
        }
        thread::sleep(Duration::from_millis(20));
    }
    let reached = root.path().join("in_transaction").exists();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(reached);
    let reopened = Storage::new(root.path().join("live")).unwrap();
    assert_eq!(reopened.load().unwrap(), before);
    assert_eq!(reopened.revision().unwrap(), revision);
    let _recovered = StageArea::open(root.path().join("stage")).unwrap();
    assert_eq!(fs::read_dir(root.path().join("stage")).unwrap().count(), 1);
    assert_eq!(
        reopened
            .connect()
            .unwrap()
            .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[test]
fn sql_cancellation_interrupts_large_query_and_clears_hook() {
    let conn = Connection::open_in_memory().unwrap();
    let cancel = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            cancel.store(true, Ordering::Release);
        });
        let started = Instant::now();
        let result = with_sql_cancel(&conn, &cancel, || {
            conn.query_row("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100000000) SELECT sum(x) FROM n", [], |r|r.get::<_,i64>(0)).map_err(err)
        });
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        eprintln!(
            "c2 SQL cancellation observed in {} ms",
            started.elapsed().as_millis()
        );
    });
    assert_eq!(
        conn.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}
