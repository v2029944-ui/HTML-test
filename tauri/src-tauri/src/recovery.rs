//! Non-destructive recovery rehearsal: validate a user-selected backup and
//! publish a verified independent SQLite copy. Never touch the live database.
use super::*;
use rusqlite::{types::ValueRef, OpenFlags};
use serde::de::IgnoredAny;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    pub report_count: i64,
    pub bytes: u64,
    pub verification: String,
}

fn validate_backup(store: &Storage, source: &Path) -> Result<PathBuf, String> {
    if !source.is_absolute() || !source.extension().and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("sqlite3")) {
        return Err("Выберите резервную копию с расширением .sqlite3.".into());
    }
    let meta = fs::symlink_metadata(source).map_err(err)?;
    if !meta.file_type().is_file() {
        return Err("Резервная копия должна быть обычным файлом, не ссылкой.".into());
    }
    let original = fs::canonicalize(source).map_err(err)?;
    if store.db_path().exists() && original == fs::canonicalize(store.db_path()).map_err(err)? {
        return Err("Рабочую базу нельзя выбирать как резервную копию.".into());
    }
    Ok(original)
}

pub(super) fn check_snapshot(path: &Path) -> Result<RestorePreview, String> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;
    // Keep integrity, schema, and every table in the same read snapshot. A
    // backup may have WAL writers while the user inspects it.
    conn.execute_batch("BEGIN").map_err(err)?;
    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(err)?;
    if integrity != "ok" {
        return Err("Резервная копия повреждена: проверка целостности SQLite не пройдена.".into());
    }
    let tables: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('reports','kv')",
        [], |row| row.get(0)
    ).map_err(err)?;
    if tables != 2 {
        return Err("Файл не является базой «Анализ работ»: отсутствует reports или kv.".into());
    }
    let mut hasher = Sha256::new();
    let mut schema = conn.prepare(
        "SELECT type,name,tbl_name,coalesce(sql,'') FROM sqlite_master ORDER BY type,name,tbl_name"
    ).map_err(err)?;
    let mut schema_rows = schema.query([]).map_err(err)?;
    while let Some(row) = schema_rows.next().map_err(err)? {
        for index in 0..4 {
            let value: String = row.get(index).map_err(err)?;
            hasher.update((value.len() as u64).to_le_bytes());
            hasher.update(value.as_bytes());
        }
    }
    drop(schema_rows);
    drop(schema);
    let mut report_count = 0i64;
    for (sql, reports) in [
        ("SELECT id,json FROM reports ORDER BY id", true),
        ("SELECT name,json FROM kv ORDER BY name", false),
    ] {
        let mut stmt = conn.prepare(sql).map_err(err)?;
        let mut rows = stmt.query([]).map_err(err)?;
        hasher.update(sql.as_bytes());
        while let Some(row) = rows.next().map_err(err)? {
            let name: String = row.get(0).map_err(err)?;
            let raw = row.get_ref(1).map_err(err)?;
            let bytes = match raw {
                ValueRef::Text(bytes) | ValueRef::Blob(bytes) => bytes,
                _ => return Err("Обнаружено повреждённое JSON-поле резервной копии.".into()),
            };
            serde_json::from_slice::<IgnoredAny>(bytes).map_err(|_| {
                "В резервной копии обнаружен некорректный JSON отчёта или настроек.".to_string()
            })?;
            if reports && bytes.iter().copied().find(|b| !b.is_ascii_whitespace()) != Some(b'{') {
                return Err("Некорректная запись отчёта в резервной копии.".into());
            }
            hasher.update((name.len() as u64).to_le_bytes());
            hasher.update(name.as_bytes());
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
            if reports { report_count += 1; }
        }
    }
    // Hash the remaining application tables as well. Previous versions only
    // verified reports and kv, which would miss changes to metric reports,
    // management caches, import receipts and other persisted analytics data.
    let mut names = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type='table'
         AND name NOT LIKE 'sqlite_%' AND name NOT IN ('reports','kv')
         ORDER BY name"
    ).map_err(err)?;
    let mut table_rows = names.query([]).map_err(err)?;
    let mut table_names = Vec::new();
    while let Some(row) = table_rows.next().map_err(err)? {
        if table_names.len() >= 256 {
            return Err("В копии слишком много таблиц для безопасной проверки.".into());
        }
        let name: String = row.get(0).map_err(err)?;
        if name.len() > 1024 {
            return Err("Имя таблицы SQLite слишком длинное.".into());
        }
        table_names.push(name);
    }
    drop(table_rows);
    drop(names);
    for table in table_names {
        hasher.update(b"\0additional-table\0");
        hasher.update((table.len() as u64).to_le_bytes());
        hasher.update(table.as_bytes());
        let quoted = table.replace('"', "\"\"");
        let sql = format!("SELECT * FROM \"{quoted}\"");
        let mut statement = conn.prepare(&sql).map_err(err)?;
        let fields = statement.column_count();
        hasher.update((fields as u64).to_le_bytes());
        let mut rows = statement.query([]).map_err(err)?;
        while let Some(row) = rows.next().map_err(err)? {
            hasher.update(b"\0row\0");
            for index in 0..fields {
                match row.get_ref(index).map_err(err)? {
                    ValueRef::Null => hasher.update([0]),
                    ValueRef::Integer(value) => {
                        hasher.update([1]);
                        hasher.update(value.to_le_bytes());
                    }
                    ValueRef::Real(value) => {
                        hasher.update([2]);
                        hasher.update(value.to_bits().to_le_bytes());
                    }
                    ValueRef::Text(bytes) => {
                        hasher.update([3]);
                        hasher.update((bytes.len() as u64).to_le_bytes());
                        hasher.update(bytes);
                    }
                    ValueRef::Blob(bytes) => {
                        hasher.update([4]);
                        hasher.update((bytes.len() as u64).to_le_bytes());
                        hasher.update(bytes);
                    }
                }
            }
        }
    }
    let mut fk = conn.prepare("PRAGMA foreign_key_check").map_err(err)?;
    if fk.query([]).map_err(err)?.next().map_err(err)?.is_some() {
        return Err("В резервной копии нарушены связи между таблицами.".into());
    }
    let page_count: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))
        .map_err(err)?;
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))
        .map_err(err)?;
    let bytes = u64::try_from(page_count).map_err(err)?
        .checked_mul(u64::try_from(page_size).map_err(err)?)
        .ok_or("Слишком большой размер SQLite-копии.")?;
    Ok(RestorePreview {
        report_count,
        bytes,
        verification: format!("{:x}", hasher.finalize()),
    })
}

