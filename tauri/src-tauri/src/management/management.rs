//! Bounded settings domain; deliberately not registered as Tauri commands.
use super::management_tree::{self as tree, label};
use super::read_model::{check_metadata_bounds, check_revision};
use super::*;
use fs2::FileExt;
use rusqlite::OptionalExtension;
use std::{
    io::{BufReader, BufWriter, Read, Write},
    sync::{atomic::AtomicBool, Arc},
};
const SESSION_MAGIC: &str = "analiz-management-v1\n";
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
pub(super) fn allow_missing_amount(c: &Connection) -> Result<(), String> {
    c.execute_batch("
        DROP TRIGGER management_report_insert;
        DROP TRIGGER management_report_update;
        DROP TRIGGER management_report_delete;
        CREATE TABLE management_report_meta_v2 (
          report_id TEXT PRIMARY KEY, amount REAL, saved_amount REAL,
          warnings INTEGER NOT NULL, shared_source_artifact INTEGER NOT NULL);
        INSERT INTO management_report_meta_v2 SELECT report_id,amount,saved_amount,warnings,shared_source_artifact FROM management_report_meta;
        DROP TABLE management_report_meta;
        ALTER TABLE management_report_meta_v2 RENAME TO management_report_meta;
    ").map_err(err)
}

pub(super) fn initialize(c: &Connection) -> Result<(), String> {
    tree::schema(c)?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS management_report_meta(report_id TEXT PRIMARY KEY,amount REAL,saved_amount REAL,warnings INTEGER NOT NULL,shared_source_artifact INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS management_report_orgs(report_id TEXT,ordinal INTEGER,org TEXT,PRIMARY KEY(report_id,ordinal)) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS management_report_org_lookup ON management_report_orgs(org,report_id);
CREATE TRIGGER IF NOT EXISTS management_settings_insert AFTER INSERT ON kv WHEN NEW.name='settings' BEGIN DELETE FROM management_settings;END;
CREATE TRIGGER IF NOT EXISTS management_settings_update AFTER UPDATE ON kv WHEN OLD.name='settings' OR NEW.name='settings' BEGIN DELETE FROM management_settings;END;
CREATE TRIGGER IF NOT EXISTS management_settings_delete AFTER DELETE ON kv WHEN OLD.name='settings' BEGIN DELETE FROM management_settings;END;
CREATE TRIGGER IF NOT EXISTS management_report_insert AFTER INSERT ON reports BEGIN DELETE FROM management_report_meta WHERE report_id=NEW.id;DELETE FROM management_report_orgs WHERE report_id=NEW.id;END;
CREATE TRIGGER IF NOT EXISTS management_report_update AFTER UPDATE ON reports BEGIN DELETE FROM management_report_meta WHERE report_id IN(OLD.id,NEW.id);DELETE FROM management_report_orgs WHERE report_id IN(OLD.id,NEW.id);END;
CREATE TRIGGER IF NOT EXISTS management_report_delete AFTER DELETE ON reports BEGIN DELETE FROM management_report_meta WHERE report_id=OLD.id;DELETE FROM management_report_orgs WHERE report_id=OLD.id;END;").map_err(err)
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
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub work_group_hierarchy: bool,
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
pub struct UnifiedManagementRow {
    pub handle: Option<String>,
    pub id: Option<String>,
    pub name: String,
    pub organization: Option<String>,
    pub members: Option<i64>,
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<i64>,
}
#[derive(Debug, Serialize)]
pub struct UnifiedManagementPage {
    pub rows: Vec<UnifiedManagementRow>,
    pub total: i64,
    pub grouped: i64,
    pub ungrouped: i64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hierarchy: bool,
}

fn unified_normalized(input: &str) -> String {
    input.to_lowercase().trim_end_matches(['.', ',', ';', ':']).split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Storage {
    /// Only the requested metadata page crosses IPC. Search and ordering stay in SQLite.
    pub fn unified_management_page(&self, revision: i64, kind: &str, query: &str, group: Option<i64>, offset: u32, limit: usize) -> Result<UnifiedManagementPage, String> {
        if !(1..=50).contains(&limit) { return Err("Размер страницы должен быть от 1 до 50.".into()); }
        search(query)?;
        let mut c = self.management_connect()?;
        c.create_scalar_function("unified_normalized", 1, rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC | rusqlite::functions::FunctionFlags::SQLITE_UTF8, |ctx| {
            Ok(unified_normalized(&ctx.get::<String>(0)?))
        }).map_err(err)?;
        let locale: icu_provider::DataLocale = "ru".parse().map_err(err)?;
        let collator = icu_collator::Collator::try_new(&locale, Default::default()).map_err(err)?;
        c.create_collation("unified_ru", move |a,b| collator.compare(a,b)).map_err(err)?;
        let tx = settings_snapshot(&mut c, revision)?;
        let term = unified_normalized(query);
        let link_root = if kind == "group_tree" { unified_settings_io::group_parents_node(&tx)? } else { None };
        let table = match kind {
            "groups" => "SELECT CAST(handle AS TEXT) handle,id,name,NULL organization,members,NULL group_handle FROM management_groups".to_string(),
            "group_tree" => {
                let link_root = link_root.map_or_else(|| "NULL".to_owned(), |handle| handle.to_string());
                let condition = if let Some(parent) = group {
                    if !tx.query_row("SELECT EXISTS(SELECT 1 FROM management_groups WHERE handle=?1)",
                        [parent], |row| row.get::<_, bool>(0)).map_err(err)?
                    {
                        return Err("Родительская группа не найдена.".into());
                    }
                    format!("WHERE l.parent_handle={parent}")
                } else if term.is_empty() {
                    "WHERE l.parent_handle IS NULL".into()
                } else {
                    String::new()
                };
                format!(
                    "WITH links AS (
                         SELECT a.handle child_handle,b.handle parent_handle
                         FROM management_nodes item
                         JOIN management_nodes child ON child.parent=item.handle AND child.field='child'
                         JOIN management_nodes parent ON parent.parent=item.handle AND parent.field='parent'
                         JOIN management_groups a ON a.id=child.value
                         JOIN management_groups b ON b.id=parent.value
                         WHERE item.parent={link_root}
                     )
                     SELECT CAST(g.handle AS TEXT) handle,g.id,g.name,NULL organization,g.members,
                            CAST(l.parent_handle AS TEXT) group_handle,
                            (SELECT COUNT(*) FROM links c WHERE c.parent_handle=g.handle) children
                     FROM management_groups g LEFT JOIN links l ON l.child_handle=g.handle {condition}"
                )
            }
            "members" => {
                let handle = group.filter(|h| *h > 0).ok_or("Выберите группу.")?;
                if !tx.query_row("SELECT EXISTS(SELECT 1 FROM management_groups WHERE handle=?1)", [handle], |r| r.get::<_, bool>(0)).map_err(err)? { return Err("Группа не найдена.".into()); }
                format!("SELECT CAST(handle AS TEXT) handle,NULL id,work name,NULL organization,NULL members,CAST(group_handle AS TEXT) group_handle FROM management_members WHERE group_handle={handle}")
            },
            "catalog" => "SELECT CAST(handle AS TEXT) handle,NULL id,work name,organization,NULL members,NULL group_handle FROM management_catalog".to_string(),
            "works" => {
                ensure_rows(&tx)?;
                "SELECT NULL handle,NULL id,work name,NULL organization,NULL members,(SELECT CAST(MIN(group_handle) AS TEXT) FROM management_members m WHERE m.work=w.work) group_handle FROM (SELECT work FROM report_rows UNION SELECT work FROM management_catalog UNION SELECT work FROM management_members) w".to_string()
            },
            _ => return Err("Неизвестная страница настроек.".into()),
        };
        let children = if kind == "group_tree" { "children" } else { "NULL" };
        let (filter, prefix_order) = if term.is_empty() {
            ("", "")
        } else {
            ("WHERE instr(unified_normalized(name),?1)>0",
             "CASE WHEN substr(unified_normalized(name),1,length(?1))=?1 THEN 0 ELSE 1 END,")
        };
        let grouped_count = if kind == "works" { "COUNT(group_handle)" } else { "0" };
        let counts_sql = format!("SELECT COUNT(*),{grouped_count} FROM ({table}) {filter}");
        let search_param = (!term.is_empty()).then_some(&term);
        let (total, grouped): (i64, i64) = tx.query_row(
            &counts_sql, rusqlite::params_from_iter(search_param), |row| Ok((row.get(0)?, row.get(1)?))
        ).map_err(err)?;
        let mut stmt = tx.prepare(&format!("SELECT handle,id,name,organization,members,group_handle,{children} FROM ({table}) {filter} ORDER BY {prefix_order}name COLLATE unified_ru,name,handle LIMIT ?2 OFFSET ?3")).map_err(err)?;
        let rows = stmt.query_map(params![term,limit as i64,offset], |r| Ok(UnifiedManagementRow { handle:r.get(0)?, id:r.get(1)?, name:r.get(2)?, organization:r.get(3)?, members:r.get(4)?, group:r.get(5)?, children:r.get(6)? })).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;
        Ok(UnifiedManagementPage { rows,total,grouped,ungrouped:total-grouped,hierarchy:link_root.is_some() })
    }

    pub fn unified_save_group(&self, revision:i64, handle:Option<i64>, id:&str, name:&str, works:&[String]) -> Result<ManagementReceipt,String> {
        if name.trim().is_empty() || name.len()>1024 || id.len()>128 || works.len()>5000 { return Err("Введите название и выберите не более 5000 работ.".into()); }
        for work in works { if work.trim().is_empty() || work.len()>4096 { return Err("Некорректное название работы.".into()); } }
        let mut editor = self.management_begin_settings(revision,256*1024*1024,Arc::new(ManagementJob::new()))?;
        editor.unified_save_group(handle,id,name,works)?;
        self.management_commit_settings(editor)
    }

    pub fn unified_delete_groups(
        &self,
        revision: i64,
        handles: Option<&[i64]>,
        confirmed_count: usize,
        backup: Option<&Path>,
        confirmed: bool,
    ) -> Result<ManagementReceipt, String> {
        if !confirmed || confirmed_count == 0 {
            return Err("Подтвердите удаление групп.".into());
        }
        if let Some(handles) = handles {
            if handles.len() != confirmed_count || handles.len() > 10_000
                || handles.iter().collect::<std::collections::HashSet<_>>().len() != handles.len()
            {
                return Err("Некорректный список удаляемых групп.".into());
            }
        }
        let mut editor = self.management_begin_settings(
            revision, 256 * 1024 * 1024, Arc::new(ManagementJob::new()),
        )?;
        let job = editor.job.clone();
        let clear = editor.mutate(|c| {
            let parent = settings(c)?.1;
            let total: usize = c.query_row(
                "SELECT COUNT(*) FROM management_nodes WHERE parent=?1 AND kind='object'",
                [parent], |row| row.get(0),
            ).map_err(err)?;
            let clear = confirmed_count == total;
            if confirmed_count > total || (handles.is_none() && !clear) {
                return Err("Список групп изменился. Повторите действие.".into());
            }
            if clear && backup.is_none() {
                return Err("Для очистки списка нужна проверенная резервная копия SQLite.".into());
            }
            unified_settings_io::update_links_before_bulk_deletion(c, parent, handles, clear, &job)?;
            if let Some(handles) = handles {
                for &handle in handles {
                    belongs(c, handle, parent, "object")?;
                    remove_node(c, handle)?;
                }
            } else {
                c.execute(
                    "WITH RECURSIVE descendants(handle) AS (
                        SELECT handle FROM management_nodes WHERE parent=?1 AND kind='object'
                        UNION ALL SELECT n.handle FROM management_nodes n JOIN descendants d ON n.parent=d.handle
                     ) DELETE FROM management_nodes WHERE handle IN descendants",
                    [parent],
                ).map_err(err)?;
            }
            Ok(clear)
        })?;
        if clear {
            let destination = backup.ok_or("Укажите путь для резервной копии.")?;
            if self.backup_sqlite_verified(destination)? != revision {
                return Err("База изменилась при создании копии. Повторите действие.".into());
            }
        }
        self.management_commit_settings(editor)
    }

    pub fn unified_assign_work(&self, revision:i64, work:&str, group:Option<i64>) -> Result<ManagementReceipt,String> {
        if work.trim().is_empty() || work.len()>4096 { return Err("Некорректное название работы.".into()); }
        let mut editor = self.management_begin_settings(revision,256*1024*1024,Arc::new(ManagementJob::new()))?;
        editor.assign_work(work, group)?;
        self.management_commit_settings(editor)
    }
}

fn unified_remove_work(c:&Connection, work:&str) -> Result<(),String> {
    // Record only source groups actually losing this work. Pre-existing empty
    // groups may carry user metadata and are unrelated to this edit.
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS unified_touched_groups(handle INTEGER PRIMARY KEY);").map_err(err)?;
    c.execute("INSERT OR IGNORE INTO unified_touched_groups SELECT g.handle FROM management_nodes g JOIN management_nodes w ON w.parent=g.handle AND w.field='works' AND w.kind='array' JOIN management_nodes n ON n.parent=w.handle AND n.kind='string' AND n.value=?1 WHERE g.parent=?2",params![work,settings(c)?.1]).map_err(err)?;
    c.execute("DELETE FROM management_nodes WHERE kind='string' AND value=?1 AND parent IN (SELECT w.handle FROM management_nodes w JOIN management_nodes g ON w.parent=g.handle WHERE w.field='works' AND w.kind='array' AND g.parent=?2)", params![work,settings(c)?.1]).map_err(err)?;
    Ok(())
}
fn check_group_name(c: &Connection, groups: i64, except: Option<i64>, name: &str) -> Result<(), String> {
    let normalized = unified_normalized(name);
    let mut stmt = c.prepare(
        "SELECT g.handle,n.value FROM management_nodes g
         JOIN management_nodes n ON n.parent=g.handle AND n.field='name' WHERE g.parent=?1",
    ).map_err(err)?;
    let mut names = stmt.query([groups]).map_err(err)?;
    while let Some(row) = names.next().map_err(err)? {
        if Some(row.get::<_, i64>(0).map_err(err)?) != except
            && unified_normalized(&row.get::<_, String>(1).map_err(err)?) == normalized {
            return Err("Группа с таким названием уже существует.".into());
        }
    }
    Ok(())
}

fn check_tree_group_id(c: &Connection, groups: i64, id: &str) -> Result<(), String> {
    if unified_settings_io::group_parents_node(c)?.is_some()
        && c.query_row(
            "SELECT EXISTS(SELECT 1 FROM management_nodes g JOIN management_nodes n
             ON n.parent=g.handle AND n.field='id' WHERE g.parent=?1 AND n.value=?2)",
            params![groups, id], |row| row.get::<_, bool>(0),
        ).map_err(err)? {
        return Err("Идентификатор группы уже существует.".into());
    }
    Ok(())
}

fn unified_remove_empty_groups(c:&Connection, except:Option<i64>) -> Result<(),String> {
    if unified_settings_io::group_parents_node(c)?.is_some() { return Ok(()); }
    loop {
        let id:Option<i64>=c.query_row("SELECT g.handle FROM management_nodes g JOIN unified_touched_groups touched ON touched.handle=g.handle JOIN management_nodes w ON w.parent=g.handle AND w.field='works' WHERE g.parent=?1 AND (?2 IS NULL OR g.handle<>?2) AND NOT EXISTS(SELECT 1 FROM management_nodes n WHERE n.parent=w.handle) LIMIT 1",params![settings(c)?.1,except],|r|r.get(0)).optional().map_err(err)?;
        match id { Some(id)=>remove_node(c,id)?, None=>return Ok(()) }
    }
}
impl SettingsEditor {
    fn assign_work(&mut self, work: &str, group: Option<i64>) -> Result<(), String> {
        self.mutate(|c| {
            let target=group.map(|g| works_node(c,g)).transpose()?;
            unified_remove_work(c,work)?;
            if let Some(parent)=target { add(c,parent,None,"string",Some(work))?; }
            unified_remove_empty_groups(c,group)?;
            Ok(())
        })
    }

    fn unified_save_group(&mut self, handle:Option<i64>, id:&str, name:&str, works:&[String]) -> Result<(),String> {
        self.mutate(|c| {
            let parent=settings(c)?.1;
            check_group_name(c, parent, handle, name)?;
            let target=if let Some(h)=handle { belongs(c,h,parent,"object")?;scalar(c,h,"name",ManagementScalar::String(name.into()))?;h } else {
                if id.trim().is_empty() { return Err("Недопустимый идентификатор группы.".into()); }
                check_tree_group_id(c, parent, id)?;
                let h=add(c,parent,None,"object",None)?;
                scalar(c,h,"id",ManagementScalar::String(id.into()))?;
                scalar(c,h,"name",ManagementScalar::String(name.into()))?;
                add(c,h,Some("works"),"array",None)?; h
            };
            let target_works=works_node(c,target)?;
            c.execute("DELETE FROM management_nodes WHERE parent=?1",[target_works]).map_err(err)?;
            let mut seen=std::collections::HashSet::new();
            for work in works { if seen.insert(work) { unified_remove_work(c,work)?;add(c,target_works,None,"string",Some(work))?; } }
            unified_remove_empty_groups(c,Some(target))?;
            Ok(())
        })
    }
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
fn settings_snapshot(c: &mut Connection, revision: i64) -> Result<Transaction<'_>, String> {
    let tx = c.transaction().map_err(err)?;
    checked(&tx, revision)?;
    Ok(tx)
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
    fn create(root: &Path, quota: u64) -> Result<Self, String> {
        if !(4 * 1024 * 1024..=16 * 1024 * 1024 * 1024).contains(&quota) {
            return Err("Session quota must be4MiB..16GiB".into());
        }
        let _lock = lifecycle_lock(root)?;
        let (_, active) = cleanup(root)?;
        if active >= 4 {
            return Err("At most4live management sessions per storage".into());
        }
        let root = stage_root(root);
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
        Ok(Self {
            dir,
            lease: Some(lease),
            quota,
        })
    }
    fn index_settings(
        root: &Path,
        filter_gate: &Mutex<()>,
        rev: i64,
        job: &ManagementJob,
        connect: impl FnOnce() -> Result<Connection, String>,
    ) -> Result<ManagementReceipt, String> {
        let stage = Self::create(root, 16 * 1024 * 1024 * 1024)?;
        let mut src = connect()?;
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
        // Join live filter writers only after the private stage is complete.
        let writer = filter_gate.lock().map_err(err)?;
        job.check()?;
        stage.attach(&src)?;
        let tx = src.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| format!("Begin settings index publication: {error}"))?;
        job.run(&tx, || {
            check_revision(&tx, rev)?;
            copy_tables(&tx)
        })?;
        job.finish(tx)?;
        drop(src);
        drop(writer);
        let cleanup_error = stage.cleanup().err();
        Ok(ManagementReceipt {
            revision: rev,
            deleted: 0,
            cleanup_error,
        })
    }

    fn reap(root: &Path) -> Result<(i64, i64), String> {
        let _lock = lifecycle_lock(root)?;
        cleanup(root)
    }
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
    fn attach(&self, c: &Connection) -> Result<(), String> {
        c.execute(
            "ATTACH DATABASE ?1 AS management_stage",
            [self.db().to_string_lossy().as_ref()],
        )
        .map_err(err)?;
        c.execute_batch("PRAGMA management_stage.cache_size=-2048;")
            .map_err(err)
    }
    fn cleanup(&self) -> Result<(), String> {
        fs::remove_dir_all(&self.dir).map_err(err)
    }
    fn write_document(
        &self,
        c: &Connection,
        root: i64,
        filename: &str,
        job: &ManagementJob,
    ) -> Result<StagedDocument, String> {
        let path = self.dir.join(filename);
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(err)?;
        let mut writer = DocumentWriter {
            inner: BufWriter::with_capacity(65536, file),
            bytes: 0,
            limit: (self.quota / 3).min(i32::MAX as u64),
            hash: Sha256::new(),
        };
        tree::write(c, root, &mut writer, job, 0)?;
        writer.flush().map_err(err)?;
        writer.inner.get_ref().sync_all().map_err(err)?;
        let DocumentWriter { bytes, hash, .. } = writer;
        Ok(StagedDocument {
            path,
            bytes,
            digest: hash.finalize().into(),
        })
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
fn stage_root(root: &Path) -> PathBuf {
    root.join("management-stages")
}
fn lifecycle_lock(root: &Path) -> Result<fs::File, String> {
    let f = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(root.join("management-stages.lock"))
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
fn cleanup(root: &Path) -> Result<(i64, i64), String> {
    let root = stage_root(root);
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
impl Storage {
    fn management_connect(&self) -> Result<Connection, String> {
        let mut c = new_connection(&self.db_path())?;
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
        Stage::reap(&self.root)
    }
    pub fn management_index_settings(
        &self,
        rev: i64,
        job: &ManagementJob,
    ) -> Result<ManagementReceipt, String> {
        Stage::index_settings(&self.root, &self.analysis_filter_gate, rev, job, || {
            self.management_connect()
        })
    }
    pub fn management_bootstrap(&self, rev: i64) -> Result<ManagementBootstrap, String> {
        let mut c = self.management_connect()?;
        let tx = settings_snapshot(&mut c, rev)?;
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
        Ok(ManagementBootstrap{revision:rev,org_names,groups:count(&tx,"management_groups")?,work_group_hierarchy:unified_settings_io::group_parents_node(&tx)?.is_some(),memberships:count(&tx,"management_members")?,catalog:count(&tx,"management_catalog")?,reports:count(&tx,"reports")?,indexed_reports:tx.query_row("SELECT COUNT(*) FROM reports r JOIN management_report_meta m ON m.report_id=r.id JOIN report_row_index i ON i.report_id=r.id",[],|r|r.get(0)).map_err(err)?})
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
        let tx = settings_snapshot(&mut c, rev)?;
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
        let tx = settings_snapshot(&mut c, rev)?;
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
        let tx = settings_snapshot(&mut c, rev)?;
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
        let tx = settings_snapshot(&mut c, rev)?;
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
        let tx = settings_snapshot(&mut c, rev)?;
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
        let tx = settings_snapshot(&mut c, rev)?;
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
/// One editor operation with identifiers already parsed by the caller.
pub enum SettingsEdit {
    CreateGroup { id: String, name: String },
    RenameGroup { handle: i64, name: String },
    DeleteGroup { handle: i64 },
    AppendMember { group: i64, work: String },
    RemoveMember { group: i64, handle: i64 },
    CreateCatalog { work: String, organization: Option<String> },
    UpdateCatalog { handle: i64, work: String, organization: Option<String> },
    DeleteCatalog { handle: i64 },
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
    pub fn apply(&mut self, edit: SettingsEdit) -> Result<(), String> {
        match edit {
            SettingsEdit::CreateGroup { id, name } => self.create_group(&id, &name).map(|_| ()),
            SettingsEdit::RenameGroup { handle, name } => self.rename_group(handle, &name),
            SettingsEdit::DeleteGroup { handle } => self.delete_group(handle),
            SettingsEdit::AppendMember { group, work } => self.append_members(group, &[work]),
            SettingsEdit::RemoveMember { group, handle } => self.remove_members(group, &[handle]),
            SettingsEdit::CreateCatalog { work, organization } => {
                self.create_catalog(&work, organization.as_deref()).map(|_| ())
            }
            SettingsEdit::UpdateCatalog { handle, work, organization } => {
                self.update_catalog(handle, &work, organization.as_deref())
            }
            SettingsEdit::DeleteCatalog { handle } => self.delete_catalog(&[handle]),
        }
    }
    pub fn cancel(&self) -> bool {
        self.job.cancel()
    }
    pub fn set_scalar(&mut self, name: &str, value: ManagementScalar) -> Result<(), String> {
        if matches!(name, "orgNames" | "groups" | "catalog" | work_group_hierarchy::GROUP_PARENTS_FIELD) {
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
            if unified_settings_io::group_parents_node(c)?.is_some() {
                check_group_name(c, groups, None, name)?;
                check_tree_group_id(c, groups, id)?;
            }
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
            let groups = settings(c)?.1;
            belongs(c, group, groups, "object")?;
            if unified_settings_io::group_parents_node(c)?.is_some() {
                check_group_name(c, groups, Some(group), name)?;
            }
            scalar(c, group, "name", ManagementScalar::String(name.into()))
        })
    }
    pub fn delete_group(&mut self, group: i64) -> Result<(), String> {
        self.mutate(|c| {
            if unified_settings_io::group_parents_node(c)?.is_some() {
                return Err("Выберите, что сделать с подгруппами.".into());
            }
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
            let hierarchy = unified_settings_io::group_parents_node(c)?.is_some();
            for work in works {
                if hierarchy && c.query_row(
                    "SELECT EXISTS(SELECT 1 FROM management_nodes g
                     JOIN management_nodes w ON w.parent=g.handle AND w.field='works'
                     JOIN management_nodes n ON n.parent=w.handle AND n.kind='string'
                     WHERE g.parent=?1 AND n.value=?2)",
                    params![settings(c)?.1, work], |row| row.get::<_, bool>(0),
                ).map_err(err)? {
                    return Err("Работа уже входит в группу.".into());
                }
                add(c, parent, None, "string", Some(work))?;
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
struct StagedDocument {
    path: PathBuf,
    bytes: u64,
    digest: [u8; 32],
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
impl SettingsEditor {
    fn begin(
        root: &Path,
        database: &Path,
        expected: i64,
        quota: u64,
        job: Arc<ManagementJob>,
        connect: impl FnOnce() -> Result<Connection, String>,
    ) -> Result<Self, String> {
        job.check()?;
        let stage = Stage::create(root, quota)?;
        let mut source = connect()?;
        let tx = settings_snapshot(&mut source, expected)?;
        let dest = stage.open()?;
        tree::schema(&dest)?;
        job.run(&dest,||{let st=dest.unchecked_transaction().map_err(err)?;let mut stmt=tx.prepare("SELECT handle,parent,ordinal,field,kind,value FROM management_nodes ORDER BY handle").map_err(err)?;let mut rows=stmt.query([]).map_err(err)?;let mut ins=st.prepare("INSERT INTO management_nodes VALUES(?1,?2,?3,?4,?5,?6)").map_err(err)?;while let Some(r)=rows.next().map_err(err)?{job.check()?;ins.execute(params![r.get::<_,i64>(0).map_err(err)?,r.get::<_,Option<i64>>(1).map_err(err)?,r.get::<_,i64>(2).map_err(err)?,r.get::<_,Option<String>>(3).map_err(err)?,r.get::<_,String>(4).map_err(err)?,r.get::<_,Option<String>>(5).map_err(err)?]).map_err(err)?;}drop(ins);tree::derive(&st,settings(&tx)?.0,&job)?;st.commit().map_err(err)})?;
        drop(dest);
        stage.check_quota()?;
        Ok(Self {
            stage,
            expected,
            database: fs::canonicalize(database).map_err(err)?,
            job,
        })
    }

    fn commit(
        self,
        database: &Path,
        page_limit: Option<i64>,
        connect: impl FnOnce() -> Result<Connection, String>,
    ) -> Result<ManagementReceipt, String> {
        if fs::canonicalize(database).map_err(err)? != self.database {
            return Err("Settings session belongs to another database".into());
        }
        self.job.check()?;
        let staged = self.stage.open()?;
        let root = settings(&staged)?.0;
        self.job.run(&staged, || {
            let tx = staged.unchecked_transaction().map_err(err)?;
            tree::derive(&tx, root, &self.job)?;
            tx.commit().map_err(err)
        })?;
        let document = self
            .stage
            .write_document(&staged, root, "settings.json", &self.job)?;
        drop(staged);
        self.stage.check_quota()?;
        let c = connect()?;
        if let Some(limit) = page_limit {
            c.pragma_update(None, "max_page_count", limit)
                .map_err(err)?;
        }
        self.stage.attach(&c)?;
        self.job.check()?;
        let revision = self.job.run(&c, || {
            let tx = c.unchecked_transaction().map_err(err)?;
            check_revision(&tx, self.expected)?;
            tx.execute("DELETE FROM kv WHERE name='settings'", [])
                .map_err(err)?;
            tx.execute(
                "INSERT INTO kv(name,json) VALUES('settings',zeroblob(?1))",
                [document.bytes],
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
                    BufReader::with_capacity(65536, fs::File::open(&document.path).map_err(err)?);
                let mut buf = [0u8; 65536];
                let mut hash = Sha256::new();
                let mut copied = 0u64;
                loop {
                    self.job.check()?;
                    let n = input.read(&mut buf).map_err(err)?;
                    if n == 0 {
                        break;
                    }
                    copied += n as u64;
                    if copied > document.bytes {
                        return Err("Settings document grew during publication".into());
                    }
                    blob.write_all(&buf[..n]).map_err(err)?;
                    hash.update(&buf[..n]);
                }
                blob.close().map_err(err)?;
                if copied != document.bytes || <[u8; 32]>::from(hash.finalize()) != document.digest {
                    return Err("Settings document changed during publication".into());
                }
            }
            copy_tables(&tx)?;
            self.job.check()?;
            let rev = read_model::revision(&tx)?;
            self.job.finish(tx)?;
            Ok(rev)
        })?;
        drop(c);
        let cleanup_error = self.stage.cleanup().err();
        Ok(ManagementReceipt {
            revision,
            deleted: 0,
            cleanup_error,
        })
    }
}
impl Storage {
    pub fn management_begin_settings(
        &self,
        expected: i64,
        quota: u64,
        job: Arc<ManagementJob>,
    ) -> Result<SettingsEditor, String> {
        SettingsEditor::begin(&self.root, &self.db_path(), expected, quota, job, || {
            self.management_connect()
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
        editor.commit(&self.db_path(), page_limit, || self.management_connect())
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

        // Explicit backup callers retain the original report before indexing.
        self.prepare_single_report_deletion(expected, id, job)?;
        self.management_delete_reports(expected, &[id.to_owned()], job)
    }
    pub fn management_delete_report(
        &self,
        expected: i64,
        id: &str,
        job: &ManagementJob,
    ) -> Result<ManagementReceipt, String> {
        self.prepare_single_report_deletion(expected, id, job)?;
        let scope = UnifiedReportDeletion::Selected { ids: vec![id.to_owned()] };
        self.unified_delete_reports(expected, &scope, 1, job)
    }
    fn prepare_single_report_deletion(
        &self,
        expected: i64,
        id: &str,
        job: &ManagementJob,
    ) -> Result<(), String> {
        label(id)?;
        job.check()?;
        check_revision(&self.management_connect()?, expected)?;
        self.index_report(id, expected)?;
        job.check()?;
        self.management_index_report(id, expected, &ManagementJob::new())
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
        check_delete_integrity(&tx)?;
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
fn check_delete_integrity(conn: &Connection) -> Result<(), String> {
    let result: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(err)?;
    if result != "ok" {
        return Err("SQLite integrity check failed before report deletion".into());
    }
    Ok(())
}

#[path = "management_reports.rs"]
mod reports;
#[path = "unified_settings_io.rs"]
mod unified_settings_io;
pub use unified_settings_io::{GroupTreeEdit,UnifiedSettingsReview,UnifiedCatalogReceipt};
pub(super) use unified_settings_io::PreparedCatalogSettings;
#[path = "unified_data_admin.rs"]
mod unified_data_admin;
pub use unified_data_admin::UnifiedReportDeletion;
pub use reports::{
    ManagementPeriod, ManagementReport, ManagementReportCursor, ManagementReportFilter,
    ManagementReportSummary,
};
impl Storage {
    pub fn management_group_count(&self, rev: i64, query: &str) -> Result<i64, String> {
        search(query)?;
        let mut c = self.management_connect()?;
        let tx = settings_snapshot(&mut c, rev)?;
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
        let tx = settings_snapshot(&mut c, rev)?;
        tx.query_row("SELECT COUNT(*) FROM management_catalog WHERE instr(work,?1)>0 AND (?2 IS NULL OR organization=?2)",params![query,org],|r|r.get(0)).map_err(err)
    }
}
