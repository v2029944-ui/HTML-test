use super::*;

#[test]
fn revisions_text_blob_corruption_and_concurrent_publication() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let data = fixture();
    let rev = prepare(&store, &data);
    let s = scope(&store, rev, "day", &["A", "B"], None);
    let c = store.connect().unwrap();
    c.execute(
        "UPDATE reports SET json=CAST(json AS BLOB) WHERE id='a'",
        [],
    )
    .unwrap();
    let new = store.revision().unwrap();
    assert!(store
        .analysis_range_summary(rev, &s, &query("2024-02-28", "2024-02-29"))
        .unwrap_err()
        .contains("Stale"));
    let s = scope(&store, new, "day", &["A", "B"], None);
    assert!(store
        .analysis_range_summary(new, &s, &query("2024-02-28", "2024-02-29"))
        .unwrap_err()
        .contains("unindexed"));
    store.analysis_index_report("a", new).unwrap();
    assert!(
        store
            .analysis_range_summary(new, &s, &query("2024-02-28", "2024-02-29"))
            .unwrap()
            .complete
    );
    c.execute("UPDATE reports SET json=json WHERE id='b'", [])
        .unwrap();
    let rev = store.revision().unwrap();
    let result = store.analysis_index_report_before_publish("b", rev, || {
        c.execute("UPDATE reports SET json=json WHERE id='c'", [])
            .unwrap();
    });
    assert!(result.is_err());
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM analysis_reports WHERE report_id='b'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    let rev = store.revision().unwrap();
    store.analysis_index_report("b", rev).unwrap();
    store.analysis_index_report("c", rev).unwrap();
    c.execute("UPDATE kv SET json=json WHERE name='settings'", [])
        .unwrap();
    let rev = store.revision().unwrap();
    let s = scope(&store, rev, "day", &["A"], None);
    assert!(store
        .analysis_range_summary(rev, &s, &query("2024-02-28", "2024-02-29"))
        .unwrap_err()
        .contains("settings unindexed"));
    store.analysis_index_settings(rev).unwrap();
    c.execute("UPDATE reports SET json=?1", ["{"]).unwrap();
    let rev = store.revision().unwrap();
    assert!(store.analysis_index_report("a", rev).is_err());
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM analysis_reports", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn disk_filters_limits_empty_missing_and_large_batches() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let rev = prepare(&store, &fixture());
    let org = store.analysis_filter_create(rev, "organizations").unwrap();
    let q = query("2024-02-28", "2024-02-29");
    let s = AnalysisScope {
        report_type: "day".into(),
        organizations: org.clone(),
        names: None,
    };
    assert!(store.analysis_range_summary(rev, &s, &q).is_err());
    store.analysis_filter_seal(rev, &org).unwrap();
    assert!(!store.analysis_range_summary(rev, &s, &q).unwrap().complete);
    assert!(store
        .analysis_filter_append(rev, &org, &["A".into()])
        .is_err());
    let id = store.analysis_filter_create(rev, "names").unwrap();
    assert!(store
        .analysis_filter_append(rev, &id, &vec!["x".into(); 501])
        .is_err());
    assert!(store
        .analysis_filter_append(rev, &id, &["x".repeat(65537)])
        .is_err());
    for i in 0..5 {
        store
            .analysis_filter_append(
                rev,
                &id,
                &(0..500)
                    .map(|n| format!("unused-{i}-{n}"))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    store
        .analysis_filter_append(rev, &id, &["Group".into()])
        .unwrap();
    store.analysis_filter_seal(rev, &id).unwrap();
    let mut s = scope(&store, rev, "day", &["A", "B", "Absent"], None);
    s.names = Some(id);
    let summary = store.analysis_range_summary(rev, &s, &q).unwrap();
    assert_eq!(summary.missing, 2);
    assert_eq!(summary.values, 1);
    assert_eq!(summary.total.amount, None);
    assert!(store
        .analysis_range_page(rev, &s, &q, None, None, 0)
        .is_err());
    assert!(store
        .analysis_range_page(rev, &s, &q, None, None, 501)
        .is_err());
    assert!(store
        .analysis_range_page(rev, &s, &q, Some("Outside"), None, 1)
        .is_err());
    let orgs = store
        .analysis_organization_page(rev, &s, &q, None, 500)
        .unwrap();
    assert_eq!(
        orgs.iter().find(|o| o.org == "B").unwrap().total.count,
        Some(0.)
    );
    assert!(!orgs.iter().find(|o| o.org == "Absent").unwrap().complete);
}

#[test]
fn corrupt_or_unsupported_settings_are_explicit_and_never_publish_partial_groups() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let rev = prepare(&store, &fixture());
    let c = store.connect().unwrap();
    for raw in ["{\"groups\":[{\"id\":\"x\",\"name\":\"first\",\"works\":[\"one\"]},{\"id\":\"x\",\"name\":\"second\",\"works\":[\"two\"]}]}","{\"groups\":[],\"groups\":[]}"]{c.execute("UPDATE kv SET json=?1 WHERE name='settings'",[raw]).unwrap();let r=store.revision().unwrap();assert!(r>rev);assert!(store.analysis_index_settings(r).is_err());assert_eq!(c.query_row("SELECT COUNT(*) FROM analysis_settings",[],|r|r.get::<_,i64>(0)).unwrap(),0);assert_eq!(c.query_row("SELECT COUNT(*) FROM analysis_groups",[],|r|r.get::<_,i64>(0)).unwrap(),0);}
    let raw = format!("{{\"groups\":[],\"unknown\":\"{}\"}}", "x".repeat(65537));
    c.execute("UPDATE kv SET json=?1 WHERE name='settings'", [&raw])
        .unwrap();
    let r = store.revision().unwrap();
    assert!(store
        .analysis_index_settings(r)
        .unwrap_err()
        .contains("token exceeds"));
    assert_eq!(
        c.query_row("SELECT json FROM kv WHERE name='settings'", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        raw
    );
}

#[test]
fn old_js_nonstring_row_org_ignored_without_publishing_partial_d1_manifest() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut data = fixture();
    data["reports"][0]["rows"][0]
        .as_object_mut()
        .unwrap()
        .remove("organization");
    let _ = store.revision().unwrap();
    let c = store.connect().unwrap();
    for r in data["reports"].as_array().unwrap() {
        c.execute("INSERT INTO reports(id,report_type,period_start,period_end,json) VALUES(?1,?2,?3,?4,?5)",params![r["id"].as_str(),r["type"].as_str(),r["start"].as_str(),r["end"].as_str(),r.to_string()]).unwrap();
    }
    c.execute(
        "INSERT INTO kv VALUES('settings',?1)",
        [data["settings"].to_string()],
    )
    .unwrap();
    let rev = store.revision().unwrap();
    for r in data["reports"].as_array().unwrap() {
        store
            .analysis_index_report(r["id"].as_str().unwrap(), rev)
            .unwrap();
    }
    store.analysis_index_settings(rev).unwrap();
    check_old(
        &store,
        rev,
        &data,
        "day",
        &["A", "B"],
        &[("2024-02-28", "2024-02-29")],
        &changes("2024-02-28", "2024-02-29", "range"),
    );
    assert!(store.aggregate(rev, &ReadFilter::default()).is_err());
}

