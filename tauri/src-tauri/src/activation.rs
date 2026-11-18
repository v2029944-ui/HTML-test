//! Offline-only database activation. A restore request is prepared in the
//! running app; the next application launch performs the swap before Storage
//! opens SQLite. Keep original and verified safety copies on disk.
use super::*;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::io::{ErrorKind, Write};
use std::time::{SystemTime, UNIX_EPOCH};

const REQUEST: &str = ".analiz-restore-pending.json";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreLaunchResult {
    pub restored: bool,
    pub message: String,
}
pub struct RestoreLaunchInfo(pub Option<RestoreLaunchResult>);

pub struct AppRestoreLock {
    _file: fs::File,
}
impl AppRestoreLock {
    pub fn acquire(root: &Path) -> Result<Self, String> {
        fs::create_dir_all(root).map_err(err)?;
        let path = root.join(".analiz-app-running.lock");
        let file = fs::OpenOptions::new().create(true).read(true).write(true)
            .open(&path).map_err(err)?;
        file.try_lock_exclusive().map_err(|_| {
            "Другой экземпляр приложения уже использует данные. Закройте его перед запуском.".to_string()
        })?;
        Ok(Self { _file: file })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreRequest {
    version: u8,
    stage: String,
    previous: String,
    safety: String,
    digest: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledRestore {
    pub report_count: i64,
    pub restart_required: bool,
}

fn leaf(root: &Path, name: &str, prefix: &str) -> Result<PathBuf, String> {
    if !name.starts_with(prefix)
        || !name.ends_with(".sqlite3")
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || name.contains("..") {
        return Err("Неправильный файл служебного восстановления.".into());
    }
    Ok(root.join(name))
}
fn request_file(root: &Path) -> PathBuf { root.join(REQUEST) }

fn load_request(root: &Path) -> Result<Option<RestoreRequest>, String> {
    let p = request_file(root);
    if !p.exists() { return Ok(None); }
    let meta = fs::symlink_metadata(&p).map_err(err)?;
    if !meta.file_type().is_file() || meta.len() > 4096 {
        return Err("Повреждён запрос на восстановление. Рабочая база не изменена.".into());
    }
    let req: RestoreRequest = serde_json::from_slice(&fs::read(&p).map_err(err)?)
        .map_err(|_| "Не удалось прочитать запрос восстановления. Рабочая база не изменена.".to_string())?;
    if req.version != 1 || req.digest.len() != 64
        || !req.digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Некорректный запрос восстановления. Рабочая база не изменена.".into());
    }
    for (name,prefix) in [
        (&req.stage[..], ".analiz-restore-stage-"),
        (&req.previous[..], ".analiz-restore-previous-"),
        (&req.safety[..], ".analiz-restore-safety-"),
    ] { leaf(root, name, prefix)?; }
    Ok(Some(req))
}

fn ensure_no_wal(path: &Path) -> Result<(), String> {
    if path.exists() {
        // A previous process might have stopped with committed WAL pages.
        // Checkpoint a live file before attempting to move it.
        let conn = Connection::open_with_flags(
            path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
        ).map_err(err)?;
        let (busy, _, _):(i64,i64,i64)=conn.query_row(
            "PRAGMA wal_checkpoint(TRUNCATE)", [],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))
        ).map_err(err)?;
        if busy != 0 {
            return Err("База используется другим процессом. Закройте другие экземпляры программы.".into());
        }
    }
    for extension in ["-wal","-shm"] {
        let mut name = path.as_os_str().to_os_string();
        name.push(extension);
        if fs::symlink_metadata(PathBuf::from(name)).is_ok() {
            return Err("У базы остались WAL-файлы. Восстановление остановлено, чтобы не повредить данные.".into());
        }
    }
    Ok(())
}

fn ensure_regular(path: &Path) -> Result<(), String> {
    if !fs::symlink_metadata(path).map_err(err)?.file_type().is_file() {
        return Err("Файл восстановления не является обычным файлом.".into());
    }
    Ok(())
}

