//! Original-interface data administration without materializing report history.
//! Organization renaming publishes a verified backup before mutation. Changes
//! to reports, source ownership and diagnostics commit in one SQLite transaction.
use super::*;

fn normalized_org(s: &str) -> String {
    s.to_lowercase()
        .trim_end_matches(['.', ',', ';', ':'])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn backup_before(
    store: &Storage,
    expected: i64,
    destination: &Path,
    job: &ManagementJob,
) -> Result<(), String> {
    job.check()?;
    check_revision(&store.management_connect()?, expected)?;
    if store.backup_sqlite_verified(destination)? != expected {
        return Err("Stale storage revision; refresh before changing stored data".into());
    }
    job.check()
}

// Bounded document staging uses the same token/depth limits, private lease,
// disk quota, full-fidelity JSON tree and serializer as settings management.
fn stage_document(
    stage: &Stage,
    c: &Connection,
    root: i64,
    job: &ManagementJob,
) -> Result<(PathBuf, u64, Vec<u8>), String> {
    let path = stage.dir.join("admin-document.json");
    let file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(err)?;
    let mut writer = DocumentWriter {
        inner: BufWriter::with_capacity(65536, file),
        bytes: 0,
        limit: (stage.quota / 3).min(i32::MAX as u64),
        hash: Sha256::new(),
    };
    tree::write(c, root, &mut writer, job, 0)?;
    writer.flush().map_err(err)?;
    writer.inner.get_ref().sync_all().map_err(err)?;
    let bytes = writer.bytes;
    let digest = writer.hash.clone().finalize().to_vec();
    drop(writer);
    stage.check_quota()?;
    Ok((path, bytes, digest))
}

fn copy_document(
    c: &Connection,
    table: &str,
    rowid: i64,
    path: &Path,
    bytes: u64,
    digest: &[u8],
    job: &ManagementJob,
) -> Result<(), String> {
    let mut blob = c
        .blob_open("main", table, "json", rowid, false)
        .map_err(err)?;
    let mut input = BufReader::with_capacity(65536, fs::File::open(path).map_err(err)?);
    let mut buffer = [0u8; 65536];
    let mut copied = 0u64;
    let mut hash = Sha256::new();
    loop {
        job.check()?;
        let n = input.read(&mut buffer).map_err(err)?;
        if n == 0 {
            break;
        }
        copied = copied
            .checked_add(n as u64)
            .ok_or("Document size overflow")?;
        if copied > bytes {
            return Err("Staged document grew during publication".into());
        }
        blob.write_all(&buffer[..n]).map_err(err)?;
        hash.update(&buffer[..n]);
    }
    blob.close().map_err(err)?;
    if copied != bytes || hash.finalize().as_slice() != digest {
        return Err("Staged document changed during publication".into());
    }
    Ok(())
}

fn require_root(c: &Connection, root: i64) -> Result<(), String> {
    let kind: String = c
        .query_row(
            "SELECT kind FROM management_nodes WHERE handle=?1",
            [root],
            |r| r.get(0),
        )
        .map_err(err)?;
    if kind != "object" {
        return Err("Saved document must be a JSON object".into());
    }
    Ok(())
}

fn rename_report(c: &Connection, root: i64) -> Result<(), String> {
    require_root(c, root)?;
    let (rows, kind, _) = tree::child(c, root, "rows")?.ok_or("Saved report has no rows array")?;
    if kind != "array" {
        return Err("Saved report rows must be an array".into());
    }
    let bad: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM management_nodes r WHERE r.parent=?1 AND
         (r.kind<>'object' OR (SELECT COUNT(*) FROM management_nodes x WHERE x.parent=r.handle AND x.field='organization' AND x.kind='string')<>1))",
        [rows], |r| r.get(0)).map_err(err)?;
    if bad {
        return Err("Saved report contains malformed organization rows".into());
    }
    // One UPDATE reads every original value once, so A->B / B->A is a swap.
    // Only known domain fields change; source identities, hashes, arbitrary
    // extension fields and unrelated strings remain intact.
    c.execute(
        "UPDATE management_nodes AS n SET value=(SELECT new FROM admin_org_map WHERE old=n.value)
         WHERE n.kind='string' AND EXISTS(SELECT 1 FROM admin_org_map WHERE old=n.value)
         AND (n.parent IN(SELECT handle FROM management_nodes WHERE parent=?1 AND field='organizations' AND kind='array')
          OR (n.field='organization' AND n.parent IN(
             SELECT item.handle FROM management_nodes item JOIN management_nodes list ON item.parent=list.handle
             WHERE list.parent=?1 AND list.field IN('rows','issues','sourceIssues') AND list.kind='array' AND item.kind='object'))
          OR (n.field='name' AND n.parent IN(
             SELECT item.handle FROM management_nodes item JOIN management_nodes list ON item.parent=list.handle
             WHERE list.parent=?1 AND list.field='totals' AND list.kind='array' AND item.kind='object')))",
        [root]).map_err(err)?;
    Ok(())
}