#[test]
fn conflict_query_uses_bounded_start_window_index() {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("CREATE TEMP TABLE an_candidates(org TEXT,start TEXT,end TEXT,report_id TEXT,warned INTEGER,PRIMARY KEY(org,start,report_id)) WITHOUT ROWID;CREATE TEMP TABLE an_issues(report_id TEXT,org TEXT,kind TEXT,other_report_id TEXT);").unwrap();
    let mut stmt = c
        .prepare(&format!("EXPLAIN QUERY PLAN {}", analytics::CONFLICT_SQL))
        .unwrap();
    let plans = stmt
        .query_map(["-6 day"], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        plans
            .iter()
            .any(|p| p.contains("SEARCH b USING PRIMARY KEY (org=? AND start>? AND start<?)")),
        "{plans:?}"
    );
}

#[test]
fn work_group_name_search_is_bounded_and_excludes_ambiguous_labels() {
    let dir=tempdir().unwrap();
    let store=Storage::new(dir.path().into()).unwrap();
    let rev=prepare(&store,&fixture());
    let list=store.analysis_name_options(rev,"",50).unwrap();
    assert!(!list.has_more);
    let names=list.rows.iter().map(|x|x.name.as_str()).collect::<Vec<_>>();
    assert!(names.contains(&"Group"));
    assert!(names.contains(&"Ignored"));
    assert!(names.contains(&"gone"));
    assert!(names.contains(&"zero"));
    assert!(!names.contains(&"one")); // grouped work has a group identity
    assert!(!names.contains(&"two"));
    assert_eq!(list.rows.iter().find(|x|x.name=="Group").unwrap().kind,"group");
    assert_eq!(list.rows.iter().find(|x|x.name=="gone").unwrap().kind,"work");
    let filtered=store.analysis_name_options(rev,"gro",50).unwrap();
    assert!(filtered.rows.is_empty()); // exact-case Cyrillic/Latin display search
    let filtered=store.analysis_name_options(rev,"Gro",50).unwrap();
    assert_eq!(filtered.rows.len(),1);
    assert_eq!(filtered.rows[0].name,"Group");
    let page=store.analysis_name_options(rev,"",2).unwrap();
    assert_eq!(page.rows.len(),2);
    assert!(page.has_more);
    assert!(store.analysis_name_options(rev,"",0).is_err());
    assert!(store.analysis_name_options(rev,"",81).is_err());
    assert!(store.analysis_name_options(rev,&"a".repeat(129),50).is_err());
    assert!(store.analysis_name_options(rev+1,"",50).is_err());
    let mut modified=fixture();
    modified["reports"][0]["rows"].as_array_mut().unwrap()
        .push(json!({"organization":"A","work":"Group","count":1.,"amount":2.}));
    let newer=prepare(&store,&modified);
    let collision=store.analysis_name_options(newer,"Group",50).unwrap();
    assert!(collision.rows.is_empty(),
        "a raw work and a group with identical names cannot be selected safely");
}

