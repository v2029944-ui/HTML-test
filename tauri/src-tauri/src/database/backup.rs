//! Online SQLite backup, safe with WAL and concurrent native writers.
//! A backup is a separate verified SQLite snapshot; never a raw reports.sqlite3 copy.
use super::*;
use rusqlite::OpenFlags;
use std::time::SystemTime;

impl Storage {
    /// Additional integrity checks for the existing bounded Online Backup.
    /// The D1 backup_sqlite method owns the SQLite snapshot and staged
    /// no-clobber publication; we verify its private output before exposing it.
    pub fn backup_sqlite_verified(&self, destination: &Path) -> Result<i64, String> {
        if !destination.is_absolute()
            || !destination.extension().and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("sqlite3")) {
            return Err("Choose an absolute backup path ending in .sqlite3".into());
        }
        if fs::symlink_metadata(destination).is_ok() {
            return Err("Backup destination already exists".into());
        }
        let parent = destination.parent().ok_or("Backup destination has no parent")?;
        if !parent.is_dir() { return Err("Backup folder does not exist".into()); }
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(err)?.as_nanos();
        // D1 backup_sqlite creates its own private staging file and links the
        // output atomically. Give it another unique private leaf, then validate
        // that *completed* snapshot before publishing the user-named destination.
        let mut owned = None;
        for attempt in 0..100 {
            let path = parent.join(format!(
                ".analiz-verified-backup-{}-{stamp}-{attempt}.sqlite3",
                std::process::id()
            ));
            if fs::symlink_metadata(&path).is_ok() { continue; }
            self.backup_sqlite(&path)?;
            owned = Some(path);
            break;
        }
        let temporary = owned.ok_or("Unable to reserve a private backup filename")?;
        let result = (|| {
            let check = Connection::open_with_flags(&temporary, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(err)?;
            let quick: String = check.query_row("PRAGMA quick_check", [], |row| row.get(0))
                .map_err(err)?;
            if quick != "ok" { return Err("SQLite backup failed quick_check".into()); }
            let revision = read_model::revision(&check)?;
            drop(check);
            fs::OpenOptions::new().read(true).write(true).open(&temporary)
                .map_err(err)?.sync_all().map_err(err)?;
            // Atomic publication, never overwrites a name created concurrently.
            fs::hard_link(&temporary, destination).map_err(err)?;
            Ok(revision)
        })();
        let _ = fs::remove_file(&temporary);
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
        let revision = source.backup_sqlite_verified(&path).unwrap();
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
        assert!(store.backup_sqlite_verified(&backup).unwrap_err().contains("already exists"));
        assert_eq!(fs::read(&backup).unwrap(), b"existing backup");
        assert!(store.backup_sqlite_verified(Path::new("relative.sqlite3")).is_err());
        assert!(store.backup_sqlite_verified(&dir.path().join("report.json")).is_err());
    }
}
