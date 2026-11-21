//! The share transports immutable generations of application data. Every live
//! SQLite schema is initialized by this binary in a private local directory.
use super::{err, opened_file_path, Storage};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};

const PREFIX: &str = "analiz-generation-";
const EXT: &str = ".snapshot";
const MAGIC: &[u8; 8] = b"ANALIZ02";
const PENDING: &str = "pending.snapshot";

// Columns and SQL are compiled constants. The transport has ordinary,
// unconstrained data tables only. Indexes, triggers, views, derived caches,
// folder/replace grants and local config are never part of the exchange.
const TABLES: &[(&str, &str)] = &[
    (
        "reports",
        "id,report_type,period_start,period_end,source_name,json",
    ),
    ("kv", "name,json"),
    ("report_source_diagnostic_links", "report_id,anchor_id"),
    (
        "metric_report_docs",
        "id,kind,start,end,source_name,row_count",
    ),
    (
        "metric_report_values",
        "report_id,row_no,organization,dimension,metric,value",
    ),
    (
        "import_receipts",
        "operation_id,revision,reports,rows,replaced",
    ),
    ("import_generation", "singleton,generation"),
    ("metric_catalog_revision", "singleton,revision"),
    // Restored last: inserts into the fresh application schema run local revision triggers.
    ("storage_revision", "singleton,revision"),
];

fn path(folder: &Path, generation: u64) -> PathBuf {
    folder.join(format!("{PREFIX}{generation:020}{EXT}"))
}

fn directory(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() || !path.is_dir() {
        return Err("Требуется существующий абсолютный путь к папке.".into());
    }
    fs::canonicalize(path).map_err(err)
}

fn separate(shared: &Path, local: &Path) -> Result<(), String> {
    if shared.starts_with(local) || local.starts_with(shared) {
        Err("Общая папка и локальное хранилище должны находиться отдельно.".into())
    } else {
        Ok(())
    }
}

pub fn head(folder: &Path) -> Result<u64, String> {
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

// Paths passed here are a canonical directory plus our own fixed leaf. Check
// the opened handle before copying a single byte, including on Windows/SMB.
fn open_regular(path: &Path) -> Result<File, String> {
    if !fs::symlink_metadata(path)
        .map_err(err)?
        .file_type()
        .is_file()
    {
        return Err("Ожидался обычный файл общей базы.".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0o400000 | 0o4000);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000).share_mode(1);
    }
    let file = options.open(path).map_err(err)?;
    if !file.metadata().map_err(err)?.is_file() || opened_file_path(&file)? != path {
        return Err("Расположение файла общей базы изменилось.".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if file.metadata().map_err(err)?.file_attributes() & 0x400 != 0 {
            return Err("Ссылки в общей базе недопустимы.".into());
        }
    }
    Ok(file)
}

fn remaining(file: &mut File) -> Result<u64, String> {
    file.metadata()
        .map_err(err)?
        .len()
        .checked_sub(file.stream_position().map_err(err)?)
        .ok_or_else(|| "Размер файла общей базы изменился.".into())
}

fn transfer(source: &mut File, output: &mut impl Write) -> Result<[u8; 32], String> {
    let bytes = remaining(source)?;
    let mut hash = Sha256::new();
    let mut reader = source.take(bytes);
    let mut buffer = [0; 64 * 1024];
    while reader.limit() > 0 {
        let n = reader.read(&mut buffer).map_err(err)?;
        if n == 0 {
            return Err("Неполная копия общей базы.".into());
        }
        output.write_all(&buffer[..n]).map_err(err)?;
        hash.update(&buffer[..n]);
    }
    if remaining(source)? != 0 {
        return Err("Размер файла общей базы изменился.".into());
    }
    Ok(hash.finalize().into())
}

fn digest_header(file: &mut File) -> Result<[u8; 32], String> {
    let mut magic = [0; 8];
    let mut digest = [0; 32];
    file.read_exact(&mut magic).map_err(err)?;
    file.read_exact(&mut digest).map_err(err)?;
    if &magic != MAGIC {
        return Err("Неизвестный или устаревший формат общей базы.".into());
    }
    Ok(digest)
}

fn hex_digest(digest: [u8; 32]) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(64);
    for byte in digest {
        write!(&mut text, "{byte:02x}").expect("writing to String");
    }
    text
}