#[test]
fn name_search_rejects_multibyte_labels_over_utf8_limit() {
    let dir=tempdir().unwrap();
    let store=Storage::new(dir.path().into()).unwrap();
    let mut data=fixture();
    // 300 Cyrillic letters fit in SQLite length()<=512 characters, but
    // require 600 UTF-8 bytes and must not enter a <=512-byte native filter.
    let oversized="К".repeat(300);
    data["reports"][0]["rows"].as_array_mut().unwrap().push(json!({
        "organization":"A","work":oversized,
        "count":1.,"amount":1.
    }));
    let revision=prepare(&store,&data);
    let options=store.analysis_name_options(revision,"К",50).unwrap();
    assert!(options.rows.is_empty(),"UTF-8 byte bound must agree with the native UI");
    let baseline=store.analysis_name_options(revision,"Group",50).unwrap();
    assert_eq!(baseline.rows.len(),1);
    assert_eq!(baseline.rows[0].name,"Group");
}

#[test]
fn concurrent_sealed_analysis_filters_do_not_upgrade_stale_read_snapshots() {
    let dir=tempdir().unwrap();let root=dir.path().to_path_buf();
    let store=Storage::new(root.clone()).unwrap();let rev=prepare(&store,&fixture());
    let barrier=std::sync::Arc::new(std::sync::Barrier::new(6));
    let workers=(0..6).map(|_|{
        let root=root.clone();let barrier=barrier.clone();
        std::thread::spawn(move||{
            let storage=Storage::new(root).unwrap();barrier.wait();
            for _ in 0..10 {
                let id=storage.analysis_filter_create(rev,"organizations").unwrap();
                storage.analysis_filter_append(rev,&id,&["A".into()]).unwrap();
                storage.analysis_filter_seal(rev,&id).unwrap();
                storage.analysis_filter_delete(&id).unwrap();
            }
        })
    }).collect::<Vec<_>>();
    let results=workers.into_iter().map(|worker|worker.join()).collect::<Vec<_>>();
    assert!(results.into_iter().all(|result|result.is_ok()));
    assert_eq!(store.revision().unwrap(),rev);
}

