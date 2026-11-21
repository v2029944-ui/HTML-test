//! The share transports immutable generations. SQLite only opens local files.
use super::{err, Storage};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};

const PREFIX: &str = "analiz-generation-";
const EXT: &str = ".snapshot";
const MAGIC: &[u8; 8] = b"ANALIZ01";

fn path(folder: &Path, generation: u64) -> PathBuf {
    folder.join(format!("{PREFIX}{generation:020}{EXT}"))
}

fn head(folder: &Path) -> Result<u64, String> {
    let mut latest = 0;
    for entry in fs::read_dir(folder).map_err(err)? {
        let entry = entry.map_err(err)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(PREFIX) || !name.ends_with(EXT) {
            continue;
        }
        let digits = &name[PREFIX.len()..name.len() - EXT.len()];
        let generation: u64 = digits.parse().map_err(err)?;
        if digits.len() != 20
            || !digits.bytes().all(|b| b.is_ascii_digit())
            || generation == 0
            || !entry.file_type().map_err(err)?.is_file()
        {
            return Err("Повреждена запись поколения в общей папке.".into());
        }
        latest = latest.max(generation);
    }
    Ok(latest)
}

fn transfer(source: &mut File, output: &mut File) -> Result<[u8; 32], String> {
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = source.read(&mut buffer).map_err(err)?;
        if n == 0 {
            break;
        }
        output.write_all(&buffer[..n]).map_err(err)?;
        hash.update(&buffer[..n]);
    }
    Ok(hash.finalize().into())
}

/// A disposable local workspace. Only an explicit successful commit publishes edits.
pub struct Checkout {
    folder: PathBuf,
    local: TempDir,
    generation: u64,
    pub storage: Storage,
}

impl Checkout {
    /// local_parent must be a private local cache directory supplied by the app.
    pub fn open(folder: &Path, local_parent: &Path) -> Result<Self, String> {
        let generation = head(folder)?;
        let local = tempfile::tempdir_in(local_parent).map_err(err)?;
        if generation > 0 {
            let mut input = File::open(path(folder, generation)).map_err(err)?;
            let mut magic = [0; 8];
            let mut expected = [0; 32];
            input.read_exact(&mut magic).map_err(err)?;
            input.read_exact(&mut expected).map_err(err)?;
            if &magic != MAGIC {
                return Err("Неизвестный формат общей базы.".into());
            }
            let mut db = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(local.path().join("reports.sqlite3"))
                .map_err(err)?;
            if transfer(&mut input, &mut db)? != expected {
                return Err("Неполная или повреждённая копия общей базы.".into());
            }
        }
        let storage = Storage::new(local.path().to_owned())?;
        Ok(Self {
            folder: folder.to_owned(),
            local,
            generation,
            storage,
        })
    }

    pub fn commit(self) -> Result<u64, String> {
        let conflict = "Общая база изменилась. Обновите данные и повторите предпросмотр.";
        if head(&self.folder)? != self.generation {
            return Err(conflict.into());
        }
        let next = self
            .generation
            .checked_add(1)
            .ok_or("Исчерпаны номера поколений.")?;
        let backup = self.local.path().join("publish.sqlite3");
        self.storage.backup_sqlite_verified(&backup)?;
        let mut input = File::open(backup).map_err(err)?;
        let mut staged = NamedTempFile::new_in(&self.folder).map_err(err)?;
        staged.write_all(MAGIC).map_err(err)?;
        staged.write_all(&[0; 32]).map_err(err)?;
        let digest = transfer(&mut input, staged.as_file_mut())?;
        staged.seek(SeekFrom::Start(8)).map_err(err)?;
        staged.write_all(&digest).map_err(err)?;
        staged.as_file().sync_all().map_err(err)?;
        // On both NTFS/SMB and local test filesystems this must be a single
        // no-replace operation. Never fall back to a copying or replacing move.
        let staged = staged.into_temp_path();
        staged
            .persist_noclobber(path(&self.folder, next))
            .map_err(|failure| {
                let error = failure.error;
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    conflict.to_owned()
                } else {
                    format!("Не удалось подтвердить публикацию. Проверьте общую папку: {error}")
                }
            })?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Barrier};

    fn exchange_folder() -> TempDir {
        match std::env::var_os("ANALIZ_TEST_SMB_PARENT") {
            Some(parent) => tempfile::tempdir_in(parent).unwrap(),
            None => tempfile::tempdir().unwrap(),
        }
    }

    #[test]
    fn simultaneous_publications_keep_one_complete_generation_and_cancel_is_local() {
        let folder = exchange_folder();
        let clients = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
        let barrier = Arc::new(Barrier::new(2));
        let results = std::thread::scope(|threads| {
            let mut workers = Vec::new();
            for (i, client) in clients.iter().enumerate() {
                let checkout = Checkout::open(folder.path(), client.path()).unwrap();
                let barrier = Arc::clone(&barrier);
                workers.push(threads.spawn(move || {
                    let id = format!("source-{i}");
                    checkout.storage.save(json!({"reports":[{"id":id,"type":"day",
                        "start":"2024-02-29","end":"2024-02-29","rows":[]}],"settings":{"orgNames":["A","B","C","D"],"groups":[],"catalog":[]}})).unwrap();
                    barrier.wait();
                    (id, checkout.commit())
                }));
            }
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(results.iter().filter(|(_, r)| r.is_ok()).count(), 1);
        let (winner, result) = results.iter().find(|(_, r)| r.is_ok()).unwrap();
        assert_eq!(result.as_ref().unwrap(), &1);
        let checkout = Checkout::open(folder.path(), clients[1].path()).unwrap();
        let data = checkout.storage.load().unwrap();
        assert_eq!(data["reports"].as_array().unwrap().len(), 1);
        assert_eq!(data["reports"][0]["id"].as_str(), Some(winner.as_str()));
        checkout.storage.save(json!({"reports":[],"settings":{"orgNames":["A","B","C","D"],"groups":[],"catalog":[]}})).unwrap();
        drop(checkout);
        let fresh = Checkout::open(folder.path(), clients[0].path()).unwrap();
        assert_eq!(fresh.storage.load().unwrap(), data);
        assert_eq!(fresh.commit().unwrap(), 2);
        assert_eq!(head(folder.path()).unwrap(), 2);
    }

    #[test]
    fn incomplete_staging_is_invisible_and_corrupt_generation_is_rejected() {
        let folder = exchange_folder();
        let client = tempfile::tempdir().unwrap();
        fs::write(folder.path().join(".interrupted-upload"), b"incomplete").unwrap();
        assert_eq!(head(folder.path()).unwrap(), 0);
        Checkout::open(folder.path(), client.path())
            .unwrap()
            .commit()
            .unwrap();
        OpenOptions::new()
            .append(true)
            .open(path(folder.path(), 1))
            .unwrap()
            .write_all(b"damage")
            .unwrap();
        assert!(Checkout::open(folder.path(), client.path()).is_err());
    }
}
