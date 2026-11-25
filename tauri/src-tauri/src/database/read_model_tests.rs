use super::*;
use tempfile::tempdir;
fn data() -> Value {
    json!({"reports":[{"id":"r1","type":"week","start":"2026-11-02","end":"2026-11-08",
        "rows":[{"organization":"A","work":"W","count":2,"amount":10},
                {"organization":"B","work":"X","count":3,"amount":30},
                {"organization":"A","work":"W","count":5,"amount":60}]}],"settings":default_settings()})
}
#[test]
fn pages_aggregate_and_backup() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().join("source")).unwrap();
    store.save(data()).unwrap();
    let rev = store.revision().unwrap();
    let filter = ReadFilter::default();
    let page = store.row_page(rev, &filter, None, 1).unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(
        store
            .row_page(
                rev,
                &filter,
                Some((&page[0].report_id, page[0].row_index)),
                5
            )
            .unwrap()
            .len(),
        2
    );
    let total = store.aggregate(rev, &filter).unwrap();
    assert_eq!(
        (
            total.rows,
            total.count,
            total.amount,
            total.weighted_average
        ),
        (3, 10.0, Some(100.0), Some(10.0))
    );
    let only_a = ReadFilter {
        organizations: vec!["A".into()],
        ..ReadFilter::default()
    };
    assert_eq!(store.aggregate(rev, &only_a).unwrap().rows, 2);
    let partial_week = ReadFilter {
        start: Some("2026-11-03".into()),
        ..ReadFilter::default()
    };
    assert_eq!(store.aggregate(rev, &partial_week).unwrap().rows, 0);
    let reports = store.report_page(rev, &filter, None, 1).unwrap();
    assert_eq!(reports.len(), 1);
    assert!(store
        .report_page(
            rev,
            &filter,
            Some((&reports[0].period_start, &reports[0].id)),
            1
        )
        .unwrap()
        .is_empty());
    let backup = dir.path().join("backup.sqlite3");
    store.backup_sqlite(&backup).unwrap();
    assert!(store.backup_sqlite(&backup).is_err());
    let sentinel = dir.path().join("existing.sqlite3");
    fs::write(&sentinel, b"do not replace").unwrap();
    assert!(store.backup_sqlite(&sentinel).is_err());
    assert_eq!(fs::read(&sentinel).unwrap(), b"do not replace");
    #[cfg(unix)]
    {
        let link = dir.path().join("backup-link.sqlite3");
        std::os::unix::fs::symlink(&sentinel, &link).unwrap();
        assert!(store.backup_sqlite(&link).is_err());
        assert_eq!(fs::read(&sentinel).unwrap(), b"do not replace");
    }
    let copied = Connection::open(&backup).unwrap();
    let text: String = copied
        .query_row("SELECT json FROM reports", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["rows"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        copied
            .query_row("SELECT COUNT(*) FROM report_rows", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        3
    );
}
#[test]
fn external_changes_invalidate_tokens_and_indexes() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    store.save(data()).unwrap();
    let rev = store.revision().unwrap();
    assert_eq!(store.revision().unwrap(), rev);
    let external = Connection::open(store.db_path()).unwrap();
    external
        .execute("UPDATE reports SET json=json WHERE id='r1'", [])
        .unwrap();
    assert!(store.aggregate(rev, &ReadFilter::default()).is_err());
    let changed = store.revision().unwrap();
    assert!(changed > rev);
    assert!(store.aggregate(changed, &ReadFilter::default()).is_err());
    store.index_report("r1", changed).unwrap();
    assert_eq!(
        store
            .aggregate(changed, &ReadFilter::default())
            .unwrap()
            .rows,
        3
    );
    external
        .execute("INSERT INTO kv VALUES('external','{}')", [])
        .unwrap();
    let kvrev = store.revision().unwrap();
    assert!(kvrev > changed);
    external
        .execute("UPDATE kv SET json='[]' WHERE name='external'", [])
        .unwrap();
    external
        .execute("DELETE FROM kv WHERE name='external'", [])
        .unwrap();
    assert_eq!(store.revision().unwrap(), kvrev + 2);
    external.execute("UPDATE reports SET json='{\"rows\":[{\"organization\":\"A\",\"work\":\"W\",\"count\":1,\"amount\":2},BROKEN'", []).unwrap();
    let malformed = store.revision().unwrap();
    assert!(store.index_report("r1", malformed).is_err());
    assert!(store.aggregate(malformed, &ReadFilter::default()).is_err());
    assert_eq!(
        external
            .query_row("SELECT COUNT(*) FROM reports", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        external
            .query_row("SELECT COUNT(*) FROM report_rows", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn huge_report_streaming_and_atomic_invalid_backfill() {
    use std::io::Write;
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let conn = store.connect().unwrap();
    let path = dir.path().join("huge.json");
    let mut out = fs::File::create(&path).unwrap();
    write!(out, "{{\"id\":\"huge\",\"type\":\"day\",\"start\":\"2026-11-01\",\"end\":\"2026-11-01\",\"metadata\":[").unwrap();
    for i in 0..100_000 {
        if i > 0 {
            write!(out, ",").unwrap();
        }
        write!(out, "[1,2,3]").unwrap();
    }
    write!(out, "],\"rows\":[").unwrap();
    for i in 0..100_000 {
        if i > 0 {
            write!(out, ",").unwrap();
        }
        write!(out, "{{\"organization\":\"A\",\"work\":\"W\",\"count\":1,\"amount\":2,\"unknown\":[1,2,3]}}").unwrap();
    }
    write!(out, "]}}").unwrap();
    drop(out);
    conn.execute(
        "INSERT INTO reports VALUES('huge','day','2026-11-01','2026-11-01',NULL,?1)",
        [fs::read_to_string(path).unwrap()],
    )
    .unwrap();
    let rev = store.revision().unwrap();
    assert_eq!(store.index_report("huge", rev).unwrap(), 100_000);
    assert_eq!(
        store.aggregate(rev, &ReadFilter::default()).unwrap().rows,
        100_000
    );
    assert_eq!(
        store
            .row_page(rev, &ReadFilter::default(), None, 1)
            .unwrap()
            .len(),
        1
    );
    for bad in ["{\"rows\":[{\"organization\":\"A\",\"work\":\"W\",\"count\":1,\"amount\":2},{\"work\":\"bad\"}]}", "{\"rows\":[],\"rows\":[]}", "{\"metadata\":[1,2]}"] {
        conn.execute("UPDATE reports SET json=?1 WHERE id='huge'", [bad]).unwrap();
        assert!(store.index_report("huge", store.revision().unwrap()).is_err());
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM report_rows", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM reports", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn concurrent_change_during_backfill_cannot_publish_an_old_snapshot() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    store.save(data()).unwrap();
    let conn = Connection::open(store.db_path()).unwrap();
    conn.execute("UPDATE reports SET json=json", []).unwrap();
    let rev = store.revision().unwrap();
    assert!(store
        .index_report_before_publish("r1", rev, || {
            conn.execute("UPDATE reports SET source_name='concurrent.xlsx'", [])
                .unwrap();
        })
        .is_err());
    assert!(store.revision().unwrap() > rev);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM report_row_index", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM reports", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    store.index_report("r1", store.revision().unwrap()).unwrap();
}

#[test]
fn index_write_failure_rolls_back_reports_revision_settings_and_rows() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    store.save(data()).unwrap();
    let rev = store.revision().unwrap();
    let conn = Connection::open(store.db_path()).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_index BEFORE INSERT ON report_rows WHEN NEW.row_index=2 BEGIN SELECT RAISE(ABORT,'injected index failure'); END;").unwrap();
    let mut replacement = data();
    replacement["reports"][0]["rows"][0]["amount"] = json!(999);
    assert!(store.save(replacement).is_err());
    assert_eq!(store.revision().unwrap(), rev);
    assert_eq!(store.load().unwrap(), data());
    assert_eq!(
        store.aggregate(rev, &ReadFilter::default()).unwrap().amount,
        Some(100.0)
    );
}

#[test]
fn scalar_filter_page_and_numeric_limits_fail_explicitly() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    store.save(data()).unwrap();
    let rev = store.revision().unwrap();
    assert!(store
        .row_page(rev, &ReadFilter::default(), None, 0)
        .is_err());
    assert!(store
        .row_page(rev, &ReadFilter::default(), None, 501)
        .is_err());
    let filter = ReadFilter {
        works: vec!["W".into()],
        report_type: Some("week".into()),
        end: Some("2026-11-08".into()),
        ..ReadFilter::default()
    };
    assert_eq!(store.aggregate(rev, &filter).unwrap().amount, Some(70.0));
    let conn = Connection::open(store.db_path()).unwrap();
    let mut huge = data()["reports"][0].clone();
    huge["metadata"] = json!("x".repeat(65537));
    conn.execute("UPDATE reports SET json=?1", [huge.to_string()])
        .unwrap();
    assert!(store
        .index_report("r1", store.revision().unwrap())
        .unwrap_err()
        .contains("64 KiB"));
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM report_rows", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    huge.as_object_mut().unwrap().remove("metadata");
    huge["id"] = json!("mismatched");
    conn.execute("UPDATE reports SET json=?1", [huge.to_string()])
        .unwrap();
    assert!(store.index_report("r1", store.revision().unwrap()).is_err());
    let mut long_legacy = data();
    long_legacy["reports"][0]["rows"][0]["organization"] = json!("A".repeat(65537));
    store.save(long_legacy.clone()).unwrap();
    assert_eq!(store.load().unwrap(), long_legacy);
    assert!(store
        .aggregate(store.revision().unwrap(), &ReadFilter::default())
        .is_err());
    let mut overflow = data();
    for row in overflow["reports"][0]["rows"].as_array_mut().unwrap() {
        row["count"] = json!(1e308);
    }
    store.save(overflow).unwrap();
    assert!(store
        .aggregate(store.revision().unwrap(), &ReadFilter::default())
        .is_err());
}

#[test]
fn original_sqlite_schema_upgrades_without_replacing_json_or_reports() {
    let dir = tempdir().unwrap();
    let conn = Connection::open(dir.path().join("reports.sqlite3")).unwrap();
    conn.execute_batch("CREATE TABLE reports(id TEXT PRIMARY KEY, report_type TEXT NOT NULL,period_start TEXT NOT NULL,period_end TEXT NOT NULL,source_name TEXT,json TEXT NOT NULL); CREATE TABLE kv(name TEXT PRIMARY KEY,json TEXT NOT NULL);").unwrap();
    let raw = data()["reports"][0].to_string();
    conn.execute(
        "INSERT INTO reports VALUES('r1','week','2026-11-02','2026-11-08',NULL,?1)",
        [&raw],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO kv VALUES('settings',?1)",
        [default_settings().to_string()],
    )
    .unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    assert_eq!(store.load().unwrap(), data());
    let rev = store.revision().unwrap();
    assert_eq!(
        store
            .report_page(rev, &ReadFilter::default(), None, 1)
            .unwrap()
            .len(),
        1
    );
    let backup = dir.path().join("unindexed-backup.sqlite3");
    store.backup_sqlite(&backup).unwrap();
    let copied = Connection::open(backup).unwrap();
    assert_eq!(
        copied
            .query_row("SELECT json FROM reports", [], |r| r.get::<_, String>(0))
            .unwrap(),
        raw
    );
    assert!(store.aggregate(rev, &ReadFilter::default()).is_err());
    assert_eq!(store.index_report("r1", rev).unwrap(), 3);
    assert_eq!(store.load().unwrap(), data());
    assert_eq!(
        conn.query_row("SELECT json FROM reports", [], |r| r.get::<_, String>(0))
            .unwrap(),
        raw
    );
    assert_eq!(
        conn.query_row("SELECT typeof(json) FROM reports", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "text"
    );
    conn.execute("DELETE FROM reports WHERE id='r1'", [])
        .unwrap();
    let deleted = store.revision().unwrap();
    assert!(deleted > rev);
    assert_eq!(
        store
            .aggregate(deleted, &ReadFilter::default())
            .unwrap()
            .rows,
        0
    );
    conn.execute(
        "INSERT INTO reports VALUES('r1','week','2026-11-02','2026-11-08',NULL,?1)",
        [&raw],
    )
    .unwrap();
    assert!(store.revision().unwrap() > deleted);
    assert!(store
        .aggregate(store.revision().unwrap(), &ReadFilter::default())
        .is_err());
    assert!(store
        .row_page(rev, &ReadFilter::default(), None, 10)
        .is_err());
    assert!(store
        .report_page(rev, &ReadFilter::default(), None, 10)
        .is_err());
}

#[test]
fn oversized_headers_fail_before_filters_sorting_or_key_lookups_without_data_changes() {
    for column in [
        "id",
        "report_type",
        "period_start",
        "period_end",
        "source_name",
    ] {
        let dir = tempdir().unwrap();
        let conn = Connection::open(dir.path().join("reports.sqlite3")).unwrap();
        conn.execute_batch("CREATE TABLE reports(id TEXT PRIMARY KEY,report_type TEXT NOT NULL,period_start TEXT NOT NULL,period_end TEXT NOT NULL,source_name TEXT,json TEXT NOT NULL); CREATE TABLE kv(name TEXT PRIMARY KEY,json TEXT NOT NULL);").unwrap();
        let oversized = "z".repeat(4 * 1024 * 1024);
        let mut raw = data()["reports"][0].clone();
        let json_field = match column {
            "id" => "id",
            "report_type" => "type",
            "period_start" => "start",
            "period_end" => "end",
            _ => "desktopSource",
        };
        raw[json_field] = json!(oversized);
        let serialized = raw.to_string();
        conn.execute(
            "INSERT INTO reports VALUES('r1','week','2026-11-02','2026-11-08',NULL,?1)",
            [&serialized],
        )
        .unwrap();
        conn.execute(&format!("UPDATE reports SET {column}=?1"), [&oversized])
            .unwrap();
        let store = Storage::new(dir.path().into()).unwrap();
        let rev = store.revision().unwrap();
        let filter = ReadFilter {
            report_type: Some("day".into()),
            start: Some("2027-01-01".into()),
            ..ReadFilter::default()
        };
        for selected in [&ReadFilter::default(), &filter] {
            let failure = store
                .report_page(rev, selected, Some(("2026-01-01", "cursor")), 1)
                .unwrap_err();
            assert!(failure.contains("64 KiB"), "{column}: {failure}");
            assert!(store
                .row_page(rev, selected, Some(("cursor", 0)), 1)
                .unwrap_err()
                .contains("64 KiB"));
            assert!(store
                .aggregate(rev, selected)
                .unwrap_err()
                .contains("64 KiB"));
        }
        assert!(store
            .index_report("r1", rev)
            .unwrap_err()
            .contains("64 KiB"));
        let preserved: String = conn
            .query_row("SELECT json FROM reports", [], |r| r.get(0))
            .unwrap();
        assert_eq!(preserved, serialized);
        assert_eq!(store.load().unwrap()["reports"][0], raw);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM reports", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(store.revision().unwrap(), rev);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('reports_by_source','reports_by_period')",[],|r|r.get::<_,i64>(0)).unwrap(),0,"first open must not build missing indexes over oversized keys");
        if matches!(column, "id" | "source_name") {
            let legacy = json!({"reports":[raw.clone()],"settings":default_settings()});
            store.save(legacy.clone()).unwrap();
            assert_eq!(
                store.load().unwrap(),
                legacy,
                "oversized metadata accepted by the original format must remain saveable"
            );
        }
    }
}

#[test]
fn sqlite_metadata_preflight_uses_payload_free_byte_lengths_and_preserves_old_indexes() {
    let dir = tempdir().unwrap();
    let conn = Connection::open(dir.path().join("reports.sqlite3")).unwrap();
    conn.execute_batch("CREATE TABLE reports(id TEXT PRIMARY KEY,report_type TEXT NOT NULL,period_start TEXT NOT NULL,period_end TEXT NOT NULL,source_name TEXT,json TEXT NOT NULL); CREATE TABLE kv(name TEXT PRIMARY KEY,json TEXT NOT NULL); CREATE INDEX reports_by_source ON reports(source_name);").unwrap();
    let old_index: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name='reports_by_source'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    store.revision().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT sql FROM sqlite_master WHERE name='reports_by_source'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        old_index
    );
    let mut explain = conn
        .prepare(&format!("EXPLAIN {}", read_model::OVERSIZED_METADATA_SQL))
        .unwrap();
    let columns: Vec<(i64, i64)> = explain
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(1)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(6)?,
            ))
        })
        .unwrap()
        .map(|r| r.unwrap())
        .filter(|(op, _, _)| op == "Column")
        .map(|(_, column, flags)| (column, flags))
        .collect();
    assert_eq!(
        columns.len(),
        5,
        "every metadata field must use the size-only path"
    );
    for (column, flags) in columns {
        assert!(column <= 4);
        assert_eq!(
            flags & 0xc0,
            0xc0,
            "OP_Column must use BYTELENARG, not materialize the TEXT"
        );
    }
    let plan: Vec<String> = conn
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            read_model::OVERSIZED_METADATA_SQL
        ))
        .unwrap()
        .query_map([], |r| r.get::<_, String>(3))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert!(
        plan.iter().any(|line| line == "SCAN reports"),
        "guard must scan table records instead of giant index keys: {plan:?}"
    );
    assert!(!plan
        .iter()
        .any(|line| line.contains("USING") || line.contains("TEMP B-TREE")));
}
