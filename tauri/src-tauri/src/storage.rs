use chrono::NaiveDate;
use rusqlite::{params, Connection, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::UNIX_EPOCH,
};

const MAX_EXCEL_BYTES: u64 = 30 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: usize = 200 * 1024 * 1024;
// Keep synchronized with REPORT_TYPES in the root index.html.
const REPORT_TYPES: [&str; 3] = ["week", "month", "day"];

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    #[serde(rename = "type")]
    pub kind: String,
    pub start: String,
    #[serde(default)]
    pub sheet: String,
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
    too_large: bool,
}

#[derive(Serialize)]
struct Listing {
    folder: Option<String>,
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

pub struct Storage {
    root: PathBuf,
    gate: Mutex<()>,
}

fn valid_date(input: &str) -> bool {
    input.len() == 10
        && input.as_bytes().get(4) == Some(&b'-')
        && input.as_bytes().get(7) == Some(&b'-')
        && NaiveDate::parse_from_str(input, "%Y-%m-%d").is_ok()
}

fn safe_excel_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 240
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
    if !valid_date(&rule.start) || (rule.kind == "month" && !rule.start.ends_with("-01")) {
        return Err("Некорректная дата первого периода.".into());
    }
    if rule.sheet.len() > 120 {
        return Err("Название листа Excel слишком длинное.".into());
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

fn new_connection(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(err)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=FULL;
         CREATE TABLE IF NOT EXISTS reports (
           id TEXT PRIMARY KEY,
           report_type TEXT NOT NULL,
           period_start TEXT NOT NULL,
           period_end TEXT NOT NULL,
           source_name TEXT,
           json TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS reports_by_period
           ON reports(report_type, period_start);
         CREATE INDEX IF NOT EXISTS reports_by_source
           ON reports(source_name);
         CREATE TABLE IF NOT EXISTS kv (
           name TEXT PRIMARY KEY,
           json TEXT NOT NULL
         );",
    ).map_err(err)?;
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
        Some(row) => row.get::<_, String>(0).map(Some).map_err(err),
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
    }
    drop(stmt);
    insert_key(tx, "settings", &data["settings"].to_string())?;
    Ok(())
}

fn file_signature(meta: &fs::Metadata) -> Result<String, String> {
    let millis = |time: Result<std::time::SystemTime, std::io::Error>| {
        time.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|x| x.as_millis()).unwrap_or(0)
    };
    Ok(format!("{}:{}:{}", meta.len(), millis(meta.modified()), millis(meta.created())))
}

impl Storage {
    pub fn new(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(err)?;
        Ok(Self { root, gate: Mutex::new(()) })
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
        let rows = stmt.query_map([], |row| row.get::<_, String>(0)).map_err(err)?;
        let mut reports = Vec::new();
        for row in rows {
            reports.push(serde_json::from_str::<Value>(&row.map_err(err)?).map_err(err)?);
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

    pub fn list_excel(&self) -> Result<Value, String> {
        let _guard = self.gate.lock().map_err(err)?;
        let conn = self.connect()?;
        let config = self.config(&conn)?;
        let mut files = Vec::new();
        if let Some(ref folder) = config.folder {
            for entry in fs::read_dir(folder).map_err(err)? {
                let entry = entry.map_err(err)?;
                if !entry.file_type().map_err(err)?.is_file() { continue; }
                let name = entry.file_name().to_string_lossy().to_string();
                if !safe_excel_name(&name) { continue; }
                let meta = entry.metadata().map_err(err)?;
                files.push(ExcelFile {
                    name, size:meta.len(),signature:file_signature(&meta)?,
                    too_large:meta.len()>MAX_EXCEL_BYTES,
                });
            }
            files.sort_by(|a,b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        }
        serde_json::to_value(Listing { folder:config.folder, files, rules:config.rules }).map_err(err)
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
        assert!(!valid_date("2026-02-30"));
        assert!(valid_date("2026-10-19"));
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
            kind: "day".into(), start: "2026-11-16".into(), sheet: String::new()
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
            kind:"quarter".into(),start:"2026-11-16".into(),sheet:String::new()
        }).is_err());
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
