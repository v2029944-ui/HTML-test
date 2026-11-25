//! Keep recent payloads and a compact record of the digests that clients trust.
use super::*;
use std::{collections::BTreeMap, io::ErrorKind};

const POLICY: &str = ".analiz-retention-policy.json";
const HISTORY: &str = ".analiz-retention-v1.bin";
const HISTORY_BYTES: u64 = 32 * 1024 * 1024;
pub(in crate::storage) const SNAPSHOTS: usize = 3;

#[derive(Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Policy {
    enabled: bool,
    has_history: bool,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct History {
    generation: u64,
    digest: [u8; 32],
    archived: BTreeMap<u64, [u8; 32]>,
}

fn optional_file(path: &Path) -> Result<Option<File>, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(err(error)),
        Ok(_) => open_regular(path).map(Some),
    }
}

fn policy(folder: &Path) -> Result<Policy, String> {
    let Some(file) = optional_file(&folder.join(POLICY))? else { return Ok(Policy::default()) };
    let mut bytes = Vec::new();
    file.take(129).read_to_end(&mut bytes).map_err(err)?;
    if bytes.len() > 128 { return Err("Настройка очистки общей папки повреждена.".into()); }
    serde_json::from_slice(&bytes).map_err(err)
}

pub(in crate::storage) fn enabled(folder: &Path) -> Result<bool, String> {
    Ok(policy(folder)?.enabled)
}

fn read_history(folder: &Path) -> Result<Option<History>, String> {
    let Some(mut file) = optional_file(&folder.join(HISTORY))? else { return Ok(None) };
    let invalid = "Не удалось подтвердить историю удалённых снимков общей базы.";
    let mut expected = [0; 32];
    file.read_exact(&mut expected).map_err(err)?;
    let mut bytes = Vec::new();
    file.take(HISTORY_BYTES + 1).read_to_end(&mut bytes).map_err(err)?;
    if bytes.len() as u64 > HISTORY_BYTES || <[u8; 32]>::from(Sha256::digest(&bytes)) != expected {
        return Err(invalid.into());
    }
    let history: History = serde_json::from_slice(&bytes).map_err(err)?;
    if history.generation == 0 || history.archived.first_key_value().is_some_and(|(&n, _)| n == 0)
        || history.archived.last_key_value().is_some_and(|(&n, _)| n >= history.generation)
    {
        return Err(invalid.into());
    }
    match advertised(folder, history.generation) {
        Ok(Some(digest)) if digest == history.digest => {}
        Err(_) if reset_follows(folder, history.generation, &hex_digest(history.digest))? => {}
        _ => return Err(invalid.into()),
    }
    Ok(Some(history))
}

pub(super) fn archived_digest(folder: &Path, generation: u64) -> Result<Option<[u8; 32]>, String> {
    Ok(read_history(folder)?.and_then(|history| history.archived.get(&generation).copied()))
}

fn sync_directory(folder: &Path) -> Result<(), String> {
    #[cfg(unix)]
    File::open(folder).and_then(|file| file.sync_all()).map_err(err)?;
    #[cfg(not(unix))]
    let _ = folder;
    Ok(())
}

fn publish_file(temporary: NamedTempFile, target: &Path) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        temporary.persist(target).map_err(err)?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
        }
        const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
        let temporary = temporary.into_temp_path();
        let wide = |path: &Path| path.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
        let from = wide(&temporary);
        let to = wide(target);
        // The temp file is in the same directory and has already been flushed.
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) } == 0 {
            return Err(err(std::io::Error::last_os_error()));
        }
        let mut options = OpenOptions::new();
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        options.read(true).write(true).custom_flags(0x00200000).share_mode(3);
        let file = options.open(target).map_err(err)?;
        let metadata = file.metadata().map_err(err)?;
        if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 || opened_file_path(&file)? != target {
            return Err("Расположение файла истории очистки изменилось.".into());
        }
        file.sync_all().map_err(err)?;
    }
    sync_directory(target.parent().ok_or("Не найдена общая папка.")?)
}

fn write_policy(folder: &Path, next: &Policy) -> Result<(), String> {
    let mut temporary = NamedTempFile::new_in(folder).map_err(err)?;
    serde_json::to_writer(&mut temporary, next).map_err(err)?;
    temporary.as_file().sync_all().map_err(err)?;
    publish_file(temporary, &folder.join(POLICY))?;
    if policy(folder)? != *next {
        return Err("Общая папка не подтвердила настройку очистки.".into());
    }
    Ok(())
}

