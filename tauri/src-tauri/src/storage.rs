use chrono::NaiveDate;
use rusqlite::{params, Connection, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex, OnceLock, Weak},
    time::UNIX_EPOCH,
};

#[path = "management.rs"]
mod management;
pub use management::*;
#[path = "management_tree.rs"]
mod management_tree;

#[path = "analytics.rs"]
mod analytics;
pub use analytics::*;
#[path = "report_calendar.rs"]
mod report_calendar;
pub use report_calendar::ReportCalendar;
#[path = "report_quality.rs"]
mod report_quality;
pub use report_quality::{QualityScope, ReportQualityPage};
#[path = "unified_reports.rs"]
mod unified_reports;
pub use unified_reports::{UnifiedQualityFilter, UnifiedQualityPage, UnifiedCalendar};

#[path = "shared_runtime.rs"]
pub mod shared_runtime;

#[path = "shared_folder.rs"]
pub mod shared_folder;

#[path = "backup.rs"]
mod backup;
#[path = "report_transfer.rs"]
mod report_transfer;
pub use report_transfer::{TransferPreview, TransferReceipt, TransferRow};
#[path = "recovery.rs"]
mod recovery;
pub use recovery::RestorePreview;
#[path = "activation.rs"]
mod activation;
pub use activation::{AppRestoreLock, RestoreLaunchInfo, RestoreLaunchResult, ScheduledRestore, apply_pending_restore_or_open_original};
#[path = "metric_reports.rs"]
mod metric_reports;
pub use metric_reports::{kinds as metric_kinds, MetricKind, MetricReceipt, MetricReview, MetricSummary};
#[path = "metric_xlsx.rs"]
mod metric_xlsx;
pub use metric_xlsx::{profiles as metric_xlsx_profiles, MetricXlsxProfile};

#[path = "read_model.rs"]
mod read_model;
#[path = "import_apply.rs"]
mod import_apply;
pub use import_apply::{ImportConfirmation, ImportCounts, ImportJob, ImportPreview, ImportProgress, ImportReceipt};
pub use read_model::{Aggregate, IndexedRow, ReadFilter, ReportHeader, MAX_PAGE_ROWS};

const MAX_EXCEL_BYTES: u64 = 30 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: usize = 200 * 1024 * 1024;
// Keep synchronized with REPORT_TYPES in the root index.html.
const REPORT_TYPES: [&str; 3] = ["week", "month", "day"];

#[derive(Debug, Serialize)]
pub struct SqlPage<T> {
    pub rows: Vec<T>,
    pub total_rows: i64,
}

