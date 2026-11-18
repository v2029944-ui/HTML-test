//! Online SQLite backup, safe with WAL and concurrent native writers.
//! A backup is a separate verified SQLite snapshot; never a raw reports.sqlite3 copy.
use super::*;
use rusqlite::{backup::Backup, OpenFlags};
use std::time::{Duration, SystemTime};

impl Storage {
    pub fn backup_sqlite(&self, destination: &Path) -> Result<i64, String> {
        if !destination.is_absolute()
            || !destination.extension().and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("sqlite3"))
        {
            return Err("Choose an absolute backup path ending in .sqlite3".into());
        }
        if fs::symlink_metadata(destination).is_ok() {
            return Err("Backup destination already exists: choose another filename".into());
        }
        let parent = destination.parent().ok_or("Backup destination has no parent")?;
        if !parent.is_dir() { return Err("Backup destination folder does not exist".into()); }
        // Serialize with the old full-snapshot save path; newer native writes
        // are covered by SQLite's online backup snapshot instead.
        let _legacy_guard = self.gate.lock().map_err(err)?;
        let source = self.connect()?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(err)?.as_nanos();
        let mut staging = None;
        for attempt in 0..100 {
            let path = parent.join(format!(
                ".analiz-sqlite-backup-{}-{stamp}-{attempt}.sqlite3",
                std::process::id()
            ));
            match fs::OpenOptions::new().create_new(true).write(true).open(&path) {
                Ok(file) => {
                    drop(file);
                    staging = Some(path);
                    break;
                }
                Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(err(e)),
            }
        }
        let tmp = staging.ok_or("Could not reserve private SQLite backup file")?;
        let result = (|| {
            let mut target = Connection::open(&tmp).map_err(err)?;
            {
                let backup = Backup::new(&source, &mut target).map_err(err)?;
                backup.run_to_completion(32, Duration::from_millis(10), None).map_err(err)?;
            }
            drop(target);
            let check = Connection::open_with_flags(&tmp, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;
            let quick: String = check.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(err)?;
            if quick != "ok" { return Err("SQLite backup failed quick_check".into()); }
            // Verify the backed-up revision, not a later source revision.
            let revision = read_model::revision(&check)?;
            drop(check);
            fs::OpenOptions::new().read(true).write(true).open(&tmp).map_err(err)?
                .sync_all().map_err(err)?;
            // Atomic, no-overwrite publish, including cross-process races.
            fs::hard_link(&tmp, destination).map_err(err)?;
            Ok(revision)
        })();
        let _ = fs::remove_file(&tmp);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fixture() -> Value {
        json!({"reports":[{
            "id":"backup-day","type":"day","start":"2024-02-29","end":"2024-02-29",
            "rows":[{"work":"Ремонт","organization":"СМУ16","count":2.5,"amount":350.25}]
        }],"settings":{
            "orgNames":["СМУ16","АВТО","ТРАК","ЦЕНТР"],
            "groups":[],"catalog":[]
        }})
    }

    #[test]
    fn sqlite_online_backup_preserves_blob_json_and_settings_without_modifying_source() {
        let dir = tempdir().unwrap();
        let source = Storage::new(dir.path().join("live")).unwrap();
        let expected = fixture();
        source.save(expected.clone()).unwrap();
        let conn = source.connect().unwrap();
        // Verify backup of reports encoded as BLOB like new C3 wide imports,
        // not just legacy TEXT rows.
        conn.execute("UPDATE reports SET json=CAST(json AS BLOB) WHERE id='backup-day'", []).unwrap();
        let path = dir.path().join("snapshot.sqlite3");
        let revision = source.backup_sqlite(&path).unwrap();
        assert_eq!(source.revision().unwrap(), revision);
        let backup = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let integrity: String = backup.query_row("PRAGMA quick_check", [], |r| r.get(0)).unwrap();
        assert_eq!(integrity, "ok");
        let source_json: Vec<u8> = backup.query_row(
            "SELECT json FROM reports WHERE id='backup-day'", [], |r| r.get(0)
        ).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&source_json).unwrap(), expected["reports"][0]);
        let settings: String = backup.query_row("SELECT json FROM kv WHERE name='settings'", [], |r| r.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&settings).unwrap(), expected["settings"]);
        assert_eq!(source.load().unwrap(), expected);
    }

    #[test]
    fn sqlite_backup_never_replaces_existing_file_or_accepts_relative_paths() {
        let dir = tempdir().unwrap();
        let store = Storage::new(dir.path().join("live")).unwrap();
        store.save(fixture()).unwrap();
        let backup = dir.path().join("snapshot.sqlite3");
        fs::write(&backup, b"existing backup").unwrap();
        assert!(store.backup_sqlite(&backup).unwrap_err().contains("already exists"));
        assert_eq!(fs::read(&backup).unwrap(), b"existing backup");
        assert!(store.backup_sqlite(Path::new("relative.sqlite3")).is_err());
        assert!(store.backup_sqlite(&dir.path().join("report.json")).is_err());
    }
}