fn write_history(folder: &Path, history: &History) -> Result<(), String> {
    let bytes = serde_json::to_vec(history).map_err(err)?;
    if bytes.len() as u64 > HISTORY_BYTES {
        return Err("Достигнут предел журнала очистки общей папки.".into());
    }
    let mut temporary = NamedTempFile::new_in(folder).map_err(err)?;
    temporary.write_all(&Sha256::digest(&bytes)).map_err(err)?;
    temporary.write_all(&bytes).map_err(err)?;
    temporary.as_file().sync_all().map_err(err)?;
    publish_file(temporary, &folder.join(HISTORY))
}

fn verify_snapshot(folder: &Path, generation: u64) -> Result<([u8; 32], Option<ResetAnchor>), String> {
    let mut file = open_regular(&path(folder, generation))?;
    let (digest, reset) = snapshot_header(&mut file, generation)?;
    if transfer_snapshot(&mut file, &mut std::io::sink(), reset)? != digest {
        return Err("Контрольная сумма снимка общей базы не совпадает.".into());
    }
    Ok((digest, reset))
}

fn insert(history: &mut History, generation: u64, digest: [u8; 32]) -> Result<(), String> {
    if history.archived.insert(generation, digest).is_some_and(|previous| previous != digest) {
        return Err("Поколение общей базы не совпадает с историей очистки.".into());
    }
    Ok(())
}

// The caller owns the share's publication lock throughout the checkpoint and deletion.
fn prune(folder: &Path, generation: u64, expected: Option<&str>, mut policy: Policy) -> Result<(), String> {
    let mut generations = Vec::new();
    visit_generations(folder, |number| generations.push(number))?;
    generations.sort_unstable();
    if generations.last().copied().unwrap_or(0) != generation {
        return Err("Общая база обновилась. Очистка отложена до следующей публикации.".into());
    }
    let remove = generations.len().saturating_sub(SNAPSHOTS);
    if remove == 0 { return Ok(()); }
    let (digest, _) = verify_snapshot(folder, generation)?;
    if expected != Some(hex_digest(digest).as_str()) {
        return Err("Состояние общей базы изменилось. Очистка не выполнена.".into());
    }
    for &number in &generations[remove..generations.len() - 1] {
        verify_snapshot(folder, number)?;
    }
    let mut history = match read_history(folder)? {
        Some(history) => history,
        None if policy.has_history => return Err("Файл истории очистки отсутствует. Восстановите его из копии общей папки.".into()),
        None => History { generation, digest, archived: BTreeMap::new() },
    };
    history.generation = generation;
    history.digest = digest;
    for &number in &generations[..remove] {
        let (digest, reset) = verify_snapshot(folder, number)?;
        insert(&mut history, number, digest)?;
        if let Some(anchor) = reset {
            insert(&mut history, anchor.generation, anchor.digest)?;
        }
    }
    write_history(folder, &history)?;
    if read_history(folder)?.as_ref() != Some(&history) {
        return Err("Общая папка не подтвердила сохранение истории снимков.".into());
    }
    if !policy.has_history {
        policy.has_history = true;
        write_policy(folder, &policy)?;
    }
    for &number in &generations[..remove] {
        fs::remove_file(path(folder, number)).map_err(err)?;
    }
    sync_directory(folder)
}

pub(in crate::storage) fn maintain(folder: &Path, generation: u64, expected: Option<&str>) -> Result<(), String> {
    if enabled(folder)? {
        let _lock = lock_publication(folder)?;
        let policy = policy(folder)?;
        if policy.enabled { prune(folder, generation, expected, policy)?; }
    }
    Ok(())
}

pub(in crate::storage) fn configure(folder: &Path, enable: bool, generation: u64, expected: Option<&str>) -> Result<(), String> {
    let _lock = lock_publication(folder)?;
    let mut policy = policy(folder)?;
    if !enable {
        if policy.enabled { policy.enabled = false; write_policy(folder, &policy)?; }
        return Ok(());
    }
    if head(folder)? != generation || advertised(folder, generation)?.map(hex_digest).as_deref() != expected {
        return Err("Общая база изменилась. Проверьте подключение и повторите действие.".into());
    }
    if !policy.enabled {
        policy.enabled = true;
        write_policy(folder, &policy)?;
    }
    prune(folder, generation, expected, policy)
}