pub(crate) fn direct_page_offset(index: usize, size: usize, total: i64) -> Result<i64, String> {
    if size == 0 || total < 0 {
        return Err("Invalid page size or row count".into());
    }
    let offset = index.checked_mul(size)
        .and_then(|value| i64::try_from(value).ok())
        .ok_or("Page offset exceeds SQLite bounds")?;
    if offset > 0 && offset >= total {
        return Err("Page lies outside the result".into());
    }
    Ok(offset)
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// An omitted option is valid; an explicit null is a malformed import option.
fn present_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where D: serde::Deserializer<'de>, T: Deserialize<'de> {
    T::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SheetMapping {
    pub work_col: u32,
    pub header_row: u32,
    pub first_day_col: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_count_col: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_amount_col: Option<u32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Rule {
    #[serde(rename = "type")]
    pub kind: String,
    pub start: String,
    #[serde(default)]
    pub sheet: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present_option")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present_option")]
    pub year: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present_option")]
    pub sheets: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present_option")]
    pub mappings: Option<HashMap<String, SheetMapping>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Config {
    version: u8,
    folder: Option<String>,
    #[serde(default)]
    rules: HashMap<String, Rule>,
}

impl Default for Config {
    fn default() -> Self {
        Self { version: 1, folder: None, rules: HashMap::new() }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExcelFile {
    name: String,
    size: u64,
    signature: String,
    native_signature: String,
    too_large: bool,
    /// Independent from the old browser 30-MiB limit.
    native_too_large: bool,
}

#[derive(Serialize)]
struct Listing {
    folder: Option<String>,
    revision: i64,
    files: Vec<ExcelFile>,
    rules: HashMap<String, Rule>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadExcel {
    bytes: Vec<u8>,
    signature: String,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FolderImportRequest {
    pub session: String,
    pub folder: String,
    pub revision: i64,
    pub name: String,
    pub signature: String,
}

struct FolderImportSession {
    token: String,
    owner: String,
    active: Option<Arc<ImportJob>>,
}

pub struct FolderImportOperation<'a> {
    store: &'a Storage,
    pub job: Arc<ImportJob>,
}

impl Drop for FolderImportOperation<'_> {
    fn drop(&mut self) {
        if let Ok(mut session) = self.store.folder_import.lock() {
            if let Some(session) = session.as_mut() {
                if session.active.as_ref().is_some_and(|active| Arc::ptr_eq(active, &self.job)) {
                    session.active = None;
                }
            }
        }
    }
}

pub struct Storage {
    folder_import: Arc<Mutex<Option<FolderImportSession>>>,
    folder_import_serial: AtomicU64,
    root: PathBuf,
    gate: Mutex<()>,
    analysis_filter_gate: Arc<Mutex<()>>,
}

// Each live database has one queue for filter writes and settings index publication. Weak
// entries avoid retaining paths for databases which are no longer in use.
fn analysis_filter_gate(root: &Path) -> Result<Arc<Mutex<()>>, String> {
    static GATES: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();
    let key = fs::canonicalize(root).map_err(err)?;
    let mut gates = GATES.get_or_init(|| Mutex::new(HashMap::new())).lock().map_err(err)?;
    gates.retain(|_, gate| gate.strong_count() > 0);
    if let Some(gate) = gates.get(&key).and_then(Weak::upgrade) { return Ok(gate); }
    let gate = Arc::new(Mutex::new(()));
    gates.insert(key, Arc::downgrade(&gate));
    Ok(gate)
}

fn valid_date(input: &str) -> bool {
    input.len() == 10
        && input.as_bytes().get(4) == Some(&b'-')
        && input.as_bytes().get(7) == Some(&b'-')
        && NaiveDate::parse_from_str(input, "%Y-%m-%d").is_ok()
}

fn safe_excel_name(name: &str) -> bool {
    !name.is_empty()
        && name.encode_utf16().count() <= 240
        && name.to_ascii_lowercase().ends_with(".xlsx")
        && !name.starts_with("~$")
        && !name.chars().any(|c| c == '/' || c == '\\' || c == ':')
        && !name.chars().any(|c| c.is_control())
        && name != "."
        && name != ".."
        && Path::new(name).file_name().and_then(|x| x.to_str()) == Some(name)
}

fn validate_rule(rule: &Rule) -> Result<(), String> {
    if !REPORT_TYPES.contains(&rule.kind.as_str()) {
        return Err("Тип отчёта должен быть «Дни», «Недели» или «Месяцы».".into());
    }
    let wide = matches!(rule.format.as_deref(), Some("daily-wide" | "period-pairs"));
    if (rule.format.is_some() && !matches!(rule.format.as_deref(), Some("auto" | "legacy" | "daily-wide" | "period-pairs"))) || (rule.format.as_deref() == Some("daily-wide") && rule.kind != "day") {
        return Err("Некорректный формат импорта.".into());
    }
    if !(wide && rule.start.is_empty())
        && (!valid_date(&rule.start) || (rule.kind == "month" && !rule.start.ends_with("-01"))) {
        return Err("Некорректная дата первого периода.".into());
    }
    if rule.sheet.encode_utf16().count() > 120 {
        return Err("Название листа Excel слишком длинное.".into());
    }
    if (!wide && (rule.year.is_some() || rule.sheets.is_some() || rule.mappings.is_some()))
        || rule.year.is_some_and(|year| !(1900..=9999).contains(&year)) {
        return Err("Некорректные параметры годового импорта.".into());
    }
    let valid_sheet = |name: &str| !name.trim().is_empty() && name.encode_utf16().count() <= 120;
    if let Some(sheets) = &rule.sheets {
        if sheets.len() > 12 || sheets.iter().any(|name| !valid_sheet(name)) {
            return Err("Некорректный список листов Excel.".into());
        }
    }
    if let Some(mappings) = &rule.mappings {
        for (name, mapping) in mappings {
            if !valid_sheet(name) || mapping.work_col > 2047 || mapping.first_day_col > 2047
                || mapping.header_row > 59999
                || mapping.total_count_col.is_some_and(|col| col > 2047)
                || mapping.total_amount_col.is_some_and(|col| col > 2047) {
                return Err("Некорректное сопоставление столбцов Excel.".into());
            }
        }
    }
    Ok(())
}

fn validate_data(data: &Value) -> Result<(), String> {
    let reports = data.get("reports").and_then(Value::as_array)
        .ok_or("Отсутствует массив отчётов.")?;
    let settings = data.get("settings").ok_or("Нет настроек.")?;
    if settings.get("orgNames").and_then(Value::as_array).map(Vec::len) != Some(4)
        || !settings.get("groups").is_some_and(Value::is_array)
        || !settings.get("catalog").is_some_and(Value::is_array)
    {
        return Err("Настройки подразделений, групп или справочника некорректны.".into());
    }
    for report in reports {
        if !report.get("id").is_some_and(Value::is_string)
            || !report.get("type").and_then(Value::as_str)
                .is_some_and(|x| REPORT_TYPES.contains(&x))
            || !report.get("start").and_then(Value::as_str).is_some_and(valid_date)
            || !report.get("end").and_then(Value::as_str).is_some_and(valid_date)
        {
            return Err("У отчёта неверный идентификатор, тип или дата.".into());
        }
        let rows = report.get("rows").and_then(Value::as_array)
            .ok_or("В отчёте отсутствуют строки работ.")?;
        for row in rows {
            if !row.get("work").is_some_and(Value::is_string)
                || !row.get("organization").is_some_and(Value::is_string)
                || !row.get("count").is_some_and(Value::is_number)
                || !row.get("amount").is_some_and(Value::is_number)
            {
                return Err("В отчёте некорректная строка количества или суммы.".into());
            }
        }
    }
    if serde_json::to_vec(data).map_err(err)?.len() > MAX_PAYLOAD_BYTES {
        return Err("Размер операции превышает лимит 200 МБ.".into());
    }
    Ok(())
}

// Increment when persisted schema definitions change. Connection-local settings
// belong outside initialization: ready WAL readers must never acquire a writer.
const STORAGE_SCHEMA_VERSION: i64 = 1;
fn new_connection(path: &Path) -> Result<Connection, String> {
    let mut conn = Connection::open(path).map_err(err)?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(err)?;
    if version > STORAGE_SCHEMA_VERSION {
        return Err("Версия схемы SQLite новее поддерживаемой программой.".into());
    }
    let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).map_err(err)?;
    if !mode.eq_ignore_ascii_case("wal") {
        let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0)).map_err(err)?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err("Не удалось включить WAL для SQLite.".into());
        }
    }
    conn.execute_batch("PRAGMA synchronous=FULL; PRAGMA temp_store=FILE; PRAGMA cache_size=-2048; PRAGMA temp.cache_size=-2048;").map_err(err)?;
    if version == STORAGE_SCHEMA_VERSION { return Ok(conn); }
    // A second opener may have initialized while we waited for the writer.
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(err)?;
    let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(err)?;
    if version > STORAGE_SCHEMA_VERSION {
        return Err("Версия схемы SQLite новее поддерживаемой программой.".into());
    }
    if version < STORAGE_SCHEMA_VERSION {
        tx.execute_batch("CREATE TABLE IF NOT EXISTS reports (
           id TEXT PRIMARY KEY, report_type TEXT NOT NULL,
           period_start TEXT NOT NULL, period_end TEXT NOT NULL,
           source_name TEXT, json TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS kv (name TEXT PRIMARY KEY, json TEXT NOT NULL);").map_err(err)?;
        read_model::initialize(&tx)?;
        metric_reports::initialize(&tx)?;
        analytics::initialize(&tx)?;
        management::initialize(&tx)?;
        import_apply::initialize(&tx)?;
        tx.pragma_update(None, "user_version", STORAGE_SCHEMA_VERSION).map_err(err)?;
    }
    tx.commit().map_err(err)?;
    Ok(conn)
}

fn default_settings() -> Value {
    json!({
        "orgNames":["СМУ16","АВТО","ТРАК","ЦЕНТР"],
        "groups":[],
        "catalog":[]
    })
}

fn read_key(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    let mut stmt = conn.prepare("SELECT json FROM kv WHERE name=?1").map_err(err)?;
    let mut rows = stmt.query([key]).map_err(err)?;
    match rows.next().map_err(err)? {
        Some(row) => match row.get_ref(0).map_err(err)? {
            rusqlite::types::ValueRef::Text(bytes) | rusqlite::types::ValueRef::Blob(bytes) =>
                String::from_utf8(bytes.to_vec()).map(Some).map_err(err),
            _ => Err("Invalid stored kv JSON type".into()),
        },
        None => Ok(None),
    }
}

fn insert_key(tx: &Transaction<'_>, key: &str, value: &str) -> Result<(), String> {
    tx.execute(
        "INSERT INTO kv(name, json) VALUES(?1, ?2)
         ON CONFLICT(name) DO UPDATE SET json=excluded.json",
        params![key, value],
    ).map_err(err)?;
    Ok(())
}

fn write_data(tx: &Transaction<'_>, data: &Value) -> Result<(), String> {
    validate_data(data)?;
    // The legacy full-snapshot writer cannot rebuild wide source-diagnostic
    // links. Refuse the entire transaction instead of silently orphaning days.
    let protected: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM report_source_diagnostic_links)",
        [],
        |row| row.get(0),
    ).map_err(err)?;
    if protected {
        return Err("Legacy full-history save cannot preserve shared source diagnostics; use prepared or bounded management operations.".into());
    }
    tx.execute("DELETE FROM reports", []).map_err(err)?;
    let mut stmt = tx.prepare(
        "INSERT INTO reports(id, report_type, period_start, period_end, source_name, json)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6)"
    ).map_err(err)?;
    for report in data["reports"].as_array().ok_or("Нет массива отчётов.")? {
        stmt.execute(params![
            report["id"].as_str().ok_or("Отсутствует id.")?,
            report["type"].as_str().ok_or("Отсутствует тип.")?,
            report["start"].as_str().ok_or("Отсутствует начало.")?,
            report["end"].as_str().ok_or("Отсутствует окончание.")?,
            report["desktopSource"].as_str(),
            report.to_string(),
        ]).map_err(err)?;
        read_model::index_value(tx, report["id"].as_str().ok_or("Missing id")?, report)?;
    }
    drop(stmt);
    insert_key(tx, "settings", &data["settings"].to_string())?;
    Ok(())
}

// Classic folder reports persist this signature in desktopSignature.
fn file_signature(meta: &fs::Metadata) -> Result<String, String> {
    let millis = |time: Result<std::time::SystemTime, std::io::Error>| {
        time.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|x| x.as_millis()).unwrap_or(0)
    };
    Ok(format!("{}:{}:{}", meta.len(), millis(meta.modified()), millis(meta.created())))
}

fn native_file_signature(meta: &fs::Metadata) -> String {
    let nanos = |time: Result<std::time::SystemTime, std::io::Error>| {
        time.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|x| x.as_nanos()).unwrap_or(0)
    };
    let signature = format!("v1:{}:{}:{}", meta.len(), nanos(meta.modified()), nanos(meta.created()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!("{signature}:{}:{}:{}:{}", meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec())
    }
    #[cfg(not(unix))]
    signature
}