impl Storage {
    pub fn unified_rename_organizations(
        &self,
        expected: i64,
        names: &[String],
        destination: &Path,
        job: &ManagementJob,
    ) -> Result<ManagementReceipt, String> {
        if names.len() != 4 {
            return Err("Нужны четыре названия подразделений".into());
        }
        let names: Vec<String> = names.iter().map(|s| s.trim().to_owned()).collect();
        let mut normalized = std::collections::HashSet::new();
        for name in &names {
            label(name)?;
            if name.is_empty() || !normalized.insert(normalized_org(name)) {
                return Err("Нужны четыре непустых разных названия подразделений".into());
            }
        }
        backup_before(self, expected, destination, job)?;
        let stage = new_stage(self, 16 * 1024 * 1024 * 1024)?;
        let disk = stage.open()?;
        tree::schema(&disk)?;
        disk.execute_batch("CREATE TABLE admin_org_map(old TEXT PRIMARY KEY,new TEXT NOT NULL);")
            .map_err(err)?;
        let live = self.management_connect()?;
        let revision = job.run(&live, || {
            let tx = live.unchecked_transaction().map_err(err)?;
            // Obtain the writer lock before reading documents. No retry can
            // accidentally publish a stale rename after a concurrent import.
            tx.execute("UPDATE storage_revision SET revision=revision WHERE singleton=1", []).map_err(err)?;
            check_revision(&tx, expected)?;
            check_metadata_bounds(&tx)?;
            let disk_tx = disk.unchecked_transaction().map_err(err)?;
            let settings_row: Option<i64> = tx.query_row("SELECT rowid FROM kv WHERE name='settings'", [], |r| r.get(0)).optional().map_err(err)?;
            let root = if let Some(row) = settings_row {
                tree::parse(&disk, tx.blob_open("main", "kv", "json", row, true).map_err(err)?, job)?
            } else {
                tree::parse(&disk, std::io::Cursor::new(serde_json::to_vec(&default_settings()).map_err(err)?), job)?
            };
            tree::derive(&disk, root, job)?;
            let old: Vec<String> = disk.prepare("SELECT name FROM management_orgs ORDER BY ordinal").map_err(err)?
                .query_map([], |r| r.get(0)).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
            if old.len() != 4 || old.iter().collect::<std::collections::HashSet<_>>().len() != 4 {
                return Err("Stored organization settings must contain four distinct names".into());
            }
            for (old, new) in old.iter().zip(&names) {
                disk.execute("INSERT INTO admin_org_map VALUES(?1,?2)", params![old,new]).map_err(err)?;
            }
            disk.execute("UPDATE management_nodes AS n SET value=(SELECT new FROM admin_org_map WHERE old=n.value)
                WHERE n.kind='string' AND EXISTS(SELECT 1 FROM admin_org_map WHERE old=n.value)
                AND (n.parent=(SELECT orgs_node FROM management_settings)
                  OR (n.field='organization' AND n.parent IN(SELECT handle FROM management_catalog)))", []).map_err(err)?;
            tree::derive(&disk, root, job)?;
            let (path, bytes, digest) = stage_document(&stage, &disk, root, job)?;
            disk_tx.commit().map_err(err)?;
            tx.execute("INSERT INTO kv(name,json) VALUES('settings',zeroblob(?1)) ON CONFLICT(name) DO UPDATE SET json=excluded.json", [bytes]).map_err(err)?;
            let settings_row: i64 = tx.query_row("SELECT rowid FROM kv WHERE name='settings'", [], |r| r.get(0)).map_err(err)?;
            copy_document(&tx, "kv", settings_row, &path, bytes, &digest, job)?;
            fs::remove_file(path).map_err(err)?;
            // Updating reports invalidates ownership. Keep all source links on
            // disk and release protection only inside this uncommitted writer.
            tx.execute_batch("CREATE TEMP TABLE admin_ownership AS SELECT * FROM report_source_ownership;
                CREATE TEMP TABLE admin_links AS SELECT * FROM report_source_diagnostic_links;
                DELETE FROM report_source_diagnostic_links;").map_err(err)?;
            let mut previous: Option<i64> = None;
            loop {
                job.check()?;
                let next: Option<i64> = match previous {
                    Some(after) => tx.query_row("SELECT rowid FROM reports WHERE rowid>?1 ORDER BY rowid LIMIT 1", [after], |r| r.get(0)),
                    None => tx.query_row("SELECT rowid FROM reports ORDER BY rowid LIMIT 1", [], |r| r.get(0)),
                }.optional().map_err(err)?;
                let Some(rowid) = next else { break; };
                let disk_tx = disk.unchecked_transaction().map_err(err)?;
                disk.execute_batch("DELETE FROM management_nodes;").map_err(err)?;
                let root = tree::parse(&disk, tx.blob_open("main", "reports", "json", rowid, true).map_err(err)?, job)?;
                rename_report(&disk, root)?;
                let (path, bytes, digest) = stage_document(&stage, &disk, root, job)?;
                disk_tx.commit().map_err(err)?;
                tx.execute("UPDATE reports SET json=zeroblob(?1) WHERE rowid=?2", params![bytes,rowid]).map_err(err)?;
                copy_document(&tx, "reports", rowid, &path, bytes, &digest, job)?;
                fs::remove_file(path).map_err(err)?;
                job.record();
                previous = Some(rowid);
            }
            tx.execute_batch("DELETE FROM report_source_ownership;
                INSERT INTO report_source_ownership SELECT * FROM temp.admin_ownership;
                INSERT INTO report_source_diagnostic_links SELECT * FROM temp.admin_links;
                CREATE TEMP TABLE admin_org_map(old TEXT PRIMARY KEY,new TEXT NOT NULL);").map_err(err)?;
            for (old, new) in old.iter().zip(&names) {
                tx.execute("INSERT INTO temp.admin_org_map VALUES(?1,?2)", params![old,new]).map_err(err)?;
            }
            let changed = tx.execute("UPDATE metric_report_values AS v SET organization=(SELECT new FROM temp.admin_org_map WHERE old=v.organization)
                WHERE EXISTS(SELECT 1 FROM temp.admin_org_map WHERE old=v.organization AND old<>new)", []).map_err(err)?;
            if changed > 0 {
                tx.execute("UPDATE metric_catalog_revision SET revision=revision+1 WHERE singleton=1", []).map_err(err)?;
            }
            job.check()?;
            let revision = read_model::revision(&tx)?;
            job.finish(tx)?;
            Ok(revision)
        })?;
        drop(live);
        drop(disk);
        let cleanup_error = stage.cleanup().err();
        Ok(ManagementReceipt {
            revision,
            deleted: 0,
            cleanup_error,
        })
    }

    /// Caller must show the destructive-action confirmation. Deletion does not
    /// create a backup; Excel source files are never touched.
    pub fn unified_clear_reports(
        &self,
        expected: i64,
        job: &ManagementJob,
    ) -> Result<ManagementReceipt, String> {
        let live = self.management_connect()?;
        job.run(&live, || {
            let tx = live.unchecked_transaction().map_err(err)?;
            tx.execute(
                "UPDATE storage_revision SET revision=revision WHERE singleton=1",
                [],
            )
            .map_err(err)?;
            check_revision(&tx, expected)?;
            check_metadata_bounds(&tx)?;
            check_delete_integrity(&tx)?;
            // All target reports disappear together. No surviving day can lose
            // its diagnostic anchor. Delete triggers release ownership/indexes.
            tx.execute("DELETE FROM report_source_diagnostic_links", [])
                .map_err(err)?;
            let deleted = tx.execute("DELETE FROM reports", []).map_err(err)? as i64;
            tx.execute("DELETE FROM report_source_ownership", [])
                .map_err(err)?;
            job.check()?;
            let revision = read_model::revision(&tx)?;
            job.finish(tx)?;
            Ok(ManagementReceipt {
                revision,
                deleted,
                cleanup_error: None,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fixture(root: &Path) -> Storage {
        let store = Storage::new(root.into()).unwrap();
        store.save(json!({"reports":[
            {"id":"a","type":"day","start":"2026-11-01","end":"2026-11-01","sourceSheet":"Wide","sheetName":"Alias","sourceDiagnosticsReport":"a","sourceDiagnostics":{"sha256":"unchanged","extra":[1,{"organization":"A"}]},"sourceIssues":[{"organization":"A","reason":"source","extra":[1,2]}],"issues":[{"organization":"B","reason":"row"}],"organizations":["A","B"],"totals":[{"name":"A","type":"org","amount":4}],"rows":[{"organization":"A","work":"W","count":1,"amount":4,"unknown":{"organization":"A"}}],"unknown":{"organization":"A","n":[1,2,3]}},
            {"id":"b","type":"day","start":"2026-11-02","end":"2026-11-02","sourceDiagnosticsReport":"a","organizations":["B"],"rows":[{"organization":"B","work":"W","count":2,"amount":8}]}],
            "settings":{"orgNames":["A","B","C","D"],"groups":[{"id":"g","name":"G","works":["W"],"extra":7}],"catalog":[{"organization":"A","work":"W","extra":{"preserve":true}}],"extra":{"organization":"A"}}})).unwrap();
        let c = store.management_connect().unwrap();
        c.execute_batch("INSERT INTO report_source_ownership VALUES('a','wide.xlsx','/untouched','daily-wide'),('b','wide.xlsx','/untouched','daily-wide');
            INSERT INTO report_source_diagnostic_links VALUES('a','a'),('b','a');
            INSERT INTO metric_report_docs VALUES('metric','staff','2026-11-01','2026-11-01','metric.xlsx',1);
            INSERT INTO metric_report_values VALUES('metric',0,'A','x','value',3);").unwrap();
        drop(c);
        store
    }

    fn swap() -> Vec<String> {
        ["B", "A", "C", "D"].into_iter().map(String::from).collect()
    }

    fn links(store: &Storage) -> Vec<(String, String)> {
        let c = store.management_connect().unwrap();
        let mut statement = c
            .prepare(
                "SELECT report_id,anchor_id FROM report_source_diagnostic_links ORDER BY report_id",
            )
            .unwrap();
        let rows = statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        rows
    }

    #[test]
    fn unified_org_swap_retains_c3_links_unknown_fields_and_fresh_backup() {
        let dir = tempdir().unwrap();
        let store = fixture(dir.path());
        let source = dir.path().join("wide.xlsx");
        fs::write(&source, b"original spreadsheet must stay intact").unwrap();
        let before = store.load().unwrap();
        let expected_links = links(&store);
        let revision = store.revision().unwrap();
        let backup = dir.path().join("before.sqlite3");
        let receipt = store
            .unified_rename_organizations(revision, &swap(), &backup, &ManagementJob::new())
            .unwrap();
        assert!(receipt.revision > revision);
        let after = store.load().unwrap();
        assert_eq!(after["settings"]["orgNames"], json!(["B", "A", "C", "D"]));
        assert_eq!(after["settings"]["catalog"][0]["organization"], "B");
        assert_eq!(after["settings"]["groups"], before["settings"]["groups"]);
        let a = &after["reports"][0];
        assert_eq!(a["organizations"], json!(["B", "A"]));
        assert_eq!(a["rows"][0]["organization"], "B");
        assert_eq!(a["issues"][0]["organization"], "A");
        assert_eq!(a["sourceIssues"][0]["organization"], "B");
        assert_eq!(a["totals"][0]["name"], "B");
        assert_eq!(
            a["sourceDiagnostics"],
            before["reports"][0]["sourceDiagnostics"]
        );
        assert_eq!(a["unknown"], before["reports"][0]["unknown"]);
        assert_eq!(
            a["rows"][0]["unknown"],
            before["reports"][0]["rows"][0]["unknown"]
        );
        assert_eq!(a["sheetName"], "Alias");
        assert_eq!(a["sourceDiagnosticsReport"], "a");
        assert_eq!(after["reports"][1]["rows"][0]["organization"], "A");
        assert_eq!(links(&store), expected_links);
        let c = store.management_connect().unwrap();
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM report_source_ownership", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            c.query_row("SELECT organization FROM metric_report_values", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
            "B"
        );
        assert_eq!(
            c.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        let saved = Connection::open(backup).unwrap();
        assert_eq!(read_model::revision(&saved).unwrap(), revision);
        let original: String = saved
            .query_row("SELECT json FROM reports WHERE id='a'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&original).unwrap(),
            before["reports"][0]
        );
        assert_eq!(
            fs::read(source).unwrap(),
            b"original spreadsheet must stay intact"
        );
    }

    #[test]
    fn unified_rename_fresh_database_persists_defaults_after_verified_backup() {
        let dir=tempdir().unwrap();
        let store=Storage::new(dir.path().join("fresh")).unwrap();
        let revision=store.revision().unwrap();
        let names:Vec<String>=["Первое","Второе","Третье","Четвёртое"].into_iter().map(String::from).collect();
        let backup=dir.path().join("before-rename.sqlite3");
        let receipt=store.unified_rename_organizations(revision,&names,&backup,&ManagementJob::new()).unwrap();
        assert!(receipt.revision>revision);
        let data=store.load().unwrap();
        assert_eq!(data["settings"]["orgNames"],json!(names));
        assert_eq!(data["settings"]["groups"],json!([]));
        assert_eq!(data["reports"],json!([]));
        let c=Connection::open(backup).unwrap();
        assert_eq!(c.query_row("SELECT COUNT(*) FROM kv WHERE name='settings'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        assert_eq!(c.query_row("PRAGMA integrity_check",[],|r|r.get::<_,String>(0)).unwrap(),"ok");
    }

    #[test]
    fn unified_clear_all_releases_source_anchors_preserves_settings_metrics_and_excel() {
        let dir = tempdir().unwrap();
        let store = fixture(dir.path());
        let before = store.load().unwrap();
        let source = dir.path().join("wide.xlsx");
        fs::write(&source, b"keep").unwrap();
        let receipt = store
            .unified_clear_reports(
                store.revision().unwrap(),
                &ManagementJob::new(),
            )
            .unwrap();
        assert_eq!(receipt.deleted, 2);
        let after = store.load().unwrap();
        assert_eq!(after["reports"], json!([]));
        assert_eq!(after["settings"], before["settings"]);
        assert!(links(&store).is_empty());
        let c = store.management_connect().unwrap();
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM report_source_ownership", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM metric_report_docs", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            c.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        assert_eq!(fs::read(source).unwrap(), b"keep");
    }

    #[test]
    fn unified_admin_rejects_stale_cancelled_invalid_names_and_existing_backup() {
        let dir = tempdir().unwrap();
        let store = fixture(dir.path());
        let before = store.load().unwrap();
        let revision = store.revision().unwrap();
        let cancelled = ManagementJob::new();
        assert!(cancelled.cancel());
        let backup = dir.path().join("no.sqlite3");
        assert!(store
            .unified_clear_reports(revision, &cancelled)
            .is_err());
        assert!(store
            .unified_clear_reports(revision - 1, &ManagementJob::new())
            .is_err());
        assert!(store
            .unified_rename_organizations(
                revision,
                &["a".into(), " A ".into(), "c".into(), "d".into()],
                &backup,
                &ManagementJob::new()
            )
            .is_err());
        assert!(!backup.exists());
        fs::write(&backup, b"do not overwrite").unwrap();
        assert!(store
            .unified_rename_organizations(revision, &swap(), &backup, &ManagementJob::new())
            .is_err());
        assert_eq!(fs::read(backup).unwrap(), b"do not overwrite");
        assert_eq!(store.load().unwrap(), before);
        assert_eq!(store.revision().unwrap(), revision);
    }

    #[test]
    fn unified_admin_fault_after_first_report_rolls_back_settings_reports_and_links() {
        let dir = tempdir().unwrap();
        let store = fixture(dir.path());
        let before = store.load().unwrap();
        let before_links = links(&store);
        let revision = store.revision().unwrap();
        let c = store.management_connect().unwrap();
        c.execute_batch("CREATE TRIGGER injected_rename_failure BEFORE UPDATE OF json ON reports WHEN OLD.id='b' BEGIN SELECT RAISE(ABORT,'injected publication fault');END;").unwrap();
        drop(c);
        let error = store
            .unified_rename_organizations(
                revision,
                &swap(),
                &dir.path().join("fault.sqlite3"),
                &ManagementJob::new(),
            )
            .unwrap_err();
        assert!(error.contains("injected publication fault"), "{error}");
        assert_eq!(store.load().unwrap(), before);
        assert_eq!(links(&store), before_links);
        assert_eq!(store.revision().unwrap(), revision);
        let c = store.management_connect().unwrap();
        c.execute_batch("DROP TRIGGER injected_rename_failure; CREATE TRIGGER injected_clear_failure BEFORE DELETE ON reports WHEN OLD.id='b' BEGIN SELECT RAISE(ABORT,'injected delete fault');END;").unwrap();
        drop(c);
        assert!(store
            .unified_clear_reports(
                revision,
                &ManagementJob::new()
            )
            .unwrap_err()
            .contains("injected delete fault"));
        assert_eq!(store.load().unwrap(), before);
        assert_eq!(links(&store), before_links);
        assert_eq!(store.revision().unwrap(), revision);
    }

    #[test]
    fn unified_rename_malformed_second_document_cannot_publish_first_document() {
        let dir = tempdir().unwrap();
        let store = fixture(dir.path());
        let c = store.management_connect().unwrap();
        c.execute_batch("DELETE FROM report_source_diagnostic_links; UPDATE reports SET json='{\"rows\":42}' WHERE id='b'; INSERT INTO report_source_diagnostic_links VALUES('a','a'),('b','a');").unwrap();
        drop(c);
        let before = store.load().unwrap();
        let revision = store.revision().unwrap();
        let before_links = links(&store);
        let error = store
            .unified_rename_organizations(
                revision,
                &swap(),
                &dir.path().join("malformed.sqlite3"),
                &ManagementJob::new(),
            )
            .unwrap_err();
        assert!(error.contains("rows must be an array"), "{error}");
        assert_eq!(store.load().unwrap(), before);
        assert_eq!(links(&store), before_links);
        assert_eq!(store.revision().unwrap(), revision);
    }

    #[test]
    fn unified_rename_cancel_during_streaming_preserves_live_documents() {
        let dir = tempdir().unwrap();
        let store = fixture(dir.path());
        // An extension field exceeds a frontend page but is streamed through
        // the disk tree without becoming one giant production Value.
        let c = store.management_connect().unwrap();
        c.execute_batch("DELETE FROM report_source_diagnostic_links;")
            .unwrap();
        let mut document = store.load().unwrap()["reports"][0].clone();
        document["large_extension"] = json!(vec![1; 20000]);
        c.execute(
            "UPDATE reports SET json=?1 WHERE id='a'",
            [document.to_string()],
        )
        .unwrap();
        c.execute_batch("INSERT INTO report_source_diagnostic_links VALUES('a','a'),('b','a');")
            .unwrap();
        drop(c);
        let before = store.load().unwrap();
        let before_links = links(&store);
        let revision = store.revision().unwrap();
        let job = ManagementJob::new();
        std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                store.unified_rename_organizations(
                    revision,
                    &swap(),
                    &dir.path().join("cancel.sqlite3"),
                    &job,
                )
            });
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            while job.records_processed() < 500
                && !handle.is_finished()
                && std::time::Instant::now() < deadline
            {
                std::thread::yield_now();
            }
            assert!(
                job.records_processed() >= 500,
                "rename did not reach streamed report parsing"
            );
            assert!(job.cancel());
            assert!(handle.join().unwrap().is_err());
        });
        assert_eq!(store.load().unwrap(), before);
        assert_eq!(links(&store), before_links);
        assert_eq!(store.revision().unwrap(), revision);
    }
}