fn advertised(folder: &Path, generation: u64) -> Result<Option<[u8; 32]>, String> {
    if generation == 0 {
        return Ok(None);
    }
    Ok(Some(digest_header(&mut open_regular(&path(
        folder, generation,
    ))?)?))
}

pub fn generation_digest(folder: &Path, generation: u64) -> Result<Option<String>, String> {
    Ok(advertised(&directory(folder)?, generation)?.map(hex_digest))
}

fn check_space(parent: &Path, bytes: u64) -> Result<(), String> {
    if fs2::available_space(parent).map_err(err)? < bytes {
        Err("Недостаточно свободного места для копии общей базы.".into())
    } else {
        Ok(())
    }
}

fn attach_wire(conn: &Connection, wire: &Path, read_only: bool) -> Result<(), String> {
    use rusqlite::config::DbConfig;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
        .map_err(err)?;
    conn.execute_batch("PRAGMA trusted_schema=OFF")
        .map_err(err)?;
    let path = wire.to_str().ok_or("Некорректный путь локальной копии.")?;
    let name = if read_only {
        // SQLite URI metacharacters in a local path (including the Windows
        // verbatim prefix) are filename bytes, never URI options.
        let mut uri = String::from("file:");
        for byte in path.bytes() {
            if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
                uri.push(char::from(byte));
            } else {
                use std::fmt::Write;
                write!(&mut uri, "%{byte:02X}").map_err(err)?;
            }
        }
        uri.push_str("?mode=ro&immutable=1");
        uri
    } else {
        path.to_owned()
    };
    conn.execute("ATTACH DATABASE ?1 AS wire", [name])
        .map_err(err)?;
    if read_only && !conn.is_readonly("wire").map_err(err)? {
        return Err("Не удалось открыть общую копию только для чтения.".into());
    }
    Ok(())
}

fn wire_schema(conn: &Connection) -> Result<(), String> {
    let count: i64 = conn
        .query_row("SELECT count(*) FROM wire.sqlite_schema", [], |r| r.get(0))
        .map_err(err)?;
    if count != TABLES.len() as i64 {
        return Err("Недопустимая структура общей базы.".into());
    }
    for &(name, columns) in TABLES {
        let expected = format!("CREATE TABLE {name}({columns})");
        let valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM wire.sqlite_schema WHERE type='table' AND name=?1 AND tbl_name=?1 AND sql=?2)",[name,expected.as_str()],|r|r.get(0)).map_err(err)?;
        if !valid {
            return Err("Недопустимая таблица в общей базе.".into());
        }
    }
    let integrity: String = conn
        .query_row("PRAGMA wire.quick_check(1)", [], |r| r.get(0))
        .map_err(err)?;
    if integrity != "ok" {
        return Err("Нарушена целостность общей базы.".into());
    }
    let invalid: bool = conn
        .query_row(
            "
        SELECT EXISTS(SELECT 1 FROM wire.kv WHERE name IS NOT 'settings')
        OR EXISTS(SELECT 1 FROM wire.reports r,json_each(CAST(r.json AS TEXT)) f
          WHERE f.key='desktopFolder')
        OR (SELECT count(*) FROM wire.storage_revision)<>1
        OR (SELECT count(*) FROM wire.import_generation)<>1
        OR (SELECT count(*) FROM wire.metric_catalog_revision)<>1",
            [],
            |r| r.get(0),
        )
        .map_err(err)?;
    if invalid {
        return Err("Общая копия содержит недопустимые параметры компьютера или версии.".into());
    }
    Ok(())
}