fn open_checked_folder_excel(path: &Path, expected_signature: &str) -> Result<fs::File, String> {
    let changed = "Файл Excel изменился или недоступен. Обновите список файлов.";
    let matches = |meta: &fs::Metadata| {
        meta.is_file() && meta.len() <= analiz_import_core::NATIVE_XLSX_ARCHIVE_LIMIT
            && native_file_signature(meta) == expected_signature
    };
    if !matches(&fs::symlink_metadata(path).map_err(err)?) { return Err(changed.into()); }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // O_NONBLOCK avoids blocking if a substituted entry is a FIFO.
        options.custom_flags(0o4000);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT, and share read access only.
        options.custom_flags(0x00200000).share_mode(1);
    }
    let file = options.open(path).map_err(err)?;
    let metadata = file.metadata().map_err(err)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { return Err(changed.into()); }
    }
    // Check the actual opened handle. A lexical path or pre-open canonicalize
    // alone cannot catch a substituted link or a redirected parent directory.
    if !matches(&metadata) || opened_file_path(&file)? != path
        || !matches(&fs::symlink_metadata(path).map_err(err)?)
    {
        return Err(changed.into());
    }
    Ok(file)
}

#[cfg(target_os = "linux")]
fn opened_file_path(file: &fs::File) -> Result<PathBuf, String> {
    use std::os::fd::AsRawFd;
    fs::read_link(Path::new("/proc/self/fd").join(file.as_raw_fd().to_string())).map_err(err)
}

#[cfg(windows)]
fn opened_file_path(file: &fs::File) -> Result<PathBuf, String> {
    use std::ffi::{c_void, OsString};
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFinalPathNameByHandleW(file: *mut c_void, path: *mut u16, size: u32, flags: u32) -> u32;
    }
    let mut path = vec![0u16; 32768];
    // The handle is valid for this call and the API receives the exact buffer length.
    let length = unsafe {
        GetFinalPathNameByHandleW(file.as_raw_handle(), path.as_mut_ptr(), path.len() as u32, 0)
    } as usize;
    if length == 0 { return Err(err(std::io::Error::last_os_error())); }
    if length >= path.len() { return Err("Excel path is too long.".into()); }
    Ok(PathBuf::from(OsString::from_wide(&path[..length])))
}

#[cfg(not(any(target_os = "linux", windows)))]
fn opened_file_path(_file: &fs::File) -> Result<PathBuf, String> {
    Err("Проверяемый автоматический импорт недоступен на этой платформе.".into())
}

impl Storage {
    pub fn new(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(err)?;
        let analysis_filter_gate = analysis_filter_gate(&root)?;
        Ok(Self {
            root,
            gate: Mutex::new(()),
            analysis_filter_gate,
            folder_import: Arc::new(Mutex::new(None)),
            folder_import_serial: AtomicU64::new(0),
        })
    }

    fn db_path(&self) -> PathBuf {
        self.root.join("reports.sqlite3")
    }

    fn connect(&self) -> Result<Connection, String> {
        let mut conn = new_connection(&self.db_path())?;
        self.migrate_legacy(&mut conn)?;
        Ok(conn)
    }