impl Storage {
    /// Create the candidate in the app data directory. This command never
    /// renames, removes or writes to the active reports.sqlite3.
    pub fn schedule_restore(&self, source: &Path, digest: &str) -> Result<ScheduledRestore, String> {
        let root = fs::canonicalize(&self.root).map_err(err)?;
        if fs::symlink_metadata(request_file(&root)).is_ok() {
            return Err("Восстановление уже запланировано. Перезапустите программу перед новым запросом.".into());
        }
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(err)?.as_nanos();
        let tag = format!("{}-{stamp}",std::process::id());
        let req = RestoreRequest {
            version: 1,
            stage: format!(".analiz-restore-stage-{tag}.sqlite3"),
            previous: format!(".analiz-restore-previous-{tag}.sqlite3"),
            safety: format!(".analiz-restore-safety-{tag}.sqlite3"),
            digest: digest.to_owned(),
        };
        let candidate = leaf(&root, &req.stage, ".analiz-restore-stage-")?;
        let copied = self.restore_verified_copy(source, digest, &candidate)?;
        let marker = request_file(&root);
        let result = (|| {
            // Never replace a request concurrently created by another action.
            let mut file = fs::OpenOptions::new().write(true).create_new(true)
                .open(&marker).map_err(err)?;
            file.write_all(&serde_json::to_vec(&req).map_err(err)?).map_err(err)?;
            file.sync_all().map_err(err)?;
            Ok(ScheduledRestore {
                report_count: copied.report_count,
                restart_required: true,
            })
        })();
        if result.is_err() {
            // A partially written marker is retained so the next launch
            // fails safely rather than silently losing a restore request.
            // The verified candidate remains available for recovery.
        }
        result
    }
}