fn export_wire(storage: &Storage, wire: &Path) -> Result<(), String> {
    let mut conn = storage.connect()?;
    attach_wire(&conn, wire, false)?;
    let tx = conn.transaction().map_err(err)?;
    for &(name, columns) in TABLES {
        tx.execute_batch(&format!("CREATE TABLE wire.{name}({columns})"))
            .map_err(err)?;
        let projection = match name {
            "kv" => "SELECT name,json FROM main.kv WHERE name='settings'".to_owned(),
            // Folder ownership only belongs to the machine that inspected this
            // workbook. Preserve source filenames, decisions, diagnostics and numeric JSON.
            "reports" => "SELECT id,report_type,period_start,period_end,source_name,
                CASE WHEN json_type(CAST(json AS TEXT),'$.desktopFolder') IS NOT NULL
                THEN CAST(json_remove(CAST(json AS TEXT),'$.desktopFolder') AS BLOB)
                ELSE json END FROM main.reports"
                .to_owned(),
            _ => format!("SELECT {columns} FROM main.{name}"),
        };
        tx.execute_batch(&format!("INSERT INTO wire.{name}({columns}) {projection}"))
            .map_err(err)?;
    }
    tx.commit().map_err(err)?;
    wire_schema(&conn)?;
    Ok(())
}

