//! Per-workspace local backup schedule for the running desktop application.
use super::*;
use fs2::FileExt;
use std::io;
use std::time::SystemTime;

const DAY_MS: u64 = 24 * 60 * 60 * 1_000;
const RETAIN: usize = 7;

#[derive(Default, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledBackupStatus {
    enabled: bool,
    last_success: Option<u64>,
    last_path: Option<PathBuf>,
    error: Option<String>,
}

// Only operate in real descendants of the application's private local directory.
fn child_directory(parent: &Path, name: &str, create: bool) -> Result<Option<PathBuf>, String> {
    let path = parent.join(name);
    if create {
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(err(error)),
        }
    }
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(err(error)),
    };
    if !metadata.file_type().is_dir() || fs::canonicalize(&path).map_err(err)? != path {
        return Err("Папка ежедневных копий должна находиться в локальной папке приложения.".into());
    }
    Ok(Some(path))
}

fn scheduled_files(directory: &Path) -> Result<Vec<(u64, PathBuf)>, String> {
    let mut backups = Vec::new();
    for entry in fs::read_dir(directory).map_err(err)? {
        let entry = entry.map_err(err)?;
        let name = entry.file_name();
        let Some(stamp) = name.to_str().and_then(|name| name.strip_prefix("daily-")?.strip_suffix(".sqlite3")) else {
            continue;
        };
        if stamp.len() != 20 || !stamp.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if let Ok(timestamp) = stamp.parse::<u64>() {
            if entry.file_type().map_err(err)?.is_file() {
                backups.push((timestamp, entry.path()));
            }
        }
    }
    backups.sort_unstable_by_key(|(stamp, _)| std::cmp::Reverse(*stamp));
    Ok(backups)
}

fn lock_schedule(directory: &Path) -> Result<fs::File, String> {
    let path = directory.join(".schedule.lock");
    // Never create an outside file through a pre-existing symbolic link.
    let file = match fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            fs::OpenOptions::new().read(true).write(true).open(&path).map_err(err)?
        }
        Err(error) => return Err(err(error)),
    };
    if !fs::symlink_metadata(&path).map_err(err)?.file_type().is_file()
        || fs::canonicalize(&path).map_err(err)? != path
    {
        return Err("Файл блокировки ежедневных копий должен находиться в папке копий.".into());
    }
    FileExt::try_lock_exclusive(&file).map_err(|error| {
        if error.kind() == io::ErrorKind::WouldBlock || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() {
            "Другая операция с ежедневной копией ещё выполняется. Повторите позже.".into()
        } else {
            err(error)
        }
    })?;
    Ok(file)
}

fn rotate(backups: &[(u64, PathBuf)]) -> Result<(), String> {
    for (_, path) in backups.iter().skip(RETAIN) {
        if !fs::symlink_metadata(path).map_err(err)?.file_type().is_file() {
            return Err("Старая ежедневная копия больше не является обычным файлом.".into());
        }
        fs::remove_file(path).map_err(err)?;
    }
    Ok(())
}