    fn migrate_legacy(&self, conn: &mut Connection) -> Result<(), String> {
        if read_key(conn, "migration_v1")?.is_some() {
            return Ok(());
        }
        // Copy the old Electron files only once; do not rename, delete or mutate them.
        let old_data = self.root.join("reports.json");
        let old_config = self.root.join("desktop-config.json");
        let reports_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM reports", [], |r| r.get(0)
        ).map_err(err)?;
        let data: Option<Value> = if reports_count == 0 && old_data.exists() {
            let bytes = fs::read(&old_data).map_err(err)?;
            if bytes.len() > MAX_PAYLOAD_BYTES {
                return Err("Прежняя база JSON слишком велика для миграции.".into());
            }
            let parsed: Value = serde_json::from_slice(&bytes).map_err(err)?;
            validate_data(&parsed)?;
            Some(parsed)
        } else {
            None
        };
        let config: Option<Config> = if old_config.exists()
            && read_key(conn, "config")?.is_none() {
            let text = fs::read_to_string(&old_config).map_err(err)?;
            let parsed: Config = serde_json::from_str(&text).map_err(err)?;
            if parsed.version != 1 {
                return Err("Неизвестная версия прежней конфигурации.".into());
            }
            for (name, rule) in &parsed.rules {
                if !safe_excel_name(name) { return Err("Небезопасное имя файла в прежней конфигурации.".into()); }
                validate_rule(rule)?;
            }
            Some(parsed)
        } else {
            None
        };
        let tx = conn.transaction().map_err(err)?;
        if let Some(data) = data {
            write_data(&tx, &data)?;
        }
        if let Some(config) = config {
            insert_key(&tx, "config", &serde_json::to_string(&config).map_err(err)?)?;
        }
        insert_key(&tx, "migration_v1", "\"complete\"")?;
        tx.commit().map_err(err)?;
        Ok(())
    }

    pub fn load(&self) -> Result<Value, String> {
        let _guard = self.gate.lock().map_err(err)?;
        let conn = self.connect()?;
        let mut stmt = conn.prepare("SELECT json FROM reports ORDER BY period_start,id").map_err(err)?;
        let mut rows = stmt.query([]).map_err(err)?;
        let mut reports = Vec::new();
        while let Some(row) = rows.next().map_err(err)? {
            let raw = row.get_ref(0).map_err(err)?;
            let bytes = match raw {
                rusqlite::types::ValueRef::Text(b) | rusqlite::types::ValueRef::Blob(b) => b,
                _ => return Err("Invalid stored report JSON type.".into()),
            };
            reports.push(serde_json::from_slice::<Value>(bytes).map_err(err)?);
        }
        let settings = match read_key(&conn, "settings")? {
            Some(text) => serde_json::from_str::<Value>(&text).map_err(err)?,
            None => default_settings(),
        };
        Ok(json!({ "reports":reports, "settings":settings }))
    }

    pub fn save(&self, data: Value) -> Result<Value, String> {
        validate_data(&data)?;
        let _guard = self.gate.lock().map_err(err)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(err)?;
        write_data(&tx, &data)?;
        tx.commit().map_err(err)?;
        Ok(json!({ "ok":true,"dataFile":self.db_path() }))
    }

    fn config(&self, conn: &Connection) -> Result<Config, String> {
        match read_key(conn, "config")? {
            Some(text) => {
                let config: Config = serde_json::from_str(&text).map_err(err)?;
                if config.version != 1 { return Err("Неизвестная версия конфигурации.".into()); }
                Ok(config)
            }
            None => Ok(Config::default()),
        }
    }

    pub fn info(&self) -> Result<Value, String> {
        let _guard = self.gate.lock().map_err(err)?;
        let conn = self.connect()?;
        let config = self.config(&conn)?;
        Ok(json!({
            "folder":config.folder,
            "rules":config.rules,
            "dataFile":self.db_path(),
            "configFile":self.db_path()
        }))
    }

    pub fn set_folder(&self, folder: String) -> Result<Value, String> {
        let resolved = fs::canonicalize(&folder).map_err(err)?;
        if !resolved.is_dir() { return Err("Выбранный путь не является папкой.".into()); }
        let _guard = self.gate.lock().map_err(err)?;
        let mut conn = self.connect()?;
        let mut config = self.config(&conn)?;
        let new_folder = resolved.to_string_lossy().to_string();
        if config.folder.as_deref() != Some(&new_folder) {
            config.rules.clear();
        }
        config.folder = Some(new_folder.clone());
        let tx = conn.transaction().map_err(err)?;
        insert_key(&tx, "config", &serde_json::to_string(&config).map_err(err)?)?;
        tx.commit().map_err(err)?;
        Ok(json!({
            "folder":new_folder, "rules":config.rules,
            "dataFile":self.db_path(),"configFile":self.db_path()
        }))
    }

    pub fn set_rule(&self, name: String, rule: Rule) -> Result<Rule, String> {
        if !safe_excel_name(&name) { return Err("Некорректное имя файла XLSX.".into()); }
        validate_rule(&rule)?;
        let _guard = self.gate.lock().map_err(err)?;
        let mut conn = self.connect()?;
        let mut config = self.config(&conn)?;
        if config.folder.is_none() { return Err("Сначала выберите папку Excel.".into()); }
        config.rules.insert(name, rule.clone());
        let tx = conn.transaction().map_err(err)?;
        insert_key(&tx, "config", &serde_json::to_string(&config).map_err(err)?)?;
        tx.commit().map_err(err)?;
        Ok(rule)
    }

    /// Update an Excel import rule only for the exact folder, file metadata
    /// and database revision presented to the SQL-only frontend.
    /// This edits config metadata, NEVER imports XLSX bytes or report rows.
    pub fn set_rule_checked(
        &self,
        name: String,
        rule: Rule,
        expected_revision: i64,
        expected_folder: String,
        expected_signature: String,
    ) -> Result<i64, String> {
        if !safe_excel_name(&name) {
            return Err("Некорректное имя файла XLSX.".into());
        }
        validate_rule(&rule)?;
        if expected_folder.is_empty() || expected_folder.len() > 32767
            || expected_signature.is_empty() || expected_signature.len() > 256
        {
            return Err("Некорректные данные исходной папки или XLSX.".into());
        }
        let _guard = self.gate.lock().map_err(err)?;
        let mut conn = self.connect()?;
        let mut config = self.config(&conn)?;
        let folder = config.folder.as_deref().ok_or("Сначала выберите папку Excel.")?;
        if folder != expected_folder {
            return Err("Папка Excel изменилась. Обновите список файлов.".into());
        }
        let path = Path::new(folder).join(&name);
        let meta = fs::symlink_metadata(&path).map_err(err)?;
        if !meta.file_type().is_file()
            || meta.len() > analiz_import_core::NATIVE_XLSX_ARCHIVE_LIMIT
            || native_file_signature(&meta) != expected_signature
        {
            return Err("Файл Excel изменился или недоступен. Обновите список файлов.".into());
        }
        let tx = conn.transaction().map_err(err)?;
        read_model::check_revision(&tx, expected_revision)?;
        config.rules.insert(name, rule);
        insert_key(&tx, "config", &serde_json::to_string(&config).map_err(err)?)?;
        tx.commit().map_err(err)?;
        read_model::revision(&conn)
    }

    pub fn set_folder_import_session(&self, owner: &str, enabled: bool) -> Result<Option<String>, String> {
        let mut session = self.folder_import.lock().map_err(err)?;
        if session.as_ref().is_some_and(|current| current.owner != owner) {
            return Err("Auto import is controlled by another window.".into());
        }
        if let Some(previous) = session.take() {
            if let Some(active) = previous.active { active.cancel(); }
        }
        if !enabled { return Ok(None); }
        let token = self.folder_import_serial.fetch_add(1, Ordering::Relaxed).to_string();
        *session = Some(FolderImportSession { token:token.clone(), owner:owner.into(), active:None });
        Ok(Some(token))
    }

    pub fn begin_folder_import(&self, owner: &str, request: &FolderImportRequest) -> Result<FolderImportOperation<'_>, String> {
        if !safe_excel_name(&request.name) || request.folder.is_empty() || request.folder.len() > 32767
            || request.revision < 0 || request.signature.is_empty() || request.signature.len() > 256
        {
            return Err("Некорректные данные исходной папки или XLSX.".into());
        }
        let mut session = self.folder_import.lock().map_err(err)?;
        let session = session.as_mut().filter(|s| s.owner == owner && s.token == request.session)
            .ok_or("Автоматический импорт выключен.")?;
        if session.active.is_some() { return Err("Автоматический импорт уже выполняется.".into()); }
        let path = Path::new(&request.folder).join(&request.name);
        let signature = request.signature.clone();
        let job = Arc::new(ImportJob::for_checked_source(move || {
            open_checked_folder_excel(&path, &signature).map(|_| ())
        }));
        session.active = Some(job.clone());
        Ok(FolderImportOperation { store:self, job })
    }

    pub fn checked_folder_import(&self, request: &FolderImportRequest) -> Result<(Rule, PathBuf, fs::File), String> {
        let _guard = self.gate.lock().map_err(err)?;
        let conn = self.connect()?;
        let tx = conn.unchecked_transaction().map_err(err)?;
        read_model::check_revision(&tx, request.revision)?;
        let config = self.config(&tx)?;
        if config.folder.as_deref() != Some(&request.folder) {
            return Err("Папка Excel изменилась. Обновите список файлов.".into());
        }
        let rule = config.rules.get(&request.name).ok_or("Для файла Excel нет сохранённого правила.")?;
        validate_rule(rule)?;
        let path = Path::new(&request.folder).join(&request.name);
        let handle = open_checked_folder_excel(&path, &request.signature)?;
        Ok((rule.clone(), path, handle))
    }

    pub fn folder_import_organizations(&self, revision: i64) -> Result<[String; 4], String> {
        let indexed: bool = {
            let _guard = self.gate.lock().map_err(err)?;
            let conn = self.connect()?;
            let tx = conn.unchecked_transaction().map_err(err)?;
            read_model::check_revision(&tx, revision)?;
            tx.query_row("SELECT EXISTS(SELECT 1 FROM management_settings)", [], |r| r.get(0)).map_err(err)?
        };
        if !indexed {
            self.management_index_settings(revision, &ManagementJob::new())?;
        }
        self.management_bootstrap(revision)?.org_names.try_into()
            .map_err(|_| "Для Excel требуются четыре организации.".into())
    }

    /// Resolve only the book selected by the user. The connected folder can be offline.
    pub fn import_folder_context(&self, source: &Path) -> Result<Option<Value>, String> {
        if !source.is_absolute() || !source.extension().and_then(|v| v.to_str())
            .is_some_and(|v| v.eq_ignore_ascii_case("xlsx")) {
            return Err("Выберите файл XLSX с полным путём.".into());
        }
        let conn = self.connect()?;
        let snapshot = conn.unchecked_transaction().map_err(err)?;
        let config = self.config(&snapshot)?;
        let Some(folder) = config.folder else { return Ok(None) };
        let revision = read_model::revision(&snapshot)?;
        snapshot.commit().map_err(err)?;

        let meta = fs::symlink_metadata(source).map_err(err)?;
        if !meta.is_file() { return Ok(None); }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 { return Ok(None); }
        }
        // set_folder stores the OS canonical name. Resolve the picker result in
        // the same way, preserving case-sensitive directories and UNC paths.
        let path = fs::canonicalize(source).map_err(err)?;
        if path.parent() != Some(Path::new(&folder)) { return Ok(None); }
        let Some(name) = path.file_name().and_then(|s| s.to_str()).filter(|s| safe_excel_name(s))
            else { return Ok(None) };
        let signature = native_file_signature(&meta);
        Ok(Some(json!({"folder":folder,"revision":revision,"name":name,
            "nativeSignature":signature,"rule":config.rules.get(name)})))
    }

    pub fn list_excel(&self) -> Result<Value, String> {
        let _guard = self.gate.lock().map_err(err)?;
        let conn = self.connect()?;
        // Read the config and revision under one SQLite read transaction.
        // Release it before filesystem enumeration so large directories do
        // not hold a database read lock while their metadata is scanned.
        let snapshot = conn.unchecked_transaction().map_err(err)?;
        let config = self.config(&snapshot)?;
        let current_revision = read_model::revision(&snapshot)?;
        snapshot.commit().map_err(err)?;
        let mut files = Vec::new();
        if let Some(ref folder) = config.folder {
            for entry in fs::read_dir(folder).map_err(err)? {
                let entry = entry.map_err(err)?;
                if !entry.file_type().is_ok_and(|kind| kind.is_file()) { continue; }
                let Ok(name) = entry.file_name().into_string() else { continue; };
                if !safe_excel_name(&name) { continue; }
                let Ok(meta) = entry.metadata() else { continue; };
                if !meta.is_file() { continue; }
                files.push(ExcelFile {
                    name, size:meta.len(),signature:file_signature(&meta)?,
                    native_signature:native_file_signature(&meta),
                    too_large:meta.len()>MAX_EXCEL_BYTES,
                    native_too_large:meta.len()>analiz_import_core::NATIVE_XLSX_ARCHIVE_LIMIT,
                });
            }
            files.sort_by(|a,b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        }
        serde_json::to_value(Listing {
            folder: config.folder,
            revision: current_revision,
            files,
            rules: config.rules,
        }).map_err(err)
    }

    fn source_path(&self, conn: &Connection, name: &str) -> Result<PathBuf, String> {
        if !safe_excel_name(name) { return Err("Небезопасное имя XLSX.".into()); }
        let config = self.config(conn)?;
        let folder = config.folder.ok_or("Сначала выберите папку Excel.")?;
        Ok(Path::new(&folder).join(name))
    }

    pub fn read_excel(&self, name: String) -> Result<ReadExcel, String> {
        let _guard = self.gate.lock().map_err(err)?;
        let conn = self.connect()?;
        let filename = self.source_path(&conn, &name)?;
        let before = fs::symlink_metadata(&filename).map_err(err)?;
        if !before.file_type().is_file() { return Err("Разрешён только обычный XLSX-файл.".into()); }
        if before.len()>MAX_EXCEL_BYTES { return Err("Excel превышает 30 МБ.".into()); }
        let bytes = fs::read(&filename).map_err(err)?;
        let after = fs::symlink_metadata(&filename).map_err(err)?;
        let signature = file_signature(&after)?;
        if !after.file_type().is_file()
            || file_signature(&before)? != signature
            || bytes.len() as u64 != after.len() {
            return Err("Excel менялся при чтении: повторите после копирования.".into());
        }
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        Ok(ReadExcel { bytes, signature, sha256 })
    }

    pub fn check_excel(&self, name: String, signature: String) -> bool {
        let Ok(_guard) = self.gate.lock() else { return false; };
        let Ok(conn) = self.connect() else { return false; };
        let Ok(filename) = self.source_path(&conn, &name) else { return false; };
        let Ok(meta) = fs::symlink_metadata(filename) else { return false; };
        meta.file_type().is_file()
            && file_signature(&meta).is_ok_and(|s| s==signature)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn example() -> Value {
        json!({"reports":[{
            "id":"report-1","type":"week","start":"2026-10-19","end":"2026-10-25",
            "rows":[{"work":"Работа","organization":"СМУ16","count":1.5,"amount":250}]
        }],"settings":default_settings()})
    }

    fn leap_year_source() -> Value {
        let first = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let organizations = ["СМУ16", "АВТО", "ТРАК", "ЦЕНТР"];
        let reports: Vec<Value> = (0..366).map(|offset| {
            let date = (first + chrono::Duration::days(offset)).format("%Y-%m-%d").to_string();
            let rows: Vec<Value> = organizations.iter().enumerate().map(|(index, organization)| {
                json!({"work":"Дробная работа","organization":organization,
                    "count":index as f64 + 0.25,"amount":100.125 + index as f64})
            }).collect();
            json!({"id":format!("annual-{date}"),"type":"day","start":date,"end":date,
                "desktopSource":"Год-2024.xlsx","rows":rows})
        }).collect();
        json!({"reports":reports,"settings":default_settings()})
    }

    #[test]
    fn classic_reports_keep_the_persisted_desktop_signature() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("excel");
        fs::create_dir(&folder).unwrap();
        let name = "same.xlsx";
        let source = folder.join(name);
        fs::write(&source, b"sample").unwrap();
        let store = Storage::new(temp.path().join("store")).unwrap();
        store.set_folder(folder.to_str().unwrap().into()).unwrap();
        let meta = fs::metadata(source).unwrap();
        let millis = |time: std::io::Result<std::time::SystemTime>| time.ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |x| x.as_millis());
        let persisted = format!("{}:{}:{}", meta.len(), millis(meta.modified()), millis(meta.created()));
        let listing = store.list_excel().unwrap();
        let file = &listing["files"][0];
        assert_eq!(file["signature"], persisted);
        assert_ne!(file["nativeSignature"], persisted);
        assert_eq!(store.read_excel(name.into()).unwrap().signature, persisted);
        assert!(store.check_excel(name.into(), persisted));
    }

    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn folder_import_checks_session_revision_and_source_and_cancels_at_commit() {
        use analiz_import_core::{ImportMode, LegacyOptions, Limits, StageArea, TripleMapping};
        use std::sync::atomic::Ordering;
        let temp = tempfile::tempdir().unwrap();
        let store = Storage::new(temp.path().join("data")).unwrap();
        let folder = temp.path().join("Excel");
        fs::create_dir(&folder).unwrap();
        let source = folder.join("report.xlsx");
        import_apply_tests::fixture(&source, 1);
        store.set_folder(folder.to_str().unwrap().into()).unwrap();
        let rule = Rule { format:Some("legacy".into()), kind:"day".into(), start:"2025-01-01".into(), ..Rule::default() };
        store.set_rule("report.xlsx".into(), rule.clone()).unwrap();
        let listing = store.list_excel().unwrap();
        let mut request = FolderImportRequest {
            session:"outside-window".into(),
            folder:listing["folder"].as_str().unwrap().into(),
            revision:listing["revision"].as_i64().unwrap(),
            name:"report.xlsx".into(),
            signature:listing["files"][0]["nativeSignature"].as_str().unwrap().into(),
        };
        assert_eq!(
            store.folder_import_organizations(request.revision).unwrap(),
            ["СМУ16", "АВТО", "ТРАК", "ЦЕНТР"]
        );
        assert_eq!(store.revision().unwrap(), request.revision);
        assert!(store.begin_folder_import("main", &request).is_err());
        request.session = store.set_folder_import_session("main", true).unwrap().unwrap();
        assert!(store.begin_folder_import("other-window", &request).is_err());
        assert!(store.set_folder_import_session("other-window", false).is_err());
        let operation = store.begin_folder_import("main", &request).unwrap();
        assert!(store.begin_folder_import("main", &request).is_err());
        request.name = "../report.xlsx".into();
        assert!(store.begin_folder_import("main", &request).is_err());
        request.name = "report.xlsx".into();
        let (current, path, file) = store.checked_folder_import(&request).unwrap();
        assert_eq!(current.start, rule.start);
        let area = StageArea::open(temp.path().join("stage")).unwrap();
        let cancel = operation.job.cancellation();
        let mut stage = area.ingest_opened(path, file, Limits::default(), cancel, |_|{}).unwrap();
        stage.convert_legacy(&LegacyOptions {
            period_dates: None,
            sheet:"xl/worksheets/sheet1.xml".into(),
            kind:rule.kind.clone(),
            start:rule.start.clone(),
            organizations:["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()],
            mappings:Some(vec![TripleMapping { header_row:1, work_col:1, count_col:2, amount_col:3 }]),
        }, cancel, |_|{}).unwrap();
        let prepared = stage.prepare_with_mode(ImportMode::Folder, cancel, |_|{}).unwrap();
        assert_eq!(prepared.source_folder(), request.folder);
        assert_eq!(prepared.source_name(), request.name);
        let preview = store.preview_import(&prepared).unwrap();
        let out = store.apply_import(&prepared, &preview, ImportConfirmation::default(), &operation.job, |progress| {
            if progress.phase == "before_commit" {
                store.set_folder_import_session("main", false).unwrap();
            }
        });
        assert!(out.is_err());
        assert!(cancel.load(Ordering::Acquire));
        assert_eq!(store.revision().unwrap(), request.revision);
        assert!(store.begin_folder_import("main", &request).is_err());
        drop(operation);

        request.session = store.set_folder_import_session("main", true).unwrap().unwrap();
        let operation = store.begin_folder_import("main", &request).unwrap();
        let receipt = store.apply_import(&prepared, &preview, ImportConfirmation::default(), &operation.job, |_|{}).unwrap();
        assert_eq!(receipt.reports, 1);
        assert_eq!(receipt.replaced, 0);
        assert!(store.checked_folder_import(&request).is_err(), "The consumed revision is stale");
        request.revision = receipt.revision;
        assert!(store.checked_folder_import(&request).is_ok());
        let changed = Rule { start:"2025-02-01".into(), ..rule };
        store.set_rule("report.xlsx".into(), changed).unwrap();
        assert!(store.checked_folder_import(&request).is_err(), "A changed rule invalidates the snapshot");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn checked_folder_import_rejects_swapped_links_files_and_parent() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("folder");
        let outside = temp.path().join("outside");
        fs::create_dir(&folder).unwrap();
        fs::create_dir(&outside).unwrap();
        let source = folder.join("report.xlsx");
        let target = outside.join("report.xlsx");
        fs::write(&source, b"valid").unwrap();
        fs::write(&target, b"other").unwrap();
        let signature = native_file_signature(&fs::metadata(&source).unwrap());
        assert!(open_checked_folder_excel(&source, &signature).is_ok());

        fs::rename(&source, folder.join("old")).unwrap();
        symlink(&target, &source).unwrap();
        assert!(open_checked_folder_excel(&source, &signature).is_err());
        fs::remove_file(&source).unwrap();

        fs::hard_link(&target, &source).unwrap();
        assert!(open_checked_folder_excel(&source, &signature).is_err());
        fs::remove_file(&source).unwrap();
        fs::rename(&folder, temp.path().join("old-folder")).unwrap();
        symlink(&outside, &folder).unwrap();
        let target_signature = native_file_signature(&fs::metadata(&target).unwrap());
        assert!(open_checked_folder_excel(&source, &target_signature).is_err());
    }

    #[test]
    fn manual_import_resolves_a_single_file_without_enumerating_the_connected_folder() {
        let temp = tempdir().unwrap();
        let store = Storage::new(temp.path().join("db")).unwrap();
        let folder = temp.path().join("Connected");
        fs::create_dir(&folder).unwrap();
        let book = folder.join("February.xlsx");
        fs::write(&book, b"folder").unwrap();
        let outside = temp.path().join("February.xlsx");
        fs::write(&outside, b"local").unwrap();
        store.set_folder(folder.to_string_lossy().into_owned()).unwrap();
        store.set_rule("February.xlsx".into(), Rule { kind:"week".into(),start:"2024-02-05".into(),
            sheet:"Second".into(),format:Some("legacy".into()),..Default::default() }).unwrap();

        let from_picker = store.import_folder_context(&book).unwrap().unwrap();
        let canonical = fs::canonicalize(&book).unwrap();
        assert_eq!(from_picker,store.import_folder_context(&canonical).unwrap().unwrap());
        assert_eq!(from_picker["folder"].as_str(),canonical.parent().and_then(Path::to_str));
        assert_eq!(from_picker["rule"]["sheet"],"Second");
        assert_eq!(from_picker["nativeSignature"],native_file_signature(&fs::metadata(&book).unwrap()));
        #[cfg(windows)]
        {
            let alternate = folder.with_file_name("CONNECTED").join("february.XLSX");
            if alternate.exists() { assert_eq!(from_picker,store.import_folder_context(&alternate).unwrap().unwrap()); }
        }
        #[cfg(unix)]
        {
            let alias = folder.join("alias.xlsx");
            std::os::unix::fs::symlink(&outside,&alias).unwrap();
            assert!(store.import_folder_context(&alias).unwrap().is_none());
            fs::remove_file(alias).unwrap();
        }
        fs::rename(&folder,temp.path().join("Disconnected")).unwrap();
        assert!(store.list_excel().is_err());
        assert!(store.import_folder_context(&outside).unwrap().is_none());
    }

    #[test]
    fn checked_rule_saves_only_for_current_sql_revision_folder_and_file() {
        let tmp = tempdir().unwrap();
        let excel_dir = tmp.path().join("excel");
        fs::create_dir(&excel_dir).unwrap();
        let file = excel_dir.join("sample.xlsx");
        fs::write(&file, b"test workbook metadata").unwrap();
        let store = Storage::new(tmp.path().join("data")).unwrap();
        store.set_folder(excel_dir.to_string_lossy().to_string()).unwrap();
        let listing = store.list_excel().unwrap();
        let folder = listing["folder"].as_str().unwrap().to_string();
        let signature = listing["files"][0]["nativeSignature"].as_str().unwrap().to_string();
        let revision = store.revision().unwrap();
        let rule = Rule {
            kind: "week".to_string(),
            start: "2026-11-16".to_string(),
            format: Some("legacy".to_string()),
            ..Default::default()
        };
        assert!(store.set_rule_checked(
            "sample.xlsx".into(), rule.clone(), revision,
            "other-folder".into(), signature.clone()
        ).is_err());
        assert_eq!(store.revision().unwrap(), revision);
        assert!(store.set_rule_checked(
            "sample.xlsx".into(), rule.clone(), revision,
            folder.clone(), "wrong-signature".into()
        ).is_err());
        assert_eq!(store.revision().unwrap(), revision);
        let next = store.set_rule_checked(
            "sample.xlsx".into(), rule.clone(), revision,
            folder.clone(), signature.clone()
        ).unwrap();
        assert!(next > revision);
        assert_eq!(store.list_excel().unwrap()["rules"]["sample.xlsx"]["type"], "week");
        assert!(store.set_rule_checked(
            "sample.xlsx".into(), rule.clone(), revision,
            folder.clone(), signature.clone()
        ).is_err(), "stale revision must not overwrite another edit");
        let fresh = store.revision().unwrap();
        fs::write(&file, b"completely different Excel contents").unwrap();
        assert!(store.set_rule_checked(
            "sample.xlsx".into(), rule, fresh, folder, signature
        ).is_err(), "a changed source file must require a fresh selection");
        assert_eq!(store.revision().unwrap(), fresh);
    }

    #[test]
    fn native_40_mib_rule_does_not_raise_classic_browser_excel_budget() {
        let tmp = tempdir().unwrap();
        let folder = tmp.path().join("sources");
        fs::create_dir_all(&folder).unwrap();
        let source = folder.join("native-only.xlsx");
        let f = fs::File::create(&source).unwrap();
        f.set_len(40 * 1024 * 1024).unwrap(); // Sparse file; no workbook content is read.
        let store = Storage::new(tmp.path().join("db")).unwrap();
        store.set_folder(folder.to_string_lossy().to_string()).unwrap();
        let listing = store.list_excel().unwrap();
        assert_eq!(listing["files"][0]["tooLarge"], true);
        assert_eq!(listing["files"][0]["nativeTooLarge"], false);
        assert!(store.read_excel("native-only.xlsx".into()).is_err());
        let rule = Rule {
            kind: "week".into(),
            start: "2026-11-16".into(),
            format: Some("legacy".into()),
            ..Default::default()
        };
        let rev = listing["revision"].as_i64().unwrap();
        let sig = listing["files"][0]["nativeSignature"].as_str().unwrap();
        // On Windows, set_folder canonicalizes to a verbatim ("\\\\?\\") path;
        // versioned rule writes must pass that exact folder from the snapshot.
        let selected_folder = listing["folder"].as_str().unwrap().to_string();
        let saved = store.set_rule_checked("native-only.xlsx".into(),
            rule.clone(), rev, selected_folder.clone(),
            sig.into());
        assert!(saved.is_ok(), "40-MiB native rule was rejected: {:?}", saved.err());
        f.set_len(101 * 1024 * 1024).unwrap();
        let refreshed = store.list_excel().unwrap();
        assert_eq!(refreshed["files"][0]["nativeTooLarge"], true);
        assert_eq!(refreshed["files"][0]["tooLarge"], true);
        let current = refreshed["revision"].as_i64().unwrap();
        assert!(store.set_rule_checked("native-only.xlsx".into(), rule, current,
            refreshed["folder"].as_str().unwrap().to_string(),
            refreshed["files"][0]["nativeSignature"].as_str().unwrap().into()).is_err());
        assert_eq!(store.revision().unwrap(), current);
    }

    #[test]
    fn full_leap_year_round_trips_and_duplicate_replacement_rolls_back() {
        let tmp = tempdir().unwrap();
        let original = leap_year_source();
        let store = Storage::new(tmp.path().to_path_buf()).unwrap();
        store.save(original.clone()).unwrap();
        drop(store);
        let reopened = Storage::new(tmp.path().to_path_buf()).unwrap();
        let saved = reopened.load().unwrap();
        assert_eq!(saved, original);
        assert_eq!(saved["reports"].as_array().unwrap().len(), 366);
        assert_eq!(saved["reports"][59]["start"], "2024-02-29");
        assert_eq!(saved["reports"][365]["end"], "2024-12-31");
        assert_eq!(saved["reports"][365]["rows"][3]["count"], 3.25);
        assert_eq!(saved["reports"][365]["rows"][3]["amount"], 103.125);

        let mut replacement = original.clone();
        replacement["reports"][0]["rows"][0]["amount"] = json!(999.875);
        replacement["settings"]["catalog"] = json!(["replacement"]);
        // Fail at the final insert after DELETE and 365 successful new inserts.
        let duplicate_id = replacement["reports"][0]["id"].clone();
        replacement["reports"][365]["id"] = duplicate_id;
        assert!(reopened.save(replacement).is_err());
        assert_eq!(reopened.load().unwrap(), original);
        drop(reopened);
        assert_eq!(Storage::new(tmp.path().to_path_buf()).unwrap().load().unwrap(), original);
    }

    #[test]
    fn sqlite_round_trip_and_reopen() {
        let tmp = tempdir().unwrap();
        Storage::new(tmp.path().to_path_buf()).unwrap().save(example()).unwrap();
        let reopened = Storage::new(tmp.path().to_path_buf()).unwrap();
        assert_eq!(reopened.load().unwrap()["reports"][0]["rows"][0]["amount"],250);
        assert!(tmp.path().join("reports.sqlite3").exists());
    }

    #[test]
    fn invalid_report_cannot_replace_good_data() {
        let tmp = tempdir().unwrap();
        let store = Storage::new(tmp.path().to_path_buf()).unwrap();
        store.save(example()).unwrap();
        let mut invalid = example();
        invalid["reports"][0]["rows"][0]["amount"] = Value::Null;
        assert!(store.save(invalid).is_err());
        assert_eq!(store.load().unwrap()["reports"][0]["rows"][0]["amount"],250);
    }

    #[test]
    fn imports_legacy_without_deleting_json() {
        let tmp = tempdir().unwrap();
        let source = tmp.path().join("reports.json");
        fs::write(&source, example().to_string()).unwrap();
        let store = Storage::new(tmp.path().to_path_buf()).unwrap();
        assert_eq!(store.load().unwrap()["reports"].as_array().unwrap().len(),1);
        assert!(source.exists());
        assert_eq!(store.load().unwrap()["reports"].as_array().unwrap().len(),1);
    }

    #[test]
    fn invalid_legacy_refuses_silent_reset() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("reports.json"), "{broken").unwrap();
        let store = Storage::new(tmp.path().to_path_buf()).unwrap();
        assert!(store.load().is_err());
        assert!(tmp.path().join("reports.json").exists());
    }

    #[test]
    fn filename_and_period_validation() {
        for unsafe_name in ["../x.xlsx", "~$lock.xlsx", "name:stream.xlsx", r"dir\x.xlsx","a.xls"] {
            assert!(!safe_excel_name(unsafe_name));
        }
        assert!(safe_excel_name("Анализ.xlsx"));
        // Electron uses JavaScript string.length (UTF-16 code units), not UTF-8 bytes.
        assert!(safe_excel_name(&format!("{}.xlsx", "Я".repeat(235))));
        assert!(!safe_excel_name(&format!("{}.xlsx", "Я".repeat(236))));
        assert!(safe_excel_name(&format!("{}.xlsx", "😀".repeat(117))));
        assert!(!safe_excel_name(&format!("{}.xlsx", "😀".repeat(118))));
        assert!(!valid_date("2026-02-30"));
        assert!(valid_date("2026-10-19"));
    }

    #[test]
    fn long_cyrillic_legacy_filename_migrates_and_remains_visible() {
        let tmp = tempdir().unwrap();
        let folder = tmp.path().join("Excel");
        fs::create_dir_all(&folder).unwrap();
        let name = format!("{}.xlsx", "Я".repeat(121));
        assert!(name.len() > 240);
        fs::write(folder.join(&name), b"example").unwrap();
        let mut rules = serde_json::Map::new();
        rules.insert(name.clone(), json!({"type":"day","start":"2026-11-16","sheet":""}));
        let config = json!({
            "version":1,
            "folder":folder.to_string_lossy().to_string(),
            "rules":rules
        });
        fs::write(tmp.path().join("desktop-config.json"), config.to_string()).unwrap();
        let store = Storage::new(tmp.path().to_path_buf()).unwrap();
        let info = store.info().unwrap();
        assert_eq!(info["rules"][name.as_str()]["type"], "day");
        let listing = store.list_excel().unwrap();
        assert_eq!(listing["files"][0]["name"], name);
        assert_eq!(store.read_excel(name).unwrap().bytes, b"example");
    }

    #[test]
    fn daily_period_saves_in_sqlite_and_rule_matches_browser_import() {
        let tmp = tempdir().unwrap();
        let folder = tmp.path().join("Excel");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("День-2026-11-16.xlsx"), b"sample").unwrap();
        let store = Storage::new(tmp.path().join("database")).unwrap();
        let mut data = example();
        data["reports"][0]["type"] = json!("day");
        data["reports"][0]["start"] = json!("2026-11-16");
        data["reports"][0]["end"] = json!("2026-11-16");
        store.save(data).unwrap();
        assert_eq!(store.load().unwrap()["reports"][0]["type"], "day");
        store.set_folder(folder.to_string_lossy().to_string()).unwrap();
        let rule=Rule {
            kind: "day".into(), start: "2026-11-16".into(), sheet: String::new(), ..Rule::default()
        };
        store.set_rule("День-2026-11-16.xlsx".into(), rule).unwrap();
        let info=store.info().unwrap();
        assert_eq!(info["rules"]["День-2026-11-16.xlsx"]["type"], "day");
        assert_eq!(info["rules"]["День-2026-11-16.xlsx"]["start"], "2026-11-16");
        assert_eq!(store.list_excel().unwrap()["files"].as_array().unwrap().len(),1);
    }

    #[test]
    fn legacy_daily_monthly_and_weekly_reports_migrate_without_modifying_json() {
        let tmp = tempdir().unwrap();
        let mut old = example();
        let mut day=old["reports"][0].clone();
        day["id"]=json!("day");
        day["type"]=json!("day");
        day["start"]=json!("2026-11-16");
        day["end"]=json!("2026-11-16");
        let mut month=old["reports"][0].clone();
        month["id"]=json!("month");
        month["type"]=json!("month");
        month["start"]=json!("2026-11-01");
        month["end"]=json!("2026-11-30");
        old["reports"].as_array_mut().unwrap().extend([day,month]);
        let source=old.to_string();
        fs::write(tmp.path().join("reports.json"),&source).unwrap();
        let config=json!({
            "version":1,
            "folder":null,
            "rules":{"Дневной.xlsx":{
                "type":"day","start":"2026-11-16","sheet":""
            }}
        }).to_string();
        fs::write(tmp.path().join("desktop-config.json"),&config).unwrap();
        let store=Storage::new(tmp.path().to_path_buf()).unwrap();
        let loaded=store.load().unwrap();
        assert_eq!(loaded["reports"].as_array().unwrap().len(),3);
        assert_eq!(store.info().unwrap()["rules"]["Дневной.xlsx"]["type"],"day");
        assert_eq!(fs::read_to_string(tmp.path().join("reports.json")).unwrap(),source);
        assert_eq!(fs::read_to_string(tmp.path().join("desktop-config.json")).unwrap(),config);
        assert_eq!(store.load().unwrap()["reports"].as_array().unwrap().len(),3);
    }

    #[test]
    fn unknown_report_type_does_not_change_saved_data() {
        let tmp = tempdir().unwrap();
        let store=Storage::new(tmp.path().to_path_buf()).unwrap();
        store.save(example()).unwrap();
        let mut unknown=example();
        unknown["reports"][0]["type"]=json!("quarter");
        assert!(store.save(unknown).is_err());
        assert_eq!(store.load().unwrap()["reports"][0]["type"],"week");
        assert!(validate_rule(&Rule {
            kind:"quarter".into(),start:"2026-11-16".into(),sheet:String::new(),..Rule::default()
        }).is_err());
    }

    #[test]
    fn daily_wide_config_migrates_and_round_trips_with_legacy_rules() {
        let tmp = tempdir().unwrap();
        let excel = tmp.path().join("Excel");
        fs::create_dir_all(&excel).unwrap();
        let legacy = json!({"type":"week","start":"2026-11-16","sheet":""});
        let wide = json!({"type":"day","start":"","sheet":"","format":"daily-wide","year":2026,
            "sheets":["Январь"],"mappings":{"Январь":{
                "workCol":0,"headerRow":3,"firstDayCol":1,"totalAmountCol":64
            }}});
        fs::write(tmp.path().join("desktop-config.json"), json!({
            "version":1,"folder":excel.to_string_lossy(),
            "rules":{"legacy.xlsx":legacy,"annual.xlsx":wide}
        }).to_string()).unwrap();
        let store = Storage::new(tmp.path().to_path_buf()).unwrap();
        assert_eq!(store.info().unwrap()["rules"]["legacy.xlsx"], legacy);
        assert_eq!(store.info().unwrap()["rules"]["annual.xlsx"], wide);
        let rule: Rule = serde_json::from_value(wide.clone()).unwrap();
        store.set_rule("annual.xlsx".into(), rule).unwrap();
        let reopened = Storage::new(tmp.path().to_path_buf()).unwrap();
        assert_eq!(reopened.info().unwrap()["rules"]["annual.xlsx"], wide);
        assert_eq!(reopened.info().unwrap()["rules"]["legacy.xlsx"], legacy);
        let mut invalid: Rule = serde_json::from_value(wide.clone()).unwrap();
        invalid.year = Some(1899);
        assert!(reopened.set_rule("annual.xlsx".into(), invalid).is_err());
        assert_eq!(reopened.info().unwrap()["rules"]["annual.xlsx"], wide);
    }

    #[test]
    fn explicit_import_format_survives_sqlite_round_trip() {
        let tmp = tempdir().unwrap();
        let store = Storage::new(tmp.path().to_path_buf()).unwrap();
        store.set_folder(tmp.path().to_string_lossy().to_string()).unwrap();
        for format in ["auto", "legacy"] {
            let value = json!({"type":"week","start":"2024-01-01","sheet":"Legacy","format":format});
            let rule: Rule = serde_json::from_value(value.clone()).unwrap();
            store.set_rule(format!("{format}.xlsx"), rule).unwrap();
            let reopened = Storage::new(tmp.path().to_path_buf()).unwrap();
            assert_eq!(reopened.info().unwrap()["rules"][format!("{format}.xlsx")], value);
        }
    }

    #[test]
    fn malformed_daily_wide_options_are_rejected() {
        let base = json!({"type":"day","start":"","sheet":"","format":"daily-wide"});
        for extra in [
            json!({"format":"annual"}), json!({"type":"month"}),
            json!({"start":"2026-02-30"}), json!({"year":1899}), json!({"year":10000}),
            json!({"year":2026.5}), json!({"year":null}), json!({"sheets":[""]}),
            json!({"sheets":vec!["Лист";13]}), json!({"sheets":"Лист"}),
            json!({"mappings":[]}), json!({"mappings":null}),
            json!({"mappings":{"Sheet":{"workCol":0,"headerRow":60000,"firstDayCol":1}}}),
            json!({"mappings":{"Sheet":{"workCol":2048,"headerRow":0,"firstDayCol":1}}}),
            json!({"mappings":{"Sheet":{"workCol":-1,"headerRow":0,"firstDayCol":1}}}),
            json!({"mappings":{"Sheet":{"headerRow":0,"firstDayCol":1}}}),
            json!({"mappings":{"Sheet":{"workCol":0,"headerRow":0,"firstDayCol":1,"totalCountCol":"2"}}}),
        ] {
            let mut value = base.clone();
            value.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            let rejected = match serde_json::from_value::<Rule>(value.clone()) {
                Err(_) => true,
                Ok(rule) => validate_rule(&rule).is_err(),
            };
            assert!(rejected, "accepted malformed rule: {value}");
        }
        let boundary: Rule = serde_json::from_value(json!({
            "type":"day","start":"","sheet":"","format":"daily-wide","year":9999,
            "mappings":{"Sheet":{"workCol":2047,"headerRow":59999,"firstDayCol":2047,
                "totalCountCol":null,"totalAmountCol":2047}}
        })).unwrap();
        assert!(validate_rule(&boundary).is_ok());
    }

    #[test]
    fn disk_folder_scans_and_reads_only_safe_xlsx() {
        let tmp = tempdir().unwrap();
        let excel = tmp.path().join("source");
        fs::create_dir_all(&excel).unwrap();
        fs::write(excel.join("Анализ.xlsx"), b"demo").unwrap();
        fs::write(excel.join("~$temp.xlsx"), b"skip").unwrap();
        let store = Storage::new(tmp.path().join("data")).unwrap();
        store.set_folder(excel.to_string_lossy().to_string()).unwrap();
        let list=store.list_excel().unwrap();
        assert_eq!(list["files"].as_array().unwrap().len(),1);
        let bytes=store.read_excel("Анализ.xlsx".into()).unwrap();
        assert_eq!(bytes.bytes,b"demo");
        assert!(store.check_excel("Анализ.xlsx".into(),bytes.signature));
    }
}

#[cfg(test)]
#[path = "read_model_tests.rs"]
mod read_model_tests;

#[cfg(test)]
#[path = "import_apply_tests.rs"]
mod import_apply_tests;

#[cfg(test)]
#[path = "import_wide_apply_tests.rs"]
mod import_wide_apply_tests;
#[cfg(test)]
#[path = "analytics_tests.rs"]
mod analytics_tests;

#[cfg(test)]
#[path = "management_tests.rs"]
mod management_tests;