fn import_wire(storage: &Storage, wire: &Path) -> Result<(), String> {
    let mut conn = storage.connect()?;
    // 'wire' is a verified byte copy in this new private directory. Neither
    // its schema nor any statements from it are copied into the live database.
    attach_wire(&conn, wire, true)?;
    let tx = conn.transaction().map_err(err)?;
    wire_schema(&tx)?;
    for &(name, columns) in TABLES {
        if matches!(
            name,
            "storage_revision" | "import_generation" | "metric_catalog_revision"
        ) {
            tx.execute_batch(&format!("DELETE FROM main.{name}"))
                .map_err(err)?;
        }
        tx.execute_batch(&format!(
            "INSERT INTO main.{name}({columns}) SELECT {columns} FROM wire.{name}"
        ))
        .map_err(err)?;
    }
    // Validate the logical links against locally created primary-key indexes.
    // Rebuilding the working caches must not silently lose a diagnostic owner
    // or expose totals for a metric document that is absent from the dataset.
    let broken:bool=tx.query_row("
        SELECT EXISTS(SELECT 1 FROM main.report_source_diagnostic_links l
            WHERE NOT EXISTS(SELECT 1 FROM main.reports r WHERE r.id=l.report_id)
            OR NOT EXISTS(SELECT 1 FROM main.reports r WHERE r.id=l.anchor_id))
        OR EXISTS(SELECT 1 FROM main.metric_report_values v
            WHERE NOT EXISTS(SELECT 1 FROM main.metric_report_docs d WHERE d.id=v.report_id))
        OR EXISTS(SELECT 1 FROM main.reports r WHERE r.id IS NOT json_extract(CAST(r.json AS TEXT),'$.id')
            OR r.report_type IS NOT json_extract(CAST(r.json AS TEXT),'$.type')
            OR r.period_start IS NOT json_extract(CAST(r.json AS TEXT),'$.start')
            OR r.period_end IS NOT json_extract(CAST(r.json AS TEXT),'$.end')
            OR json_type(CAST(r.json AS TEXT),'$.rows') IS NOT 'array')",[],|r|r.get(0)).map_err(err)?;
    if broken {
        return Err("Нарушена связь строк, периодов или замечаний общей базы.".into());
    }
    // Preserve how the workbook was imported, but grant no machine-local
    // replacement authority for any remotely supplied report. In source_plan
    // the 'manual' ownership class never resolves or adopts a filesystem path.
    tx.execute_batch(
        "INSERT INTO main.report_source_ownership(report_id,source_name,folder,mode)
        SELECT id,source_name,'','manual' FROM main.reports WHERE source_name IS NOT NULL",
    )
    .map_err(err)?;
    tx.commit().map_err(err)
}

pub struct Checkout {
    folder: PathBuf,
    local: TempDir,
    generation: u64,
    digest: Option<[u8; 32]>,
    pub storage: Storage,
}
impl Checkout {
    pub fn open(folder: &Path, local_parent: &Path) -> Result<Self, String> {
        let folder = directory(folder)?;
        let parent = directory(local_parent)?;
        separate(&folder, &parent)?;
        let generation = head(&folder)?;
        let local = tempfile::tempdir_in(&parent).map_err(err)?;
        let storage = Storage::new(local.path().to_owned())?;
        let digest = if generation > 0 {
            let mut input = open_regular(&path(&folder, generation))?;
            let digest = digest_header(&mut input)?;
            check_space(&parent, remaining(&mut input)?)?;
            let mut wire = NamedTempFile::new_in(local.path()).map_err(err)?;
            if transfer(&mut input, wire.as_file_mut())? != digest {
                return Err("Неполная или повреждённая копия общей базы.".into());
            }
            // Close the writer before SQLite opens the private file.
            let wire = wire.into_temp_path();
            import_wire(&storage, &wire)?;
            Some(digest)
        } else {
            None
        };
        storage.revision()?;
        Ok(Self {
            folder,
            local,
            generation,
            digest,
            storage,
        })
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn verified_digest(&self) -> Option<String> {
        self.digest.map(hex_digest)
    }
    pub fn fork(&self, local_parent: &Path) -> Result<Self, String> {
        let parent = directory(local_parent)?;
        separate(&self.folder, &parent)?;
        let local = tempfile::tempdir_in(parent).map_err(err)?;
        let storage = Storage::new(local.path().to_owned())?;
        self.storage.backup_sqlite_verified(&storage.db_path())?;
        Ok(Self {
            folder: self.folder.clone(),
            local,
            generation: self.generation,
            digest: self.digest,
            storage,
        })
    }
    pub fn confirm_publication(
        &mut self,
        generation: u64,
        publication: &Publication,
    ) -> Result<(), String> {
        if self.generation.checked_add(1) != Some(generation)
            || self.folder != publication.folder
            || self.digest != publication.base
            || generation != publication.next
        {
            return Err("Поколение публикации не соответствует рабочей копии.".into());
        }
        self.digest = Some(publication.digest);
        self.generation = generation;
        Ok(())
    }
    pub fn prepare_publication(&self) -> Result<Publication, String> {
        let next = self
            .generation
            .checked_add(1)
            .ok_or("Исчерпаны номера поколений.")?;
        let parent = self
            .local
            .path()
            .parent()
            .ok_or("Не найдена папка локальной копии.")?;
        let scratch = tempfile::tempdir_in(parent).map_err(err)?;
        let source = scratch.path().join("data.sqlite3");
        export_wire(&self.storage, &source)?;
        let digest = transfer(&mut open_regular(&source)?, &mut std::io::sink())?;
        Ok(Publication {
            _scratch: Some(scratch),
            folder: self.folder.clone(),
            base: self.digest,
            next,
            digest,
            source,
            offset: 0,
        })
    }
    pub fn commit(self) -> Result<u64, String> {
        self.prepare_publication()?.publish()
    }
}

#[derive(Serialize, Deserialize)]
struct Journal {
    folder: PathBuf,
    base: Option<[u8; 32]>,
    generation: u64,
    digest: [u8; 32],
}

pub struct Publication {
    _scratch: Option<TempDir>,
    folder: PathBuf,
    next: u64,
    base: Option<[u8; 32]>,
    digest: [u8; 32],
    source: PathBuf,
    offset: u64,
}
impl Publication {
    pub fn digest(&self) -> String {
        hex_digest(self.digest)
    }
    pub fn generation(&self) -> u64 {
        self.next
    }
    fn copy(&self, out: &mut impl Write) -> Result<(), String> {
        let mut input = open_regular(&self.source)?;
        input.seek(SeekFrom::Start(self.offset)).map_err(err)?;
        if transfer(&mut input, out)? != self.digest {
            return Err("Подготовленная копия общей базы изменилась.".into());
        }
        Ok(())
    }
    pub fn persist(&self, pending_folder: &Path) -> Result<(), String> {
        let parent = directory(pending_folder)?;
        separate(&self.folder, &parent)?;
        let journal = serde_json::to_vec(&Journal {
            folder: self.folder.clone(),
            generation: self.next,
            base: self.base,
            digest: self.digest,
        })
        .map_err(err)?;
        let len = u32::try_from(journal.len()).map_err(err)?;
        let payload = fs::metadata(&self.source)
            .map_err(err)?
            .len()
            .checked_sub(self.offset)
            .ok_or("Коррупция подготовленной копии.")?;
        check_space(
            &parent,
            payload
                .checked_add(4 + u64::from(len))
                .ok_or("Файл слишком велик.")?,
        )?;
        let mut file = NamedTempFile::new_in(&parent).map_err(err)?;
        file.write_all(&len.to_le_bytes()).map_err(err)?;
        file.write_all(&journal).map_err(err)?;
        self.copy(file.as_file_mut())?;
        file.as_file().sync_all().map_err(err)?;
        match file
            .into_temp_path()
            .persist_noclobber(parent.join(PENDING))
        {
            Ok(()) => {
                #[cfg(unix)]
                File::open(&parent)
                    .and_then(|f| f.sync_all())
                    .map_err(err)?;
                Ok(())
            }
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                let prior = Self::resume(&self.folder, &parent)?;
                if prior.next == self.next && prior.digest == self.digest && prior.base == self.base
                {
                    Ok(())
                } else {
                    Err("Сначала завершите предыдущую публикацию.".into())
                }
            }
            Err(e) => Err(err(e.error)),
        }
    }
    pub fn resume(folder: &Path, pending_folder: &Path) -> Result<Self, String> {
        let parent = directory(pending_folder)?;
        let source = parent.join(PENDING);
        let mut file = open_regular(&source)?;
        let mut len = [0; 4];
        file.read_exact(&mut len).map_err(err)?;
        let len = u32::from_le_bytes(len) as usize;
        if len > 1024 * 1024 {
            return Err("Некорректная запись ожидающей публикации.".into());
        }
        let mut header = vec![0; len];
        file.read_exact(&mut header).map_err(err)?;
        let journal: Journal = serde_json::from_slice(&header).map_err(err)?;
        if !folder.is_absolute()
            || journal.generation == 0
            || ((journal.generation == 1) != journal.base.is_none())
            || (journal.folder != folder
                && directory(folder).ok().as_ref() != Some(&journal.folder))
        {
            return Err("Ожидающая публикация относится к другой общей папке.".into());
        }
        separate(&journal.folder, &parent)?;
        let offset = file.stream_position().map_err(err)?;
        if transfer(&mut file, &mut std::io::sink())? != journal.digest {
            return Err("Неполная копия ожидающей публикации.".into());
        }
        Ok(Self {
            _scratch: None,
            folder: journal.folder,
            next: journal.generation,
            base: journal.base,
            digest: journal.digest,
            source,
            offset,
        })
    }

    fn confirmed(&self, target: &Path) -> Result<bool, String> {
        match fs::symlink_metadata(target) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(err(e)),
            Ok(_) => {}
        }
        let mut file = open_regular(target)?;
        if digest_header(&mut file)? != self.digest {
            return Err("Общая база изменилась. Обновите данные и повторите предпросмотр.".into());
        }
        if transfer(&mut file, &mut std::io::sink())? != self.digest {
            return Err("Контрольная сумма опубликованной базы не совпадает.".into());
        }
        Ok(true)
    }
    fn check_base(&self) -> Result<(), String> {
        if advertised(&self.folder, self.next - 1)? != self.base {
            return Err("Исходное поколение общей базы изменилось.".into());
        }
        Ok(())
    }
    pub fn publish(&self) -> Result<u64, String> {
        self.check_base()?;
        let target = path(&self.folder, self.next);
        if self.confirmed(&target)? {
            return Ok(self.next);
        }
        if head(&self.folder)? != self.next - 1 {
            if self.confirmed(&target)? {
                return Ok(self.next);
            }
            return Err("Нарушена последовательность версий общей базы.".into());
        }
        let mut staged = NamedTempFile::new_in(&self.folder).map_err(err)?;
        if opened_file_path(staged.as_file())? != staged.path() {
            return Err("Расположение общей папки изменилось.".into());
        }
        staged.write_all(MAGIC).map_err(err)?;
        staged.write_all(&self.digest).map_err(err)?;
        self.copy(staged.as_file_mut())?;
        staged.as_file().sync_all().map_err(err)?;
        self.check_base()?;
        let persistence = staged.into_temp_path().persist_noclobber(&target);
        if !self.confirmed(&target)? {
            return Err(match persistence {
                Err(failure)=>format!("Не удалось подтвердить публикацию. Повторите отправку подготовленной копии: {}",failure.error),
                Ok(())=>"Общая папка не подтвердила публикацию. Повторите отправку подготовленной копии.".into()
            });
        }
        #[cfg(unix)]
        File::open(&self.folder)
            .and_then(|f| f.sync_all())
            .map_err(err)?;
        Ok(self.next)
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
    fn another_client_reads_the_committed_xlsx_without_reading_the_source_file() {
        use crate::storage::{import_apply_tests, ImportConfirmation, ImportJob};
        use analiz_import_core::StageArea;
        let folder = exchange_folder();
        let writer = tempfile::tempdir().unwrap();
        let reader = tempfile::tempdir().unwrap();
        let source = writer.path().join("daily.xlsx");
        import_apply_tests::fixture(&source, 3);
        let area = StageArea::open(writer.path().join("stage")).unwrap();
        let prepared = import_apply_tests::prepared(&area, &source, "day", "2026-11-01");
        let checkout = Checkout::open(folder.path(), writer.path()).unwrap();
        let preview = checkout.storage.preview_import(&prepared).unwrap();
        assert_eq!(head(folder.path()).unwrap(), 0);
        let receipt = checkout
            .storage
            .apply_import(
                &prepared,
                &preview,
                ImportConfirmation::default(),
                &ImportJob::new(),
                |_| {},
            )
            .unwrap();
        assert_eq!((receipt.reports, receipt.rows), (1, 3));
        let mut confirmed = checkout.storage.load().unwrap();
        confirmed["reports"][0]
            .as_object_mut()
            .unwrap()
            .remove("desktopFolder");
        checkout.commit().unwrap();
        drop(prepared);
        fs::remove_file(&source).unwrap();
        let fresh = Checkout::open(folder.path(), reader.path()).unwrap();
        assert_eq!(fresh.storage.load().unwrap(), confirmed);
        let revision = fresh.storage.revision().unwrap();
        let filter = crate::storage::ReadFilter::default();
        for header in fresh
            .storage
            .report_page(revision, &filter, None, 20)
            .unwrap()
        {
            fresh
                .storage
                .index_unified_startup_report(
                    &header,
                    revision,
                    &crate::storage::ManagementJob::new(),
                )
                .unwrap();
        }
        assert_eq!(
            fresh
                .storage
                .row_page(revision, &filter, None, 20)
                .unwrap()
                .len(),
            3
        );
        assert_eq!(confirmed["reports"][0]["start"], "2026-11-01");
        assert_eq!(confirmed["reports"][0]["rows"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn retry_after_lost_acknowledgement_keeps_later_imports_and_checks_actual_bytes() {
        let folder = exchange_folder();
        let client = tempfile::tempdir().unwrap();
        let publication = Checkout::open(folder.path(), client.path())
            .unwrap()
            .prepare_publication()
            .unwrap();
        assert_eq!(publication.publish().unwrap(), 1);
        // Another writer advances the shared state before this caller retries.
        assert_eq!(
            Checkout::open(folder.path(), client.path())
                .unwrap()
                .commit()
                .unwrap(),
            2
        );
        assert_eq!(publication.publish().unwrap(), 1);
        assert_eq!(head(folder.path()).unwrap(), 2);
        OpenOptions::new()
            .append(true)
            .open(path(folder.path(), 1))
            .unwrap()
            .write_all(b"damage")
            .unwrap();
        assert!(publication.publish().is_err());
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
    #[test]
    fn shared_copy_keeps_report_provenance_but_never_transports_a_local_grant() {
        let folder = exchange_folder();
        let writer = tempfile::tempdir().unwrap();
        let reader = tempfile::tempdir().unwrap();
        let excel = writer.path().join("private-machine-excel-marker");
        fs::create_dir(&excel).unwrap();
        let checkout = Checkout::open(folder.path(), writer.path()).unwrap();
        checkout
            .storage
            .set_folder(excel.to_str().unwrap().into())
            .unwrap();
        checkout.storage.save(json!({"reports":[{"id":"r","type":"day","start":"2026-02-28","end":"2026-02-28",
            "desktopSource":"report.xlsx","desktopFolder":excel,"sourceImportMode":"folder","rows":[]}],
            "settings":{"orgNames":["A","B","C","D"],"groups":[],"catalog":[]}})).unwrap();
        let conn = checkout.storage.connect().unwrap();
        conn.execute(
            "INSERT INTO report_source_ownership VALUES('r','report.xlsx',?1,'folder')",
            [excel.to_str().unwrap()],
        )
        .unwrap();
        drop(conn);
        checkout.commit().unwrap();
        let bytes = fs::read(path(folder.path(), 1)).unwrap();
        assert!(!bytes
            .windows(b"private-machine-excel-marker".len())
            .any(|s| s == b"private-machine-excel-marker"));
        let received = Checkout::open(folder.path(), reader.path()).unwrap();
        let report = &received.storage.load().unwrap()["reports"][0];
        assert!(report.get("desktopFolder").is_none());
        assert_eq!(report["sourceImportMode"], "folder");
        let conn = received.storage.connect().unwrap();
        assert!(super::super::read_key(&conn, "config").unwrap().is_none());
        let grant: (String, String) = conn
            .query_row(
                "SELECT folder,mode FROM report_source_ownership WHERE report_id='r'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(grant, (String::new(), "manual".into()));
    }

    #[test]
    fn reject_unknown_schema_and_local_config_even_if_sender_rehashes_the_snapshot() {
        // Harmless foreign objects exercise the structural allowlist.
        for alteration in [
            "CREATE TABLE unrelated(note TEXT)",
            "CREATE VIEW unrelated AS SELECT 1",
            "INSERT INTO kv VALUES('config','{}')",
            "INSERT INTO report_source_diagnostic_links VALUES('no-report','no-owner')",
            r#"INSERT INTO reports VALUES('r','day','2026-01-01','2026-01-01',NULL,'{"desktopFolder":"unapproved"}')"#,
        ] {
            let folder = exchange_folder();
            let client = tempfile::tempdir().unwrap();
            let source = Checkout::open(folder.path(), client.path()).unwrap();
            let wire = client.path().join("foreign.sqlite3");
            export_wire(&source.storage, &wire).unwrap();
            let c = Connection::open(&wire).unwrap();
            c.execute_batch(alteration).unwrap();
            drop(c);
            let mut bytes = File::open(wire).unwrap();
            let checksum = transfer(&mut bytes, &mut std::io::sink()).unwrap();
            bytes.rewind().unwrap();
            let mut output = File::create_new(path(folder.path(), 1)).unwrap();
            output.write_all(MAGIC).unwrap();
            output.write_all(&checksum).unwrap();
            std::io::copy(&mut bytes, &mut output).unwrap();
            drop(output);
            let error = Checkout::open(folder.path(), client.path()).err().unwrap();
            assert!(
                error.contains("структур") || error.contains("параметр") || error.contains("связь"),
                "{error}"
            );
        }
    }

    #[test]
    fn durable_payload_restarts_offline_and_retries_without_overwriting_another_workspace() {
        let folder = exchange_folder();
        let share = folder.path().join("team");
        fs::create_dir(&share).unwrap();
        let other = exchange_folder();
        let parent = tempfile::tempdir().unwrap();
        let local = parent.path().join("cache % #");
        fs::create_dir(&local).unwrap();
        let pending = tempfile::tempdir_in(&local).unwrap();
        let active = Checkout::open(&share, &local).unwrap();
        let mut candidate = active.fork(&local).unwrap();
        candidate.storage.save(json!({"reports":[{"id":"r","type":"day","start":"2026-01-01","end":"2026-01-01","rows":[]}],"settings":{"orgNames":["A","B","C","D"],"groups":[],"catalog":[]}})).unwrap();
        assert_eq!(
            active.storage.load().unwrap()["reports"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        let publication = candidate.prepare_publication().unwrap();
        publication.persist(pending.path()).unwrap();
        publication.persist(pending.path()).unwrap();
        let remote = fs::canonicalize(&share).unwrap();
        drop(publication);
        drop(active);
        assert!(Publication::resume(other.path(), pending.path()).is_err());
        let offline = folder.path().join("disconnected");
        fs::rename(&share, &offline).unwrap();
        let resumed = Publication::resume(&remote, pending.path()).unwrap();
        assert!(resumed.publish().is_err());
        fs::rename(offline, &share).unwrap();
        assert_eq!(resumed.publish().unwrap(), 1);
        assert!(candidate.confirm_publication(2, &resumed).is_err());
        candidate.confirm_publication(1, &resumed).unwrap();
        assert_eq!(candidate.generation(), 1);
        assert_eq!(resumed.publish().unwrap(), 1);
        assert_eq!(
            Checkout::open(&remote, &local)
                .unwrap()
                .storage
                .load()
                .unwrap()["reports"][0]["id"],
            "r"
        );
        assert_eq!(head(other.path()).unwrap(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn shared_directory_cannot_include_private_cache_or_linked_generations() {
        use std::os::unix::fs::symlink;
        let folder = exchange_folder();
        let cache = tempfile::tempdir().unwrap();
        assert!(Checkout::open(folder.path(), folder.path()).is_err());
        let file = cache.path().join("outside");
        fs::write(&file, b"outside").unwrap();
        symlink(&file, path(folder.path(), 1)).unwrap();
        assert!(Checkout::open(folder.path(), cache.path()).is_err());
        assert_eq!(fs::read(file).unwrap(), b"outside");
    }
    #[test]
    fn publication_refuses_a_different_base_at_the_same_path_and_generation() {
        let root = exchange_folder();
        let client = tempfile::tempdir().unwrap();
        let shared = root.path().join("team");
        fs::create_dir(&shared).unwrap();
        Checkout::open(&shared, client.path())
            .unwrap()
            .commit()
            .unwrap();
        let local = Checkout::open(&shared, client.path()).unwrap();
        assert_eq!(
            local.verified_digest(),
            generation_digest(&shared, 1).unwrap()
        );
        let pending = local.prepare_publication().unwrap();
        fs::rename(&shared, root.path().join("original")).unwrap();
        fs::create_dir(&shared).unwrap();
        let other = Checkout::open(&shared, client.path()).unwrap();
        other.storage.save(json!({"reports":[],"settings":{"orgNames":["Other","B","C","D"],"groups":[],"catalog":[]}})).unwrap();
        other.commit().unwrap();
        assert_ne!(
            local.verified_digest(),
            generation_digest(&shared, 1).unwrap()
        );
        assert!(pending.publish().is_err());
        assert_eq!(head(&shared).unwrap(), 1);
    }
}