impl Storage {
    /// Read-only verification; no SQLite tables or files are changed.
    pub fn restore_preview(&self, source: &Path) -> Result<RestorePreview, String> {
        let source = validate_backup(self, source)?;
        check_snapshot(&source)
    }

    /// Create an independent copy, never overwriting any file.
    /// The caller must have explicitly reviewed the current source digest.
    pub fn restore_verified_copy(
        &self, source: &Path, expected: &str, destination: &Path
    ) -> Result<RestorePreview, String> {
        if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Повторите проверку выбранной резервной копии.".into());
        }
        let source = validate_backup(self, source)?;
        if !destination.is_absolute() || !destination.extension().and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("sqlite3")) {
            return Err("Укажите новый файл с расширением .sqlite3.".into());
        }
        if fs::symlink_metadata(destination).is_ok() {
            return Err("Файл назначения уже существует. Выберите другое имя.".into());
        }
        let parent = destination.parent().ok_or("Не найдена папка назначения.")?;
        if !parent.is_dir() { return Err("Папка назначения не существует.".into()); }
        let canonical_parent = fs::canonicalize(parent).map_err(err)?;
        let output = canonical_parent.join(destination.file_name().ok_or("Недопустимое имя файла.")?);
        if output == self.db_path() || output == source {
            return Err("Нельзя перезаписывать рабочую базу или исходную копию.".into());
        }
        let _guard = self.gate.lock().map_err(err)?;
        let current = check_snapshot(&source)?;
        if current.verification != expected {
            return Err("Резервная копия изменилась после проверки. Проверьте её заново.".into());
        }
        // The source can have uncheckpointed WAL pages; use SQLite's logical
        // size rather than file metadata and leave headroom for staging.
        let required = current.bytes.saturating_add(16 * 1024 * 1024);
        if fs2::available_space(parent).map_err(err)? < required {
            return Err("Недостаточно свободного места для восстановления. Освободите место на диске.".into());
        }
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(err)?.as_nanos();
        let mut temporary = None;
        for attempt in 0..100 {
            let path = parent.join(format!(
                ".analiz-restore-check-{}-{stamp}-{attempt}.sqlite3", std::process::id()
            ));
            match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => { drop(file); temporary = Some(path); break; }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(err(e)),
            }
        }
        let temporary = temporary.ok_or("Не удалось создать временный файл восстановления.")?;
        let result = (|| {
            {
                let source_conn = Connection::open_with_flags(
                    &source, OpenFlags::SQLITE_OPEN_READ_ONLY
                ).map_err(err)?;
                let mut target = Connection::open(&temporary).map_err(err)?;
                rusqlite::backup::Backup::new(&source_conn, &mut target).map_err(err)?
                    .run_to_completion(64, Duration::from_millis(10), None).map_err(err)?;
            }
            {
                let stage = Connection::open(&temporary).map_err(err)?;
                stage.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").map_err(err)?;
                let mode: String = stage.query_row(
                    "PRAGMA journal_mode=DELETE", [], |r| r.get(0)
                ).map_err(err)?;
                if mode.to_lowercase() != "delete" {
                    return Err("Не удалось завершить восстановленную базу без WAL.".into());
                }
            }
            let result = check_snapshot(&temporary)?;
            if result.verification != expected || result.report_count != current.report_count {
                return Err("Восстановленная копия не совпадает с проверенной резервной копией.".into());
            }
            fs::OpenOptions::new().read(true).write(true).open(&temporary)
                .map_err(err)?.sync_all().map_err(err)?;
            fs::hard_link(&temporary, &output).map_err(err)?;
            Ok(result)
        })();
        let _ = fs::remove_file(&temporary);
        for suffix in ["-wal", "-shm"] {
            let mut name = temporary.as_os_str().to_os_string();
            name.push(suffix);
            let _ = fs::remove_file(PathBuf::from(name));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample() -> Value {
        json!({"reports":[{"id":"restore-a","type":"day","start":"2024-02-29",
            "end":"2024-02-29","rows":[{"work":"Ремонт","organization":"СМУ16",
            "count":1.25,"amount":30.5}]}],
            "settings":{"orgNames":["СМУ16","АВТО","ТРАК","ЦЕНТР"],
                "groups":[],"catalog":[]}})
    }
    #[test]
    fn restore_rehearsal_preserves_original_and_recovers_reports() {
        let dir = tempdir().unwrap();
        let live = Storage::new(dir.path().join("live")).unwrap();
        let input = sample();
        live.save(input.clone()).unwrap();
        let source = dir.path().join("source.sqlite3");
        live.backup_sqlite_verified(&source).unwrap();
        let preview = live.restore_preview(&source).unwrap();
        assert_eq!(preview.report_count, 1);
        let output = dir.path().join("restored.sqlite3");
        let restored = live.restore_verified_copy(&source, &preview.verification, &output).unwrap();
        assert_eq!(restored.verification, preview.verification);
        assert_eq!(live.load().unwrap(), input);
        assert!(output.exists());
        assert_eq!(check_snapshot(&output).unwrap().verification, preview.verification);
        assert!(live.restore_verified_copy(&source, &preview.verification, &output).is_err());
    }
    #[test]
    fn restore_with_open_wal_source_includes_committed_wal_data() {
        let dir = tempdir().unwrap();
        let live = Storage::new(dir.path().join("live")).unwrap();
        live.save(sample()).unwrap();
        let source = dir.path().join("wal-source.sqlite3");
        live.backup_sqlite_verified(&source).unwrap();
        let writer = Connection::open(&source).unwrap();
        writer.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        writer.execute(
            "INSERT INTO kv(name,json) VALUES('extra-recovery','{\"from_wal\":true}')", []
        ).unwrap();
        let review = live.restore_preview(&source).unwrap();
        let output = dir.path().join("wal-restored.sqlite3");
        live.restore_verified_copy(&source, &review.verification, &output).unwrap();
        let copy = Connection::open_with_flags(&output, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let value: String = copy.query_row(
            "SELECT json FROM kv WHERE name='extra-recovery'", [], |r| r.get(0)
        ).unwrap();
        assert_eq!(value, "{\"from_wal\":true}");
        assert_eq!(live.load().unwrap(), sample());
    }
    #[test]
    fn metric_table_change_invalidates_review_and_prevents_stale_restore() {
        let dir = tempdir().unwrap();
        let live = Storage::new(dir.path().join("live")).unwrap();
        live.save(sample()).unwrap();
        let source = dir.path().join("metric-backup.sqlite3");
        live.backup_sqlite_verified(&source).unwrap();
        let preview = live.restore_preview(&source).unwrap();
        let writer = Connection::open(&source).unwrap();
        writer.execute(
            "UPDATE metric_catalog_revision SET revision=revision+1 WHERE singleton=1", []
        ).unwrap();
        drop(writer);
        let updated = live.restore_preview(&source).unwrap();
        assert_ne!(preview.verification, updated.verification,
            "Every stored metric family must be covered by the verified token");
        let target = dir.path().join("stale-metrics.sqlite3");
        assert!(live.restore_verified_copy(
            &source, &preview.verification, &target
        ).unwrap_err().contains("изменилась"));
        assert!(!target.exists());
    }

    #[test]
    fn restore_rejects_malformed_json_and_backup_changed_after_preview() {
        let dir = tempdir().unwrap();
        let live = Storage::new(dir.path().join("live")).unwrap();
        live.save(sample()).unwrap();
        let source = dir.path().join("backup.sqlite3");
        live.backup_sqlite_verified(&source).unwrap();
        let preview = live.restore_preview(&source).unwrap();
        let writer = Connection::open(&source).unwrap();
        writer.execute(
            "UPDATE kv SET json='{\"changed\":true}' WHERE name='settings'", []
        ).unwrap();
        let destination = dir.path().join("stale.sqlite3");
        assert!(live.restore_verified_copy(
            &source, &preview.verification, &destination
        ).unwrap_err().contains("изменилась"));
        assert!(!destination.exists());
        writer.execute("UPDATE reports SET json='not-json' WHERE id='restore-a'", [])
            .unwrap();
        assert!(live.restore_preview(&source).unwrap_err().contains("JSON"));
        assert_eq!(live.load().unwrap(), sample());
    }
    #[test]
    fn restore_rejects_invalid_sources_and_stale_review() {
        let dir = tempdir().unwrap();
        let live = Storage::new(dir.path().join("live")).unwrap();
        live.save(sample()).unwrap();
        assert!(live.restore_preview(&live.db_path()).is_err());
        let invalid = dir.path().join("corrupt.sqlite3");
        fs::write(&invalid, b"not a sqlite database").unwrap();
        assert!(live.restore_preview(&invalid).is_err());
        let source = dir.path().join("backup.sqlite3");
        live.backup_sqlite_verified(&source).unwrap();
        assert!(live.restore_verified_copy(
            &source, &"0".repeat(64), &dir.path().join("no-copy.sqlite3")
        ).is_err());
        assert!(!dir.path().join("no-copy.sqlite3").exists());
        assert!(live.restore_verified_copy(
            &source, &"0".repeat(64), &live.db_path()
        ).is_err());
    }
}