/// The process lock must already be held. Do not run when any Storage
/// connection from this application is open.
pub fn apply_pending_restore(root: &Path) -> Result<Option<RestoreLaunchResult>, String> {
    let Some(req) = load_request(root)? else { return Ok(None); };
    let live = root.join("reports.sqlite3");
    let candidate = leaf(root, &req.stage, ".analiz-restore-stage-")?;
    let previous = leaf(root, &req.previous, ".analiz-restore-previous-")?;
    let safety = leaf(root, &req.safety, ".analiz-restore-safety-")?;

    // Interrupted activation: restore the original without opening or changing
    // the recovered file, and never overwrite the original backup.
    if previous.exists() {
        ensure_regular(&previous)?;
        ensure_no_wal(&live)?;
        if live.exists() {
            ensure_regular(&live)?;
            let orphan = root.join(req.stage.replace(".analiz-restore-stage-", ".analiz-restore-interrupted-"));
            if orphan.exists() {
                return Err("Обнаружены файлы незавершённого восстановления. Восстановление остановлено без перезаписи.".into());
            }
            fs::rename(&live, &orphan).map_err(err)?;
        }
        fs::rename(&previous, &live).map_err(err)?;
        fs::remove_file(request_file(root)).map_err(err)?;
        return Ok(Some(RestoreLaunchResult {
            restored: false,
            message: "Предыдущая попытка восстановления была прервана. Исходная база возвращена; защитная копия сохранена.".into(),
        }));
    }

    ensure_regular(&candidate)?;
    let target = recovery::check_snapshot(&candidate)?;
    if target.verification != req.digest {
        return Err("Подготовленная копия изменилась или повреждена. Рабочая база не затронута.".into());
    }
    ensure_regular(&live)?;
    let current = recovery::check_snapshot(&live)?;
    let space = fs2::available_space(root).map_err(err)?;
    if space < current.bytes.saturating_add(16 * 1024 * 1024) {
        return Err("Для создания защитной копии текущей базы недостаточно места. Восстановление не выполнено.".into());
    }
    if safety.exists() {
        ensure_regular(&safety)?;
        if recovery::check_snapshot(&safety)?.verification != current.verification {
            return Err("Ранее созданная защитная копия отличается от рабочей базы. Переключение остановлено.".into());
        }
    } else {
        let storage = Storage::new(root.to_path_buf())?;
        storage.backup_sqlite_verified(&safety)?;
        if recovery::check_snapshot(&safety)?.verification != current.verification {
            return Err("Защитная копия рабочей базы не прошла сравнение. Восстановление остановлено.".into());
        }
    }
    // Refuse to move the live file with pending WAL pages or any remaining
    // SQLite readers. A previous app version may not honor our process lock.
    ensure_no_wal(&live)?;
    fs::rename(&live, &previous).map_err(err)?;
    let attempt = (|| {
        // A second process that does not use our lock must not create a new
        // live database between the two renames.
        if fs::symlink_metadata(&live).is_ok() {
            return Err("База снова открыта другим процессом. Восстановление отменено.".into());
        }
        fs::rename(&candidate, &live).map_err(err)?;
        let restored = recovery::check_snapshot(&live)?;
        if restored.verification != req.digest {
            return Err("Восстановленная база не совпала с проверенной копией.".into());
        }
        // Confirm current Rust schema can be opened before committing swap.
        let storage = Storage::new(root.to_path_buf())?;
        storage.revision()?;
        fs::remove_file(request_file(root)).map_err(err)?;
        Ok(RestoreLaunchResult {
            restored: true,
            message: format!(
                "Восстановление выполнено: {} отчётов. Предыдущая база сохранена в папке данных.",
                restored.report_count
            ),
        })
    })();
    if attempt.is_err() {
        // Never remove the previous database. If either rename fails, stop
        // startup rather than start an application with ambiguous data.
        ensure_no_wal(&live)?;
        if live.exists() {
            let orphan = root.join(req.stage.replace(".analiz-restore-stage-", ".analiz-restore-interrupted-"));
            fs::rename(&live, &orphan).map_err(err)?;
        }
        fs::rename(&previous, &live).map_err(err)?;
        let _ = fs::remove_file(request_file(root));
    }
    attempt.map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    fn data(amount:f64)->Value {
        json!({"reports":[{"id":"day-a","type":"day","start":"2024-01-01","end":"2024-01-01",
            "rows":[{"work":"Ремонт","organization":"СМУ16","count":1,"amount":amount}]}],
            "settings":{"orgNames":["СМУ16","АВТО","ТРАК","ЦЕНТР"],"groups":[],"catalog":[]}})
    }

    #[test]
    fn schedules_and_restores_on_next_launch_with_rollback_files() {
        let dir=tempdir().unwrap();
        let root=dir.path().join("live");
        let store=Storage::new(root.clone()).unwrap();
        store.save(data(10.0)).unwrap();
        let backup=dir.path().join("source.sqlite3");
        store.backup_sqlite_verified(&backup).unwrap();
        let digest=store.restore_preview(&backup).unwrap().verification;
        let scheduled=store.schedule_restore(&backup,&digest).unwrap();
        assert!(scheduled.restart_required);
        assert_eq!(scheduled.report_count,1);
        store.save(data(20.0)).unwrap();
        assert_eq!(store.load().unwrap(),data(20.0),"scheduling never changes the live db");
        drop(store);
        let _lock=AppRestoreLock::acquire(&root).unwrap();
        let info=apply_pending_restore(&root).unwrap().unwrap();
        assert!(info.restored);
        assert_eq!(Storage::new(root.clone()).unwrap().load().unwrap(),data(10.0));
        assert!(load_request(&root).unwrap().is_none());
        assert_eq!(fs::read_dir(&root).unwrap().filter_map(Result::ok)
            .filter(|p|p.file_name().to_string_lossy().contains("restore-safety-")).count(),1);
    }

    #[test]
    fn corrupted_candidate_blocks_swap_and_keeps_live_database() {
        let dir=tempdir().unwrap();let root=dir.path().join("live");
        let store=Storage::new(root.clone()).unwrap();
        store.save(data(10.0)).unwrap();
        let backup=dir.path().join("source.sqlite3");
        store.backup_sqlite_verified(&backup).unwrap();
        let digest=store.restore_preview(&backup).unwrap().verification;
        store.schedule_restore(&backup,&digest).unwrap();
        let req=load_request(&root).unwrap().unwrap();
        fs::write(root.join(req.stage),b"corrupted candidate").unwrap();
        drop(store);
        let _lock=AppRestoreLock::acquire(&root).unwrap();
        assert!(apply_pending_restore(&root).is_err());
        assert_eq!(Storage::new(root).unwrap().load().unwrap(),data(10.0));
    }

    #[test]
    fn interrupted_move_restores_original_before_app_starts() {
        let dir=tempdir().unwrap();let root=dir.path().join("live");
        let store=Storage::new(root.clone()).unwrap();
        store.save(data(20.0)).unwrap();
        let backup=dir.path().join("source.sqlite3");
        store.backup_sqlite_verified(&backup).unwrap();
        let digest=store.restore_preview(&backup).unwrap().verification;
        store.schedule_restore(&backup,&digest).unwrap();
        let req=load_request(&root).unwrap().unwrap();
        fs::rename(root.join("reports.sqlite3"),root.join(&req.previous)).unwrap();
        drop(store);
        let _lock=AppRestoreLock::acquire(&root).unwrap();
        let result=apply_pending_restore(&root).unwrap().unwrap();
        assert!(!result.restored);
        assert_eq!(Storage::new(root).unwrap().load().unwrap(),data(20.0));
    }

    #[test]
    fn invalid_request_path_never_touches_live_database() {
        let dir=tempdir().unwrap();let root=dir.path().join("live");
        let store=Storage::new(root.clone()).unwrap();store.save(data(2.0)).unwrap();
        fs::write(request_file(&root),
            br#"{"version":1,"stage":"../../evil.sqlite3","previous":"safe.sqlite3","safety":"x.sqlite3","digest":"0000000000000000000000000000000000000000000000000000000000000000"}"#).unwrap();
        drop(store);
        let _lock=AppRestoreLock::acquire(&root).unwrap();
        assert!(apply_pending_restore(&root).is_err());
        assert_eq!(Storage::new(root).unwrap().load().unwrap(),data(2.0));
    }
}