#[test]
fn initialized_wal_reads_do_not_require_the_writer_lock() {
    let dir=tempdir().unwrap();
    let store=Storage::new(dir.path().into()).unwrap();
    let rev=prepare(&store,&fixture());
    let writer=rusqlite::Connection::open(dir.path().join("reports.sqlite3")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let read=store.revision();
    writer.execute_batch("ROLLBACK").unwrap();
    assert_eq!(read.unwrap(),rev,"opening a ready WAL database must not perform schema writes");
}

#[test]
fn schema_v1_preserves_indexed_rows_and_report_metadata_when_allowing_null_amounts() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("reports.sqlite3");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("
        PRAGMA user_version=1;
        CREATE TABLE reports(id TEXT PRIMARY KEY,report_type TEXT NOT NULL,period_start TEXT NOT NULL,period_end TEXT NOT NULL,source_name TEXT,json TEXT NOT NULL);
        INSERT INTO reports VALUES('old','day','2024-01-01','2024-01-01',NULL,'{}');
        CREATE TABLE storage_revision(singleton INTEGER PRIMARY KEY,revision INTEGER NOT NULL);
        INSERT INTO storage_revision VALUES(1,7);
        CREATE TABLE report_row_index(report_id TEXT PRIMARY KEY,row_count INTEGER NOT NULL);
        INSERT INTO report_row_index VALUES('old',1);
        CREATE TABLE report_rows(report_id TEXT NOT NULL,row_index INTEGER NOT NULL,organization TEXT NOT NULL,work TEXT NOT NULL,count REAL NOT NULL,amount REAL NOT NULL,PRIMARY KEY(report_id,row_index)) WITHOUT ROWID;
        INSERT INTO report_rows VALUES('old',0,'A','work',2,12);
        CREATE TABLE management_report_meta(report_id TEXT PRIMARY KEY,amount REAL NOT NULL,saved_amount REAL,warnings INTEGER NOT NULL,shared_source_artifact INTEGER NOT NULL);
        INSERT INTO management_report_meta VALUES('old',12,12,0,0);
        CREATE TRIGGER read_model_report_insert AFTER INSERT ON reports BEGIN DELETE FROM report_rows WHERE report_id=NEW.id; END;
        CREATE TRIGGER read_model_report_update AFTER UPDATE ON reports BEGIN DELETE FROM report_rows WHERE report_id=OLD.id; END;
        CREATE TRIGGER read_model_report_delete AFTER DELETE ON reports BEGIN DELETE FROM report_rows WHERE report_id=OLD.id; END;
        CREATE TRIGGER management_report_insert AFTER INSERT ON reports BEGIN DELETE FROM management_report_meta WHERE report_id=NEW.id; END;
        CREATE TRIGGER management_report_update AFTER UPDATE ON reports BEGIN DELETE FROM management_report_meta WHERE report_id=OLD.id; END;
        CREATE TRIGGER management_report_delete AFTER DELETE ON reports BEGIN DELETE FROM management_report_meta WHERE report_id=OLD.id; END;
    ").unwrap();
    drop(c);

    let c = super::new_connection(&path).unwrap();
    let state = c.query_row(
        "SELECT r.json,x.count,x.amount,m.amount,i.row_count,(SELECT revision FROM storage_revision)
         FROM reports r JOIN report_rows x ON x.report_id=r.id JOIN management_report_meta m ON m.report_id=r.id
         JOIN report_row_index i ON i.report_id=r.id",
        [],
        |row| Ok((row.get::<_,String>(0)?,row.get::<_,f64>(1)?,row.get::<_,f64>(2)?,
                  row.get::<_,f64>(3)?,row.get::<_,i64>(4)?,row.get::<_,i64>(5)?)),
    ).unwrap();
    assert_eq!(state, ("{}".into(),2.0,12.0,12.0,1,7));
    c.execute_batch("UPDATE report_rows SET amount=NULL; UPDATE management_report_meta SET amount=NULL;").unwrap();
    assert_eq!(c.query_row("PRAGMA integrity_check", [], |row|row.get::<_,String>(0)).unwrap(), "ok");
    c.execute("DELETE FROM reports WHERE id='old'", []).unwrap();
    assert_eq!(c.query_row("SELECT (SELECT count(*) FROM report_rows)+(SELECT count(*) FROM management_report_meta)", [], |row|row.get::<_,i64>(0)).unwrap(), 0);
}

#[test]
fn schema_initialization_is_atomic_and_reopens_restore_delete_mode() {
    let dir=tempdir().unwrap();let path=dir.path().join("reports.sqlite3");
    let c=rusqlite::Connection::open(&path).unwrap();
    // A deliberately incompatible unversioned table must not leave a stamped,
    // partially initialized schema. Repairing that fixture must allow retry.
    c.execute_batch("CREATE TABLE report_rows(broken INTEGER)").unwrap();
    assert!(super::new_connection(&path).is_err());
    assert_eq!(c.query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(c.query_row("SELECT count(*) FROM sqlite_schema WHERE name='reports'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    c.execute_batch("DROP TABLE report_rows").unwrap();drop(c);
    let c=super::new_connection(&path).unwrap();
    assert_eq!(c.query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0)).unwrap(),super::STORAGE_SCHEMA_VERSION);
    c.execute_batch("PRAGMA journal_mode=DELETE").unwrap();drop(c);
    let c=super::new_connection(&path).unwrap();
    assert_eq!(c.query_row("PRAGMA journal_mode",[],|r|r.get::<_,String>(0)).unwrap(),"wal");
    c.pragma_update(None,"user_version",super::STORAGE_SCHEMA_VERSION+1).unwrap();
    c.execute_batch("PRAGMA journal_mode=DELETE").unwrap();drop(c);
    assert!(super::new_connection(&path).unwrap_err().contains("новее"));
    let c=rusqlite::Connection::open(&path).unwrap();
    assert_eq!(c.query_row("PRAGMA journal_mode",[],|r|r.get::<_,String>(0)).unwrap(),"delete");
}

#[test]
fn analysis_filter_writers_share_only_their_database_queue() {
    let dir=tempdir().unwrap();let root=dir.path().join("one");
    let first=Storage::new(root.clone()).unwrap();let rev=prepare(&first,&fixture());
    let second=Storage::new(root.join(".")).unwrap();
    assert!(std::sync::Arc::ptr_eq(&first.analysis_filter_gate,&second.analysis_filter_gate));
    let guard=first.analysis_filter_gate.lock().unwrap();
    assert!(matches!(second.analysis_filter_gate.try_lock(),Err(std::sync::TryLockError::WouldBlock)));
    let (send,receive)=std::sync::mpsc::channel();
    let worker=std::thread::spawn(move||{send.send(second.analysis_filter_create(rev,"organizations")).unwrap();});
    let other=Storage::new(dir.path().join("two")).unwrap();let other_rev=prepare(&other,&fixture());
    assert!(!std::sync::Arc::ptr_eq(&first.analysis_filter_gate,&other.analysis_filter_gate));
    let other_id=other.analysis_filter_create(other_rev,"organizations").unwrap();
    other.analysis_filter_delete(&other_id).unwrap();
    assert!(matches!(receive.try_recv(),Err(std::sync::mpsc::TryRecvError::Empty)));
    drop(guard);
    let id=receive.recv_timeout(std::time::Duration::from_secs(15)).unwrap().unwrap();
    worker.join().unwrap();first.analysis_filter_delete(&id).unwrap();
}

#[test]
fn hierarchy_index_has_the_full_lineage_and_unique_direct_work_assignments() {
    let dir = tempfile::tempdir().unwrap();
    let store = Storage::new(dir.path().to_owned()).unwrap();
    store.save(json!({"reports":[], "settings":{
        "orgNames":["A","B","C","D"],"catalog":[],
        "groups":[{"id":"head","name":"Head","works":["Direct"]},
            {"id":"branch","name":"Branch","works":[]},
            {"id":"leaf","name":"Leaf","works":["Nested"]}],
        "workGroupTreeParents":[{"child":"leaf","parent":"branch"},{"child":"branch","parent":"head"}]
    }})).unwrap();
    let rev = store.revision().unwrap();
    store.analysis_index_settings(rev).unwrap();
    let c = store.connect().unwrap();
    let mut stmt = c.prepare(
        "SELECT ancestor_key,distance FROM analysis_group_ancestors
         WHERE descendant_key='group:leaf' ORDER BY distance",
    ).unwrap();
    let lineage = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?,r.get::<_, i64>(1)?)))
        .unwrap().collect::<Result<Vec<_>,_>>().unwrap();
    assert_eq!(lineage, vec![("group:leaf".into(),0),("group:branch".into(),1),("group:head".into(),2)]);
    assert_eq!(c.query_row("SELECT COUNT(*) FROM analysis_groups", [], |r|r.get::<_, i64>(0)).unwrap(), 2);
}

