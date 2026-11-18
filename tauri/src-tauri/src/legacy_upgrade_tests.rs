//! Compatibility fixture: Tauri 0.1.3 SQLite schema can be opened by the
//! 0.1.4 SQL read model on an isolated copy without losing existing user data.
use super::*;
use tempfile::tempdir;

#[test]
fn v013_database_is_upgraded_on_an_isolated_copy_without_losing_data() {
    let tmp = tempdir().unwrap();
    let old_path = tmp.path().join("v013-original.sqlite3");
    let old_report = json!({
        "id":"old-day", "type":"day", "start":"2024-02-29", "end":"2024-02-29",
        "rows":[{"work":"Работа", "organization":"СМУ16", "count":2.5, "amount":345.125}]
    });
    let old_settings = json!({
        "orgNames":["СМУ16","АВТО","ТРАК","ЦЕНТР"],
        "groups":[{"name":"Старая группа", "works":["Работа"]}],
        "catalog":["Работа"], "unknownFutureSetting":{"flag":true}
    });
    let old_config = json!({"version":1,"folder":null,"rules":{}});
    {
        // Exact two-table shape of v0.1.3, without the new SQL tables.
        let old = Connection::open(&old_path).unwrap();
        old.execute_batch("CREATE TABLE reports (
            id TEXT PRIMARY KEY, report_type TEXT NOT NULL,
            period_start TEXT NOT NULL, period_end TEXT NOT NULL,
            source_name TEXT, json TEXT NOT NULL);
            CREATE TABLE kv (name TEXT PRIMARY KEY, json TEXT NOT NULL);
            CREATE INDEX reports_by_period ON reports(report_type,period_start);
            CREATE INDEX reports_by_source ON reports(source_name);
        ").unwrap();
        old.execute("INSERT INTO reports(id,report_type,period_start,period_end,source_name,json)
             VALUES(?1,'day','2024-02-29','2024-02-29','annual.xlsx',?2)",
             params!["old-day",old_report.to_string()]).unwrap();
        for (name,value) in [
            ("settings",old_settings.to_string()),
            ("config",old_config.to_string()),
            ("migration_v1",String::from(r#""complete""#)),
            ("unknown_legacy_key",String::from(r#"{"keep":true}"#)),
        ] {
            old.execute("INSERT INTO kv(name,json) VALUES(?1,?2)", params![name,value]).unwrap();
        }
    }
    let root = tmp.path().join("recovery-test-only");
    fs::create_dir(&root).unwrap();
    fs::copy(&old_path, root.join("reports.sqlite3")).unwrap();
    let storage = Storage::new(root).unwrap();
    let opened = storage.load().unwrap();
    assert_eq!(opened["reports"][0],old_report);
    assert_eq!(opened["settings"],old_settings);
    assert!(storage.revision().is_ok());
    assert_eq!(storage.info().unwrap()["rules"],json!({}));
    let newer = storage.connect().unwrap();
    let preserved: String = newer.query_row(
        "SELECT json FROM kv WHERE name='unknown_legacy_key'",[],|r|r.get(0)).unwrap();
    assert_eq!(preserved,r#"{"keep":true}"#);
    // The original file is never passed to Storage or changed.
    let original = Connection::open(&old_path).unwrap();
    let has_new_table: i64 = original.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='storage_revision'",
        [], |r|r.get(0)).unwrap();
    assert_eq!(has_new_table,0);
    let original_rows: i64 = original.query_row(
        "SELECT COUNT(*) FROM reports",[],|r|r.get(0)).unwrap();
    assert_eq!(original_rows,1);
}