fn schedule_enabled(directory: &Path, choice: Option<bool>) -> Result<bool, String> {
    let path = directory.join("enabled");
    let was_enabled = match fs::symlink_metadata(&path) {
        Ok(file) if file.file_type().is_file() && file.len() == 0 => true,
        Ok(_) => return Err("Локальная настройка ежедневных копий повреждена.".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(err(error)),
    };
    match choice {
        Some(true) if !was_enabled => {
            let marker = tempfile::NamedTempFile::new_in(directory).map_err(err)?;
            marker.as_file().sync_all().map_err(err)?;
            marker.persist_noclobber(path).map_err(|error| err(error.error))?;
            Ok(true)
        }
        Some(false) if was_enabled => {
            fs::remove_file(path).map_err(err)?;
            Ok(false)
        }
        _ => Ok(was_enabled),
    }
}

impl Storage {
    pub(super) fn scheduled_backup(
        &self, root: &Path, namespace: &str, choice: Option<bool>,
    ) -> Result<ScheduledBackupStatus, String> {
        let now = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(err)?.as_millis())
            .map_err(err)?;
        self.scheduled_backup_at(root, namespace, choice, now, |path| fs2::available_space(path))
    }

    fn scheduled_backup_at(
        &self, root: &Path, namespace: &str, choice: Option<bool>, now: u64,
        available: impl FnOnce(&Path) -> io::Result<u64>,
    ) -> Result<ScheduledBackupStatus, String> {
        let root = fs::canonicalize(root).map_err(err)?;
        let create = choice == Some(true);
        let Some(parent) = child_directory(&root, "scheduled-backups", create)? else {
            return Ok(ScheduledBackupStatus::default());
        };
        let Some(directory) = child_directory(&parent, namespace, create)? else {
            return Ok(ScheduledBackupStatus::default());
        };
        // A second local runtime may use this private cache directory. Hold the
        // lock across selecting the due time, publication, and rotation.
        let _lock = lock_schedule(&directory)?;
        let enabled = schedule_enabled(&directory, choice)?;
        let mut backups = scheduled_files(&directory)?;
        let mut status = ScheduledBackupStatus {
            enabled,
            last_success: backups.first().map(|(stamp, _)| *stamp),
            last_path: backups.first().map(|(_, path)| path.clone()),
            error: None,
        };
        if !enabled { return Ok(status); }

        let result = (|| {
            // Retry a previously interrupted rotation before adding more files.
            rotate(&backups)?;
            backups.truncate(RETAIN);
            if status.last_success.is_some_and(|last| now.saturating_sub(last) < DAY_MS) {
                return Ok(());
            }
            // Use SQLite's logical size, including WAL pages, for the preflight.
            let db = self.connect()?;
            let pages: u64 = db.query_row("PRAGMA page_count", [], |r| r.get(0)).map_err(err)?;
            let size: u64 = db.query_row("PRAGMA page_size", [], |r| r.get(0)).map_err(err)?;
            drop(db);
            let required = pages.checked_mul(size).ok_or("Слишком большой размер SQLite.")?;
            if available(&directory).map_err(err)? < required {
                return Err("Недостаточно свободного места для ежедневной копии SQLite.".into());
            }
            let destination = directory.join(format!("daily-{now:020}.sqlite3"));
            self.backup_sqlite_verified(&destination)?;
            status.last_success = Some(now);
            status.last_path = Some(destination.clone());
            // Do not retire any retained copy until its replacement is verified.
            backups.insert(0, (now, destination));
            rotate(&backups)
        })();
        status.error = result.err();
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const START: u64 = 1_700_000_000_000;

    fn fixture() -> Value {
        json!({"reports":[{"id":"saved-day","type":"day","start":"2024-02-29",
            "end":"2024-02-29","rows":[{"work":"Ремонт","organization":"СМУ16",
            "count":1.25,"amount":30.5}]}],
            "settings":{"orgNames":["СМУ16","АВТО","ТРАК","ЦЕНТР"],
                "groups":[],"catalog":[]}})
    }

    #[test]
    fn schedule_is_opt_in_rotates_only_daily_backups_and_restores_to_a_new_file() {
        let dir = tempdir().unwrap();
        let store = Storage::new(dir.path().join("live")).unwrap();
        store.save(fixture()).unwrap();
        let revision = store.revision().unwrap();
        let tick = |choice, now| store.scheduled_backup_at(dir.path(), "personal", choice, now, |_| Ok(u64::MAX)).unwrap();
        let disabled = tick(None, START);
        assert!(!disabled.enabled && disabled.last_success.is_none());
        assert!(!dir.path().join("scheduled-backups").exists());

        let first = tick(Some(true), START);
        assert!(first.enabled && first.error.is_none());
        assert_eq!(first.last_success, Some(START));
        let first_file = first.last_path.unwrap();
        let folder = first_file.parent().unwrap();
        let manual = folder.join("manual-backup.sqlite3");
        store.backup_sqlite_verified(&manual).unwrap();
        assert_eq!(tick(None, START + DAY_MS - 1).last_success, Some(START));
        assert_eq!(scheduled_files(folder).unwrap().len(), 1);

        let mut recent = first_file.clone();
        for day in 1..=RETAIN as u64 {
            let time = START + day * DAY_MS;
            let next = tick(None, time);
            assert!(next.error.is_none(), "{:?}", next.error);
            assert_eq!(next.last_success, Some(time));
            recent = next.last_path.unwrap();
        }
        assert_eq!(scheduled_files(folder).unwrap().len(), RETAIN);
        assert!(!first_file.exists());
        assert!(manual.is_file());
        let review = store.restore_preview(&recent).unwrap();
        assert_eq!(review.report_count, 1);
        let restored = dir.path().join("restored.sqlite3");
        let copied = store.restore_verified_copy(&recent, &review.verification, &restored).unwrap();
        assert_eq!(copied.verification, review.verification);
        assert_eq!(store.revision().unwrap(), revision);
        assert_eq!(store.load().unwrap(), fixture());

        let stopped = tick(Some(false), START + 9 * DAY_MS);
        assert!(!stopped.enabled);
        assert_eq!(stopped.last_path.as_ref(), Some(&recent));
        let reopened = Storage::new(dir.path().join("live")).unwrap();
        let state = reopened.scheduled_backup(dir.path(), "personal", None).unwrap();
        assert!(!state.enabled);
        assert_eq!(state.last_path.as_ref(), Some(&recent));
    }

    #[test]
    fn full_disk_and_a_blocked_destination_preserve_the_previous_verified_copy() {
        let dir = tempdir().unwrap();
        let store = Storage::new(dir.path().join("live")).unwrap();
        store.save(fixture()).unwrap();
        let tick = |choice, now, free| store.scheduled_backup_at(dir.path(), "personal", choice, now, |_| Ok(free)).unwrap();
        let first = tick(Some(true), START, u64::MAX).last_path.unwrap();
        let folder = first.parent().unwrap();
        let prior = fs::read(&first).unwrap();
        let next_time = START + DAY_MS;
        let full = tick(None, next_time, 0);
        assert!(full.enabled && full.error.unwrap().contains("Недостаточно свободного места"));
        assert_eq!(full.last_success, Some(START));
        assert_eq!(fs::read(&first).unwrap(), prior);
        assert_eq!(scheduled_files(folder).unwrap().len(), 1);

        let blocked = folder.join(format!("daily-{next_time:020}.sqlite3"));
        fs::create_dir(&blocked).unwrap();
        let denied = tick(None, next_time, u64::MAX);
        assert!(denied.error.unwrap().contains("destination already exists"));
        assert_eq!(denied.last_path.as_ref(), Some(&first));
        assert!(blocked.is_dir());
        assert_eq!(fs::read(&first).unwrap(), prior);
        assert_eq!(store.load().unwrap(), fixture());
        assert_eq!(scheduled_files(folder).unwrap().len(), 1);
    }

    #[test]
    fn runtime_uses_a_distinct_local_schedule_for_each_selected_database() {
        use super::shared_runtime::{Access, Runtime};
        let dir = tempdir().unwrap();
        let runtime = Runtime::new(dir.path().join("live")).unwrap();
        let personal = runtime.status().unwrap();
        runtime.execute(Some(&personal.workspace_token), Access::Write, |store| store.save(fixture())).unwrap();
        let personal_backup = runtime.scheduled_backup(Some(&personal.workspace_token), Some(true)).unwrap();
        let personal_path = personal_backup.last_path.unwrap();
        let server = dir.path().join("server");
        fs::create_dir(&server).unwrap();
        runtime.grant(server.clone()).unwrap();
        let shared = runtime.connect(&server).unwrap();
        assert!(runtime.scheduled_backup(Some(&personal.workspace_token), Some(true)).is_err());
        let initial = runtime.scheduled_backup(Some(&shared.workspace_token), None).unwrap();
        assert!(!initial.enabled && initial.last_path.is_none());
        let before = fs::read_dir(&server).unwrap().map(|entry| entry.unwrap().file_name()).collect::<std::collections::BTreeSet<_>>();
        let shared_backup = runtime.scheduled_backup(Some(&shared.workspace_token), Some(true)).unwrap();
        let shared_path = shared_backup.last_path.unwrap();
        assert_ne!(personal_path.parent(), shared_path.parent());
        assert_eq!(before,fs::read_dir(&server).unwrap().map(|entry| entry.unwrap().file_name()).collect());
        let personal = runtime.disconnect(Some(&shared.workspace_token)).unwrap();
        assert_eq!(runtime.scheduled_backup(Some(&personal.workspace_token), None).unwrap().last_path, Some(personal_path));
    }

    #[test]
    fn concurrent_local_requests_create_at_most_one_due_copy_and_limit_rotation() {
        let dir = tempdir().unwrap();
        let live = dir.path().join("live");
        let first = Storage::new(live.clone()).unwrap();
        let second = Storage::new(live).unwrap();
        first.save(fixture()).unwrap();
        let root = dir.path();
        let backup = |store: &Storage, choice, time| {
            store.scheduled_backup_at(root,"personal",choice,time, |_| Ok(u64::MAX))
        };
        let output = backup(&first,Some(true),START).unwrap().last_path.unwrap();
        for day in 1..RETAIN as u64 {
            let result=backup(&first,None,START + day * DAY_MS).unwrap();
            assert!(result.error.is_none());
        }
        let due = START + RETAIN as u64 * DAY_MS;
        let directory = output.parent().unwrap();
        let held = lock_schedule(directory).unwrap();
        assert!(backup(&second, Some(false), due).unwrap_err().contains("Другая операция"));
        assert_eq!(scheduled_files(directory).unwrap().len(), RETAIN);
        drop(held);

        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let other = scope.spawn(|| { barrier.wait(); backup(&second, None, due) });
            barrier.wait();
            let this = backup(&first, None, due);
            let results = [this, other.join().unwrap()];
            assert!(results.iter().any(|result| result.as_ref().is_ok_and(|status| status.enabled && status.error.is_none() && status.last_success == Some(due))));
            for error in results.iter().filter_map(|result| result.as_ref().err()) {
                assert!(error.contains("Другая операция"), "{error}");
            }
        });
        let files = scheduled_files(directory).unwrap();
        assert_eq!(files.len(), RETAIN);
        assert_eq!(files[0].0, due);
        assert!(!output.exists());
        assert_eq!(first.load().unwrap(), fixture());
    }

    #[cfg(unix)]
    #[test]
    fn locking_cannot_create_a_file_outside_the_scheduled_backup_directory() {
        let dir = tempdir().unwrap();
        let folder = dir.path().join("managed");
        fs::create_dir(&folder).unwrap();
        let unrelated = dir.path().join("unrelated");
        std::os::unix::fs::symlink(&unrelated,folder.join(".schedule.lock")).unwrap();
        assert!(lock_schedule(&folder).is_err());
        assert!(!unrelated.exists());
    }
}