#[test]
fn work_tree_migration_installs_schema_and_invalidates_old_settings_index() {
    let dir = tempfile::tempdir().unwrap();
    let store = Storage::new(dir.path().to_owned()).unwrap();
    let canonical = json!({"reports":[], "settings":{
        "orgNames":["A","B","C","D"],"catalog":[],
        "groups":[{"id":"head","name":"Head","works":[]},
            {"id":"leaf","name":"Leaf","works":["Nested"]}],
        "workGroupTreeParents":[{"child":"leaf","parent":"head"}]
    }});
    store.save(canonical.clone()).unwrap();
    let revision = store.revision().unwrap();
    store.analysis_index_settings(revision).unwrap();
    let c = store.connect().unwrap();
    c.execute_batch("DROP TRIGGER analysis_group_index_settings_insert;
        DROP TRIGGER analysis_group_index_settings_update;
        DROP TRIGGER analysis_group_index_settings_delete;
        DROP TABLE analysis_group_ancestors;
        DROP TABLE analysis_group_nodes;
        DROP INDEX analysis_group_works_by_key;
        PRAGMA user_version=2;").unwrap();
    drop(c);

    let c = store.connect().unwrap();
    assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 3);
    assert_eq!(store.revision().unwrap(), revision);
    assert_eq!(store.load().unwrap(), canonical);
    assert!(store.unified_name_options_at_page(revision, "", 0, 50)
        .err().unwrap().contains("not indexed"));

    store.analysis_index_settings(revision).unwrap();
    let children = store.unified_group_children_at_page(revision, "group:head", 0, 50).unwrap();
    assert_eq!(children.total_rows, 1);
    assert_eq!(children.rows[0].key, "group:leaf");
}
