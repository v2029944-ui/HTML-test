//! Bounded settings domain; deliberately not registered as Tauri commands.
use super::management_tree as tree;
use super::read_model::{check_metadata_bounds, check_revision};
use super::*;
use fs2::FileExt;
use rusqlite::OptionalExtension;
use std::{
    io::{BufReader, BufWriter, Read, Write},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};
const TOKEN: usize = 65536;
const SESSION_MAGIC: &str = "analiz-management-v1\n";
pub(super) fn label(s: &str) -> Result<(), String> {
    if s.len() > TOKEN || serde_json::to_string(s).map_err(err)?.len() > TOKEN + 2 {
        Err("Management string/raw token exceeds64KiB".into())
    } else {
        Ok(())
    }
}
fn batch(n: usize) -> Result<(), String> {
    if (1..=500).contains(&n) {
        Ok(())
    } else {
        Err("Management batch/page must be1..=500".into())
    }
}
fn search(s: &str) -> Result<(), String> {
    if s.len() > 512 {
        Err("Management search exceeds512UTF8bytes".into())
    } else {
        Ok(())
    }
}
fn num(n: f64) -> Result<f64, String> {
    if n.is_finite() {
        Ok(n)
    } else {
        Err("Management numeric overflow".into())
    }
}
fn count(c: &Connection, table: &str) -> Result<i64, String> {
    c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .map_err(err)
}
pub(super) fn initialize(c: &Connection) -> Result<(), String> {
    tree::schema(c)?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS management_report_meta(report_id TEXT PRIMARY KEY,amount REAL NOT NULL,saved_amount REAL,warnings INTEGER NOT NULL,shared_source_artifact INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS management_report_orgs(report_id TEXT,ordinal INTEGER,org TEXT,PRIMARY KEY(report_id,ordinal)) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS management_report_org_lookup ON management_report_orgs(org,report_id);
CREATE TRIGGER IF NOT EXISTS management_settings_insert AFTER INSERT ON kv WHEN NEW.name='settings' BEGIN DELETE FROM management_settings;END;
CREATE TRIGGER IF NOT EXISTS management_settings_update AFTER UPDATE ON kv WHEN OLD.name='settings' OR NEW.name='settings' BEGIN DELETE FROM management_settings;END;
CREATE TRIGGER IF NOT EXISTS management_settings_delete AFTER DELETE ON kv WHEN OLD.name='settings' BEGIN DELETE FROM management_settings;END;
CREATE TRIGGER IF NOT EXISTS management_report_insert AFTER INSERT ON reports BEGIN DELETE FROM management_report_meta WHERE report_id=NEW.id;DELETE FROM management_report_orgs WHERE report_id=NEW.id;END;
CREATE TRIGGER IF NOT EXISTS management_report_update AFTER UPDATE ON reports BEGIN DELETE FROM management_report_meta WHERE report_id IN(OLD.id,NEW.id);DELETE FROM management_report_orgs WHERE report_id IN(OLD.id,NEW.id);END;
CREATE TRIGGER IF NOT EXISTS management_report_delete AFTER DELETE ON reports BEGIN DELETE FROM management_report_meta WHERE report_id=OLD.id;DELETE FROM management_report_orgs WHERE report_id=OLD.id;END;").map_err(err)
}
#[derive(Default)]
pub struct ManagementJob {
    state: Mutex<u8>,
    cancelled: AtomicBool,
    records: AtomicU64,
    interrupt: Mutex<Option<rusqlite::InterruptHandle>>,
}
impl ManagementJob {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn records_processed(&self) -> u64 {
        self.records.load(Ordering::Relaxed)
    }
    pub(super) fn record(&self) {
        self.records.fetch_add(1, Ordering::Relaxed);
    }
    pub fn cancel(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if *state == 2 {
            return false;
        }
        *state = 1;
        self.cancelled.store(true, Ordering::Release);
        if let Some(h) = self
            .interrupt
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            h.interrupt();
        }
        true
    }
    pub(super) fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            Err("Management cancelled before commit".into())
        } else {
            Ok(())
        }
    }
    fn run<T>(&self, c: &Connection, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        self.check()?;
        *self.interrupt.lock().map_err(err)? = Some(c.get_interrupt_handle());
        struct Guard<'a>(&'a ManagementJob);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                *self.0.interrupt.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
        }
        let _guard = Guard(self);
        self.check()?;
        f()
    }
    fn finish(&self, tx: Transaction<'_>) -> Result<(), String> {
        let mut state = self.state.lock().map_err(err)?;
        if *state != 0 {
            return Err("Management cancelled or already committed".into());
        }
        tx.commit().map_err(err)?;
        *state = 2;
        Ok(())
    }
}
#[derive(Debug, Serialize)]
pub struct ManagementReceipt {
    pub revision: i64,
    pub deleted: i64,
    pub cleanup_error: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct ManagementBootstrap {
    pub revision: i64,
    pub org_names: Vec<String>,
    pub groups: i64,
    pub memberships: i64,
    pub catalog: i64,
    pub reports: i64,
    pub indexed_reports: i64,
}
#[derive(Debug, Serialize)]
pub struct ManagementGroup {
    pub handle: i64,
    pub ordinal: i64,
    pub id: String,
    pub name: String,
    pub members: i64,
    pub unique_members: i64,
}
#[derive(Debug, Serialize)]
pub struct ManagementMember {
    pub handle: i64,
    pub ordinal: i64,
    pub work: String,
}
#[derive(Debug, Serialize)]
pub struct ManagementCatalog {
    pub handle: i64,
    pub ordinal: i64,
    pub work: String,
    pub organization: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct ManagementWorkSummary {
    pub total: i64,
    pub grouped: i64,
    pub ungrouped: i64,
}
#[derive(Debug, Serialize)]
pub struct ManagementNode {
    pub handle: i64,
    pub parent: Option<i64>,
    pub ordinal: i64,
    pub field: Option<String>,
    pub kind: String,
    pub value: Option<String>,
    pub children: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ManagementScalar {
    String(String),
    Number(f64),
    Bool(bool),
    Null,
}
fn settings(c: &Connection) -> Result<(i64, i64, i64, i64), String> {
    c.query_row(
        "SELECT root,groups_node,catalog_node,orgs_node FROM management_settings",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .optional()
    .map_err(err)?
    .ok_or("Management settings unindexed; backfill first".into())
}
fn checked(c: &Connection, rev: i64) -> Result<(), String> {
    check_revision(c, rev)?;
    check_metadata_bounds(c)?;
    settings(c)?;
    Ok(())
}
fn ensure_rows(c: &Connection) -> Result<(), String> {
    let absent:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM reports r LEFT JOIN report_row_index i ON i.report_id=r.id WHERE i.report_id IS NULL)",[],|r|r.get(0)).map_err(err)?;
    if absent {
        Err("Management work discovery contains unindexed report history".into())
    } else {
        Ok(())
    }
}
fn copy_tables(c: &Connection) -> Result<(), String> {
    c.execute_batch("DELETE FROM management_settings;DELETE FROM management_nodes;DELETE FROM management_groups;DELETE FROM management_members;DELETE FROM management_catalog;DELETE FROM management_orgs;
INSERT INTO management_nodes SELECT * FROM management_stage.management_nodes;
INSERT INTO management_groups SELECT * FROM management_stage.management_groups;
INSERT INTO management_members SELECT * FROM management_stage.management_members;
INSERT INTO management_catalog SELECT * FROM management_stage.management_catalog;
INSERT INTO management_orgs SELECT * FROM management_stage.management_orgs;
INSERT INTO management_settings SELECT * FROM management_stage.management_settings;").map_err(err)
}
struct Stage {
    dir: PathBuf,
    lease: Option<fs::File>,
    quota: u64,
}
impl Stage {
    fn db(&self) -> PathBuf {
        self.dir.join("settings.sqlite3")
    }
    fn open(&self) -> Result<Connection, String> {
        let c = Connection::open(self.db()).map_err(err)?;
        c.execute_batch("PRAGMA journal_mode=DELETE;PRAGMA synchronous=FULL;PRAGMA temp_store=FILE;PRAGMA cache_size=-2048;PRAGMA temp.cache_size=-2048;").map_err(err)?;
        let pages = self.quota / 3 / 4096;
        c.pragma_update(None, "max_page_count", pages)
            .map_err(err)?;
        Ok(c)
    }
    fn cleanup(&self) -> Result<(), String> {
        fs::remove_dir_all(&self.dir).map_err(err)
    }
    fn check_quota(&self) -> Result<(), String> {
        let mut bytes = 0u64;
        for item in fs::read_dir(&self.dir).map_err(err)? {
            let m = item.map_err(err)?.metadata().map_err(err)?;
            bytes = bytes.checked_add(m.len()).ok_or("Session size overflow")?;
        }
        if bytes > self.quota {
            return Err("Management session disk quota exceeded".into());
        }
        Ok(())
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        // Windows cannot remove an open lease file, even after unlocking it.
        if let Some(lease) = self.lease.take() {
            let _ = FileExt::unlock(&lease);
            drop(lease);
        }
        let _ = self.cleanup();
    }
}
fn stage_root(store: &Storage) -> PathBuf {
    store.root.join("management-stages")
}
fn lifecycle_lock(store: &Storage) -> Result<fs::File, String> {
    let f = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(store.root.join("management-stages.lock"))
        .map_err(err)?;
    f.lock_exclusive().map_err(err)?;
    Ok(f)
}
fn owned_dir(path: &Path) -> bool {
    if !fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir())
        || !path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return false;
    }
    let Ok(file) = fs::File::open(path.join("owner")) else {
        return false;
    };
    let mut bytes = Vec::with_capacity(SESSION_MAGIC.len() + 1);
    if file
        .take((SESSION_MAGIC.len() + 1) as u64)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return false;
    }
    bytes == SESSION_MAGIC.as_bytes()
}
fn cleanup(store: &Storage) -> Result<(i64, i64), String> {
    let root = stage_root(store);
    if !root.exists() {
        return Ok((0, 0));
    }
    let mut removed = 0;
    let mut active = 0;
    let mut scanned = 0;
    for item in fs::read_dir(root).map_err(err)? {
        scanned += 1;
        if scanned > 1000 {
            return Err("Too many session directory entries".into());
        }
        let path = item.map_err(err)?.path();
        if !owned_dir(&path) {
            continue;
        }
        let lease = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path.join("lease"))
            .map_err(err)?;
        match lease.try_lock_exclusive() {
            Ok(()) => {
                // All cleanup/new-stage calls hold the lifecycle lock. Close the
                // lease handle before deletion; NTFS refuses an open lease file.
                FileExt::unlock(&lease).map_err(err)?;
                drop(lease);
                fs::remove_dir_all(path).map_err(err)?;
                removed += 1
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || (cfg!(windows) && e.raw_os_error() == Some(33)) =>
            {
                // Windows LockFileEx contention reports ERROR_LOCK_VIOLATION (33).
                active += 1;
            }
            Err(e) => return Err(err(e)),
        }
    }
    Ok((removed, active))
}
fn new_stage(store: &Storage, quota: u64) -> Result<Stage, String> {
    if !(4 * 1024 * 1024..=16 * 1024 * 1024 * 1024).contains(&quota) {
        return Err("Session quota must be4MiB..16GiB".into());
    }
    let _lock = lifecycle_lock(store)?;
    let (_, active) = cleanup(store)?;
    if active >= 4 {
        return Err("At most4live management sessions per storage".into());
    }
    let root = stage_root(store);
    fs::create_dir_all(&root).map_err(err)?;
    let c = Connection::open_in_memory().map_err(err)?;
    let id: String = c
        .query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))
        .map_err(err)?;
    let dir = root.join(id);
    fs::create_dir(&dir).map_err(err)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(err)?;
    }
    fs::write(dir.join("owner"), SESSION_MAGIC).map_err(err)?;
    let lease = fs::OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(dir.join("lease"))
        .map_err(err)?;
    lease.lock_exclusive().map_err(err)?;
    Ok(Stage { dir, lease: Some(lease), quota })
}
impl Storage {
    fn management_connect(&self) -> Result<Connection, String> {
        let mut c = self.open_connection()?;
        let marker: Option<i64> = c
            .query_row(
                "SELECT octet_length(json) FROM kv WHERE name='migration_v1'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        if marker.is_some_and(|n| n > 65536) {
            return Err("Unsupported oversized migration marker".into());
        }
        if marker.is_none() {
            if self.root.join("reports.json").exists()
                || self.root.join("desktop-config.json").exists()
            {
                return Err(
                    "Legacy files require an explicit migration before bounded management APIs"
                        .into(),
                );
            }
            let tx = c.transaction().map_err(err)?;
            tx.execute(
                "INSERT OR IGNORE INTO kv VALUES('migration_v1','\"complete\"')",
                [],
            )
            .map_err(err)?;
            tx.commit().map_err(err)?;
        }
        Ok(c)
    }
    pub fn management_revision(&self) -> Result<i64, String> {
        read_model::revision(&self.management_connect()?)
    }
    pub fn management_cleanup_sessions(&self) -> Result<(i64, i64), String> {
        let _lock = lifecycle_lock(self)?;
        cleanup(self)
    }
    pub fn management_index_settings(
        &self,
        rev: i64,
        job: &ManagementJob,
    ) -> Result<ManagementReceipt, String> {
        let stage = new_stage(self, 16 * 1024 * 1024 * 1024)?;
        let mut src = self.management_connect()?;
        let tx = src.transaction().map_err(err)?;
        check_revision(&tx, rev)?;
        let dest = stage.open()?;
        tree::schema(&dest)?;
        job.run(&dest,||{let st=dest.unchecked_transaction().map_err(err)?;let rowid:Option<i64>=tx.query_row("SELECT rowid FROM kv WHERE name='settings'",[],|r|r.get(0)).optional().map_err(err)?;let root=if let Some(rowid)=rowid{tree::parse(&st,tx.blob_open("main","kv","json",rowid,true).map_err(err)?,job)?}else{tree::parse(&st,std::io::Cursor::new("{\"orgNames\":[\"СМУ16\",\"АВТО\",\"ТРАК\",\"ЦЕНТР\"],\"groups\":[],\"catalog\":[]}".as_bytes()),job)?};tree::derive(&st,root,job)?;st.commit().map_err(err)})?;
        drop(dest);
        stage.check_quota()?;
        // End the read snapshot before acquiring the publication writer lock.
        // Unrelated sealed-filter writes can commit during staging; upgrading
        // that old snapshot would fail with SQLITE_BUSY_SNAPSHOT in WAL mode.
        tx.commit().map_err(err)?;
        job.check()?;
        src.execute(
            "ATTACH DATABASE ?1 AS management_stage",
            [stage.db().to_string_lossy().as_ref()],
        )
        .map_err(err)?;
        src.execute_batch("PRAGMA management_stage.cache_size=-2048;")
            .map_err(err)?;
        let tx=src.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(err)?;
        job.run(&tx, || {
            check_revision(&tx, rev)?;
            copy_tables(&tx)
        })?;
        job.finish(tx)?;
        drop(src);
        let cleanup_error = stage.cleanup().err();
        Ok(ManagementReceipt {
            revision: rev,
            deleted: 0,
            cleanup_error,
        })
    }
    pub fn management_bootstrap(&self, rev: i64) -> Result<ManagementBootstrap, String> {
        let mut c = self.management_connect()?;
        let tx = c.transaction().map_err(err)?;
        checked(&tx, rev)?;
        let mut stmt = tx
            .prepare("SELECT name FROM management_orgs ORDER BY ordinal LIMIT 101")
            .map_err(err)?;
        let org_names = stmt
            .query_map([], |r| r.get(0))
            .map_err(err)?
            .collect::<Result<Vec<String>, _>>()
            .map_err(err)?;
        if org_names.len() > 100 {
            return Err("Organization metadata exceeds bound".into());
        }
        Ok(ManagementBootstrap{revision:rev,org_names,groups:count(&tx,"management_groups")?,memberships:count(&tx,"management_members")?,catalog:count(&tx,"management_catalog")?,reports:count(&tx,"reports")?,indexed_reports:tx.query_row("SELECT COUNT(*) FROM reports r JOIN management_report_meta m ON m.report_id=r.id JOIN report_row_index i ON i.report_id=r.id",[],|r|r.get(0)).map_err(err)?})
    }
    pub fn management_group_page(
        &self,
        rev: i64,
        query: &str,
        after: Option<i64>,
        size: usize,
    ) -> Result<Vec<ManagementGroup>, String> {
        batch(size)?;
        search(query)?;
        let mut c = self.management_connect()?;
        let tx = c.transaction().map_err(err)?;
        checked(&tx, rev)?;
        let mut stmt=tx.prepare("SELECT handle,ordinal,id,name,members,unique_members FROM management_groups WHERE instr(name,?1)>0 AND (?2 IS NULL OR handle>?2) ORDER BY handle LIMIT ?3").map_err(err)?;
        let out = stmt
            .query_map(params![query, after, size as i64], |r| {
                Ok(ManagementGroup {
                    handle: r.get(0)?,
                    ordinal: r.get(1)?,
                    id: r.get(2)?,
                    name: r.get(3)?,
                    members: r.get(4)?,
                    unique_members: r.get(5)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(out)
    }
    pub fn management_member_page(
        &self,
        rev: i64,
        group: i64,
        after: Option<i64>,
        size: usize,
    ) -> Result<Vec<ManagementMember>, String> {
        batch(size)?;
        let mut c = self.management_connect()?;
        let tx = c.transaction().map_err(err)?;
        checked(&tx, rev)?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM management_groups WHERE handle=?1)",
                [group],
                |r| r.get(0),
            )
            .map_err(err)?;
        if !exists {
            return Err("Unknown group handle".into());
        }
        let mut stmt=tx.prepare("SELECT handle,ordinal,work FROM management_members WHERE group_handle=?1 AND (?2 IS NULL OR handle>?2) ORDER BY handle LIMIT ?3").map_err(err)?;
        let out = stmt
            .query_map(params![group, after, size as i64], |r| {
                Ok(ManagementMember {
                    handle: r.get(0)?,
                    ordinal: r.get(1)?,
                    work: r.get(2)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(out)
    }
    pub fn management_catalog_page(
        &self,
        rev: i64,
        query: &str,
        org: Option<&str>,
        after: Option<i64>,
        size: usize,
    ) -> Result<Vec<ManagementCatalog>, String> {
        batch(size)?;
        search(query)?;
        if let Some(o) = org {
            label(o)?;
        }
        let mut c = self.management_connect()?;
        let tx = c.transaction().map_err(err)?;
        checked(&tx, rev)?;
        let mut stmt=tx.prepare("SELECT handle,ordinal,work,organization FROM management_catalog WHERE instr(work,?1)>0 AND (?2 IS NULL OR organization=?2) AND (?3 IS NULL OR handle>?3) ORDER BY handle LIMIT ?4").map_err(err)?;
        let out = stmt
            .query_map(params![query, org, after, size as i64], |r| {
                Ok(ManagementCatalog {
                    handle: r.get(0)?,
                    ordinal: r.get(1)?,
                    work: r.get(2)?,
                    organization: r.get(3)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(out)
    }
    pub fn management_node_page(
        &self,
        rev: i64,
        parent: Option<i64>,
        after: Option<(i64, i64)>,
        size: usize,
    ) -> Result<Vec<ManagementNode>, String> {
        batch(size)?;
        let mut c = self.management_connect()?;
        let tx = c.transaction().map_err(err)?;
        checked(&tx, rev)?;
        let (ord, id) = after.unwrap_or((-1, -1));
        let mut stmt=tx.prepare("SELECT n.handle,n.parent,n.ordinal,n.field,n.kind,n.value,(SELECT COUNT(*) FROM management_nodes x WHERE x.parent=n.handle) FROM management_nodes n WHERE n.parent IS ?1 AND (n.ordinal,n.handle)>(?2,?3) ORDER BY n.ordinal,n.handle LIMIT ?4").map_err(err)?;
        let out = stmt
            .query_map(params![parent, ord, id, size as i64], |r| {
                Ok(ManagementNode {
                    handle: r.get(0)?,
                    parent: r.get(1)?,
                    ordinal: r.get(2)?,
                    field: r.get(3)?,
                    kind: r.get(4)?,
                    value: r.get(5)?,
                    children: r.get(6)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(out)
    }
}
const WORKS:&str="WITH works(work) AS (SELECT work FROM report_rows WHERE (?1 IS NULL OR organization=?1) UNION SELECT work FROM management_catalog WHERE (?1 IS NULL OR organization=?1) UNION SELECT work FROM management_members WHERE ?1 IS NULL)";
impl Storage {
    pub fn management_work_summary(
        &self,
        rev: i64,
        query: &str,
        org: Option<&str>,
    ) -> Result<ManagementWorkSummary, String> {
        search(query)?;
        if let Some(o) = org {
            label(o)?;
        }
        let mut c = self.management_connect()?;
        let tx = c.transaction().map_err(err)?;
        checked(&tx, rev)?;
        ensure_rows(&tx)?;
        let(total,grouped):(i64,i64)=tx.query_row(&format!("{WORKS} SELECT COUNT(*),COALESCE(SUM(EXISTS(SELECT 1 FROM management_members m WHERE m.work=works.work)),0) FROM works WHERE instr(work,?2)>0"),params![org,query],|r|Ok((r.get(0)?,r.get(1)?))).map_err(err)?;
        Ok(ManagementWorkSummary {
            total,
            grouped,
            ungrouped: total - grouped,
        })
    }
    pub fn management_work_page(
        &self,
        rev: i64,
        query: &str,
        org: Option<&str>,
        after: Option<&str>,
        size: usize,
    ) -> Result<Vec<String>, String> {
        batch(size)?;
        search(query)?;
        if let Some(o) = org {
            label(o)?;
        }
        if let Some(a) = after {
            label(a)?;
        }
        let mut c = self.management_connect()?;
        let tx = c.transaction().map_err(err)?;
        checked(&tx, rev)?;
        ensure_rows(&tx)?;
        let mut stmt=tx.prepare(&format!("{WORKS} SELECT work FROM works WHERE instr(work,?2)>0 AND (?3 IS NULL OR work>?3) ORDER BY work LIMIT ?4")).map_err(err)?;
        let out = stmt
            .query_map(params![org, query, after, size as i64], |r| r.get(0))
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(out)
    }
}
/// Owns a private disk snapshot. Drop/abort removes only its owned stage.
pub struct SettingsEditor {
    stage: Stage,
    expected: i64,
    database: PathBuf,
    job: Arc<ManagementJob>,
}
fn add(
    c: &Connection,
    parent: i64,
    field: Option<&str>,
    kind: &str,
    value: Option<&str>,
) -> Result<i64, String> {
    let ordinal: i64 = c
        .query_row(
            "SELECT COALESCE(MAX(ordinal)+1,0) FROM management_nodes WHERE parent=?1",
            [parent],
            |r| r.get(0),
        )
        .map_err(err)?;
    c.execute(
        "INSERT INTO management_nodes(parent,ordinal,field,kind,value) VALUES(?1,?2,?3,?4,?5)",
        params![parent, ordinal, field, kind, value],
    )
    .map_err(err)?;
    Ok(c.last_insert_rowid())
}
fn scalar(c: &Connection, parent: i64, field: &str, value: ManagementScalar) -> Result<(), String> {
    label(field)?;
    let (kind, value) = match value {
        ManagementScalar::String(s) => {
            label(&s)?;
            ("string", Some(s))
        }
        ManagementScalar::Number(n) => {
            num(n)?;
            ("number", Some(serde_json::to_string(&n).map_err(err)?))
        }
        ManagementScalar::Bool(b) => ("bool", Some(b.to_string())),
        ManagementScalar::Null => ("null", None),
    };
    if let Some((id, old, _)) = tree::child(c, parent, field)? {
        if old == "object" || old == "array" {
            return Err("Scalar setter cannot replace nested containers".into());
        }
        c.execute(
            "UPDATE management_nodes SET kind=?1,value=?2 WHERE handle=?3",
            params![kind, value, id],
        )
        .map_err(err)?;
    } else {
        add(c, parent, Some(field), kind, value.as_deref())?;
    }
    Ok(())
}
fn remove_node(c: &Connection, id: i64) -> Result<(), String> {
    c.execute("WITH RECURSIVE descendants(handle) AS (SELECT ?1 UNION ALL SELECT n.handle FROM management_nodes n JOIN descendants d ON n.parent=d.handle) DELETE FROM management_nodes WHERE handle IN descendants",[id]).map_err(err)?;
    Ok(())
}
fn belongs(c: &Connection, node: i64, parent: i64, kind: &str) -> Result<(), String> {
    let ok:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM management_nodes WHERE handle=?1 AND parent=?2 AND kind=?3)",params![node,parent,kind],|r|r.get(0)).map_err(err)?;
    if ok {
        Ok(())
    } else {
        Err("Unknown or wrong-domain settings handle".into())
    }
}
fn works_node(c: &Connection, group: i64) -> Result<i64, String> {
    belongs(c, group, settings(c)?.1, "object")?;
    let (id, kind, _) = tree::child(c, group, "works")?.ok_or("Missing works")?;
    if kind != "array" {
        return Err("Invalid group works".into());
    }
    Ok(id)
}
impl SettingsEditor {
    fn mutate<T>(
        &mut self,
        action: impl FnOnce(&Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let c = self.stage.open()?;
        self.job.run(&c, || {
            let tx = c.unchecked_transaction().map_err(err)?;
            let result = action(&tx)?;
            self.job.check()?;
            self.stage.check_quota()?;
            tx.commit().map_err(err)?;
            Ok(result)
        })
    }
    pub fn cancel(&self) -> bool {
        self.job.cancel()
    }
    pub fn set_scalar(&mut self, name: &str, value: ManagementScalar) -> Result<(), String> {
        if matches!(name, "orgNames" | "groups" | "catalog") {
            return Err(
                "Organization rename and container replacement require dedicated domain operations"
                    .into(),
            );
        }
        self.mutate(|c| scalar(c, settings(c)?.0, name, value))
    }
    pub fn create_group(&mut self, id: &str, name: &str) -> Result<i64, String> {
        label(id)?;
        label(name)?;
        self.mutate(|c| {
            let groups = settings(c)?.1;
            let group = add(c, groups, None, "object", None)?;
            scalar(c, group, "id", ManagementScalar::String(id.into()))?;
            scalar(c, group, "name", ManagementScalar::String(name.into()))?;
            add(c, group, Some("works"), "array", None)?;
            Ok(group)
        })
    }
    pub fn rename_group(&mut self, group: i64, name: &str) -> Result<(), String> {
        label(name)?;
        self.mutate(|c| {
            belongs(c, group, settings(c)?.1, "object")?;
            scalar(c, group, "name", ManagementScalar::String(name.into()))
        })
    }
    pub fn delete_group(&mut self, group: i64) -> Result<(), String> {
        self.mutate(|c| {
            belongs(c, group, settings(c)?.1, "object")?;
            remove_node(c, group)
        })
    }
    pub fn append_members(&mut self, group: i64, works: &[String]) -> Result<(), String> {
        batch(works.len())?;
        for w in works {
            label(w)?;
        }
        self.mutate(|c| {
            let parent = works_node(c, group)?;
            for w in works {
                add(c, parent, None, "string", Some(w))?;
            }
            Ok(())
        })
    }
    pub fn remove_members(&mut self, group: i64, handles: &[i64]) -> Result<(), String> {
        batch(handles.len())?;
        self.mutate(|c| {
            let parent = works_node(c, group)?;
            for id in handles {
                belongs(c, *id, parent, "string")?;
                c.execute("DELETE FROM management_nodes WHERE handle=?1", [id])
                    .map_err(err)?;
            }
            Ok(())
        })
    }
    pub fn member_page(
        &self,
        group: i64,
        after: Option<i64>,
        size: usize,
    ) -> Result<Vec<ManagementMember>, String> {
        batch(size)?;
        self.job.check()?;
        let c = self.stage.open()?;
        let parent = works_node(&c, group)?;
        let mut stmt=c.prepare("SELECT handle,ordinal,value FROM management_nodes WHERE parent=?1 AND (?2 IS NULL OR handle>?2) ORDER BY handle LIMIT ?3").map_err(err)?;
        let out = stmt
            .query_map(params![parent, after, size as i64], |r| {
                Ok(ManagementMember {
                    handle: r.get(0)?,
                    ordinal: r.get(1)?,
                    work: r.get(2)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(out)
    }
    pub fn create_catalog(&mut self, work: &str, org: Option<&str>) -> Result<i64, String> {
        label(work)?;
        if let Some(o) = org {
            label(o)?;
        }
        self.mutate(|c| {
            let parent = settings(c)?.2;
            let item = add(c, parent, None, "object", None)?;
            scalar(c, item, "work", ManagementScalar::String(work.into()))?;
            if let Some(o) = org {
                scalar(c, item, "organization", ManagementScalar::String(o.into()))?;
            }
            Ok(item)
        })
    }
    pub fn update_catalog(
        &mut self,
        item: i64,
        work: &str,
        org: Option<&str>,
    ) -> Result<(), String> {
        label(work)?;
        if let Some(o) = org {
            label(o)?;
        }
        self.mutate(|c| {
            belongs(c, item, settings(c)?.2, "object")?;
            scalar(c, item, "work", ManagementScalar::String(work.into()))?;
            match (org, tree::child(c, item, "organization")?) {
                (Some(o), _) => {
                    scalar(c, item, "organization", ManagementScalar::String(o.into()))?
                }
                (None, Some((id, _, _))) => remove_node(c, id)?,
                _ => {}
            }
            Ok(())
        })
    }
    pub fn delete_catalog(&mut self, handles: &[i64]) -> Result<(), String> {
        batch(handles.len())?;
        self.mutate(|c| {
            let parent = settings(c)?.2;
            for h in handles {
                belongs(c, *h, parent, "object")?;
                remove_node(c, *h)?;
            }
            Ok(())
        })
    }
}
struct DocumentWriter {
    inner: BufWriter<fs::File>,
    bytes: u64,
    limit: u64,
    hash: Sha256,
}
impl Write for DocumentWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if self.bytes + b.len() as u64 > self.limit {
            return Err(std::io::Error::other(
                "Settings document exceeds session quota or2GiBblob limit",
            ));
        }
        let n = self.inner.write(b)?;
        self.bytes += n as u64;
        self.hash.update(&b[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
impl Storage {
    pub fn management_begin_settings(
        &self,
        expected: i64,
        quota: u64,
        job: Arc<ManagementJob>,
    ) -> Result<SettingsEditor, String> {
        job.check()?;
        let stage = new_stage(self, quota)?;
        let mut source = self.management_connect()?;
        let tx = source.transaction().map_err(err)?;
        checked(&tx, expected)?;
        let dest = stage.open()?;
        tree::schema(&dest)?;
        job.run(&dest,||{let st=dest.unchecked_transaction().map_err(err)?;let mut stmt=tx.prepare("SELECT handle,parent,ordinal,field,kind,value FROM management_nodes ORDER BY handle").map_err(err)?;let mut rows=stmt.query([]).map_err(err)?;let mut ins=st.prepare("INSERT INTO management_nodes VALUES(?1,?2,?3,?4,?5,?6)").map_err(err)?;while let Some(r)=rows.next().map_err(err)?{job.check()?;ins.execute(params![r.get::<_,i64>(0).map_err(err)?,r.get::<_,Option<i64>>(1).map_err(err)?,r.get::<_,i64>(2).map_err(err)?,r.get::<_,Option<String>>(3).map_err(err)?,r.get::<_,String>(4).map_err(err)?,r.get::<_,Option<String>>(5).map_err(err)?]).map_err(err)?;}drop(ins);tree::derive(&st,settings(&tx)?.0,&job)?;st.commit().map_err(err)})?;
        drop(dest);
        stage.check_quota()?;
        Ok(SettingsEditor {
            stage,
            expected,
            database: fs::canonicalize(self.db_path()).map_err(err)?,
            job,
        })
    }
    pub fn management_commit_settings(
        &self,
        editor: SettingsEditor,
    ) -> Result<ManagementReceipt, String> {
        self.management_commit_settings_with_limit(editor, None)
    }
    pub(super) fn management_commit_settings_with_limit(
        &self,
        editor: SettingsEditor,
        page_limit: Option<i64>,
    ) -> Result<ManagementReceipt, String> {
        if fs::canonicalize(self.db_path()).map_err(err)? != editor.database {
            return Err("Settings session belongs to another database".into());
        }
        editor.job.check()?;
        let staged = editor.stage.open()?;
        let root = settings(&staged)?.0;
        editor.job.run(&staged, || {
            let tx = staged.unchecked_transaction().map_err(err)?;
            tree::derive(&tx, root, &editor.job)?;
            tx.commit().map_err(err)
        })?;
        let document = editor.stage.dir.join("settings.json");
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&document)
            .map_err(err)?;
        let mut writer = DocumentWriter {
            inner: BufWriter::with_capacity(65536, file),
            bytes: 0,
            limit: (editor.stage.quota / 3).min(i32::MAX as u64),
            hash: Sha256::new(),
        };
        tree::write(&staged, root, &mut writer, &editor.job, 0)?;
        writer.flush().map_err(err)?;
        writer.inner.get_ref().sync_all().map_err(err)?;
        let bytes = writer.bytes;
        let digest = writer.hash.clone().finalize();
        drop(writer);
        drop(staged);
        editor.stage.check_quota()?;
        let c = self.management_connect()?;
        if let Some(limit) = page_limit {
            c.pragma_update(None, "max_page_count", limit)
                .map_err(err)?;
        }
        c.execute(
            "ATTACH DATABASE ?1 AS management_stage",
            [editor.stage.db().to_string_lossy().as_ref()],
        )
        .map_err(err)?;
        c.execute_batch("PRAGMA management_stage.cache_size=-2048;")
            .map_err(err)?;
        editor.job.check()?;
        let revision = editor.job.run(&c, || {
            let tx = c.unchecked_transaction().map_err(err)?;
            check_revision(&tx, editor.expected)?;
            tx.execute("DELETE FROM kv WHERE name='settings'", [])
                .map_err(err)?;
            tx.execute(
                "INSERT INTO kv(name,json) VALUES('settings',zeroblob(?1))",
                [bytes],
            )
            .map_err(err)?;
            let rowid: i64 = tx
                .query_row("SELECT rowid FROM kv WHERE name='settings'", [], |r| {
                    r.get(0)
                })
                .map_err(err)?;
            {
                let mut blob = tx
                    .blob_open("main", "kv", "json", rowid, false)
                    .map_err(err)?;
                let mut input =
                    BufReader::with_capacity(65536, fs::File::open(&document).map_err(err)?);
                let mut buf = [0u8; 65536];
                let mut hash = Sha256::new();
                let mut copied = 0u64;
                loop {
                    editor.job.check()?;
                    let n = input.read(&mut buf).map_err(err)?;
                    if n == 0 {
                        break;
                    }
                    copied += n as u64;
                    if copied > bytes {
                        return Err("Settings document grew during publication".into());
                    }
                    blob.write_all(&buf[..n]).map_err(err)?;
                    hash.update(&buf[..n]);
                }
                blob.close().map_err(err)?;
                if copied != bytes || hash.finalize() != digest {
                    return Err("Settings document changed during publication".into());
                }
            }
            copy_tables(&tx)?;
            editor.job.check()?;
            let rev = read_model::revision(&tx)?;
            editor.job.finish(tx)?;
            Ok(rev)
        })?;
        drop(c);
        let cleanup_error = editor.stage.cleanup().err();
        Ok(ManagementReceipt {
            revision,
            deleted: 0,
            cleanup_error,
        })
    }
    pub fn management_delete_report_with_backup(
        &self,
        expected: i64,
        id: &str,
        destination: &Path,
        job: &ManagementJob,
    ) -> Result<ManagementReceipt, String> {
        label(id)?;
        job.check()?;
        check_revision(&self.management_connect()?, expected)?;
        let backed_revision = self.backup_sqlite_verified(destination)?;
        if backed_revision != expected {
            return Err("Stale storage revision; refresh the selection.".into());
        }

        // The backup precedes indexing and retains the original report.
        // The final transaction rechecks the revision and rejects linked sources.
        job.check()?;
        self.index_report(id, expected)?;
        job.check()?;
        self.management_index_report(id, expected, &ManagementJob::new())?;
        self.management_delete_reports(expected, &[id.to_owned()], job)
    }
    pub fn management_delete_reports(
        &self,
        expected: i64,
        ids: &[String],
        job: &ManagementJob,
    ) -> Result<ManagementReceipt, String> {
        batch(ids.len())?;
        for id in ids {
            label(id)?;
        }
        self.management_remove(expected, Some(ids), job)
    }
    pub fn management_clear_reports(
        &self,
        expected: i64,
        job: &ManagementJob,
    ) -> Result<ManagementReceipt, String> {
        self.management_remove(expected, None, job)
    }
    fn management_remove(
        &self,
        expected: i64,
        ids: Option<&[String]>,
        job: &ManagementJob,
    ) -> Result<ManagementReceipt, String> {
        let c = self.management_connect()?;
        job.run(&c,||{let tx=c.unchecked_transaction().map_err(err)?;check_revision(&tx,expected)?;check_metadata_bounds(&tx)?;
        tx.execute_batch("CREATE TEMP TABLE management_delete_ids(id TEXT PRIMARY KEY) WITHOUT ROWID;").map_err(err)?;
        if let Some(ids)=ids {for id in ids {tx.execute("INSERT OR IGNORE INTO temp.management_delete_ids VALUES(?1)",[id]).map_err(err)?;}
            let missing:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM temp.management_delete_ids d LEFT JOIN reports r ON r.id=d.id WHERE r.id IS NULL)",[],|r|r.get(0)).map_err(err)?;if missing{return Err("Report delete contains an unknown identifier".into())}}
        // C3 daily-wide documents have a source-wide diagnostic anchor shared
        // across report days. A partial deletion via management CRUD cannot
        // preserve that lifecycle. Check the durable link table independently
        // of the optional management metadata index, including zero-row days.
        let linked: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM report_source_diagnostic_links l
              WHERE ?1 OR l.report_id IN (SELECT id FROM temp.management_delete_ids))",
            [ids.is_none()],
            |r| r.get(0),
        ).map_err(err)?;
        if linked {
            return Err("Shared source diagnostics require source-aware lifecycle before report deletion".into());
        }
        let unindexed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM reports r LEFT JOIN management_report_meta m ON m.report_id=r.id WHERE (?1 OR r.id IN(SELECT id FROM temp.management_delete_ids)) AND m.report_id IS NULL)",[ids.is_none()],|r|r.get(0)).map_err(err)?;
        if unindexed{return Err("Report deletion contains unindexed metadata; backfill before removing reports".into())}
        let shared:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM reports r JOIN management_report_meta m ON m.report_id=r.id WHERE (?1 OR r.id IN(SELECT id FROM temp.management_delete_ids)) AND m.shared_source_artifact<>0)",[ids.is_none()],|r|r.get(0)).map_err(err)?;
        if shared{return Err("Shared source diagnostics require anchor lifecycle support before deletion".into())}
        let deleted=if ids.is_none(){tx.execute("DELETE FROM reports",[])}else{tx.execute("DELETE FROM reports WHERE id IN(SELECT id FROM temp.management_delete_ids)",[])}.map_err(err)?;
        job.check()?;let revision=read_model::revision(&tx)?;job.finish(tx)?;Ok(ManagementReceipt{revision,deleted:deleted as i64,cleanup_error:None})})
    }
}
#[path = "management_reports.rs"]
mod reports;
pub use reports::{
    ManagementPeriod, ManagementReport, ManagementReportCursor, ManagementReportFilter,
    ManagementReportSummary,
};
impl Storage {
    pub fn management_group_count(&self, rev: i64, query: &str) -> Result<i64, String> {
        search(query)?;
        let mut c = self.management_connect()?;
        let tx = c.transaction().map_err(err)?;
        checked(&tx, rev)?;
        tx.query_row(
            "SELECT COUNT(*) FROM management_groups WHERE instr(name,?1)>0",
            [query],
            |r| r.get(0),
        )
        .map_err(err)
    }
    pub fn management_catalog_count(
        &self,
        rev: i64,
        query: &str,
        org: Option<&str>,
    ) -> Result<i64, String> {
        search(query)?;
        if let Some(o) = org {
            label(o)?;
        }
        let mut c = self.management_connect()?;
        let tx = c.transaction().map_err(err)?;
        checked(&tx, rev)?;
        tx.query_row("SELECT COUNT(*) FROM management_catalog WHERE instr(work,?1)>0 AND (?2 IS NULL OR organization=?2)",params![query,org],|r|r.get(0)).map_err(err)
    }
}

pub(super) struct JobRead<'a, R> {
    pub inner: R,
    pub job: &'a ManagementJob,
}
impl<R: Read> Read for JobRead<'_, R> {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.job.check().map_err(std::io::Error::other)?;
        self.inner.read(b)
    }
}
