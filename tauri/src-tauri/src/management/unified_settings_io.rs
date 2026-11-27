//! File-based settings transfers. Reports never enter these documents or IPC.
use super::*;
use crate::storage::work_group_hierarchy::{validate_group_parents, GroupParent, GROUP_PARENTS_FIELD, MAX_GROUPS};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Deserialize)]
struct GroupsDocument {
    format: String,
    version: u32,
    groups: Vec<GroupDocument>,
    #[serde(default)]
    parents: Option<Value>,
}

pub(super) fn group_parents_node(c: &Connection) -> Result<Option<i64>, String> {
    let root = settings(c)?.0;
    match tree::child(c, root, GROUP_PARENTS_FIELD)? {
        Some((node, kind, _)) if kind == "array" => Ok(Some(node)),
        None => Ok(None),
        _ => Err("Некорректные связи дерева групп в настройках.".into()),
    }
}

fn write_group_parents(c: &Connection, parents: Option<&impl Serialize>, job: &ManagementJob) -> Result<(), String> {
    let root = settings(c)?.0;
    if let Some(node) = group_parents_node(c)? {
        remove_node(c, node)?;
    }
    if let Some(parents) = parents {
        let node = tree::parse(c, std::io::Cursor::new(serde_json::to_vec(parents).map_err(err)?), job)?;
        let ordinal: i64 = c.query_row(
            "SELECT COALESCE(MAX(ordinal)+1,0) FROM management_nodes WHERE parent=?1",
            [root], |row| row.get(0),
        ).map_err(err)?;
        c.execute(
            "UPDATE management_nodes SET parent=?1,ordinal=?2,field=?3 WHERE handle=?4",
            params![root,ordinal,GROUP_PARENTS_FIELD,node],
        ).map_err(err)?;
    }
    Ok(())
}

fn read_group_parents(c: &Connection, job: &ManagementJob) -> Result<Vec<GroupParent>, String> {
    let Some(node) = group_parents_node(c)? else { return Ok(Vec::new()); };
    let mut document = Vec::new();
    tree::write(c, node, &mut document, job, 0)?;
    serde_json::from_slice(&document).map_err(err)
}

// Input links have already been validated. Follow removed parents until the
// closest surviving ancestor, which can be the root of the entire forest.
fn promote_surviving_children(links: &[GroupParent], removed: &HashSet<String>) -> Vec<GroupParent> {
    let parents: HashMap<_, _> = links.iter()
        .map(|link| (link.child.as_str(), link.parent.as_str())).collect();
    links.iter().filter_map(|link| {
        if removed.contains(&link.child) {
            return None;
        }
        let mut parent = link.parent.as_str();
        while removed.contains(parent) {
            parent = *parents.get(parent)?;
        }
        Some(GroupParent { child: link.child.clone(), parent: parent.to_owned() })
    }).collect()
}

pub(super) fn update_links_before_bulk_deletion(
    c: &Connection,
    list: i64,
    handles: Option<&[i64]>,
    clear: bool,
    job: &ManagementJob,
) -> Result<(), String> {
    if group_parents_node(c)?.is_none() {
        return Ok(());
    }
    if clear {
        return write_group_parents(c, Some(&Vec::<GroupParent>::new()), job);
    }
    let handles = handles.ok_or("Укажите удаляемые группы.")?;
    let removed = handles.iter().map(|&handle| group_id(c, list, handle))
        .collect::<Result<HashSet<_>, _>>()?;
    let links = read_group_parents(c, job)?;
    let ids = group_ids(c, list)?;
    validate_group_parents(ids.keys().map(String::as_str), &links)?;
    let remaining = promote_surviving_children(&links, &removed);
    write_group_parents(c, Some(&remaining), job)
}

pub enum GroupTreeEdit {
    Create { id: String, name: String, parent: Option<i64> },
    Move { group: i64, parent: Option<i64> },
    Delete { group: i64, delete_descendants: bool, confirmed: bool },
}

fn group_id(c: &Connection, list: i64, handle: i64) -> Result<String, String> {
    belongs(c, handle, list, "object")?;
    tree::child(c, handle, "id")?.and_then(|(_, _, id)| id)
        .ok_or_else(|| "У группы нет идентификатора.".into())
}

fn group_ids(c: &Connection, list: i64) -> Result<HashMap<String, i64>, String> {
    let mut stmt = c.prepare(
        "SELECT id.value,g.handle FROM management_nodes g
         LEFT JOIN management_nodes id ON id.parent=g.handle AND id.field='id' AND id.kind='string'
         WHERE g.parent=?1 AND g.kind='object' LIMIT ?2",
    ).map_err(err)?;
    let mut rows = stmt.query(params![list,(MAX_GROUPS + 1) as i64]).map_err(err)?;
    let mut ids = HashMap::new();
    while let Some(row) = rows.next().map_err(err)? {
        let id = row.get::<_, Option<String>>(0).map_err(err)?
            .ok_or("Перед созданием дерева задайте идентификаторы всем группам.")?;
        if id.trim().is_empty() || ids.insert(id, row.get(1).map_err(err)?).is_some() {
            return Err("Перед созданием дерева исправьте пустые и повторяющиеся идентификаторы групп.".into());
        }
        if ids.len() > MAX_GROUPS {
            return Err(format!("Дерево может содержать не более {MAX_GROUPS} групп."));
        }
    }
    Ok(ids)
}

#[derive(Deserialize, Serialize)]
struct GroupDocument {
    #[serde(default)]
    id: String,
    name: String,
    works: Vec<String>,
    #[serde(flatten)]
    extensions: serde_json::Map<String, serde_json::Value>,
}
#[derive(Debug, Serialize)]
pub struct UnifiedSettingsReview {
    pub hash: String,
    pub groups: u64,
    pub works: u64,
    pub catalog: u64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub requires_backup: bool,
}
#[derive(Debug, Serialize)]
pub struct UnifiedCatalogReceipt {
    pub revision: i64,
    pub added: u64,
    pub cleanup_warning: bool,
}

/// An owned private settings document attached to the import connection. Its
/// publication joins the report transaction and its rollback/receipt boundary.
pub(crate) struct PreparedCatalogSettings {
    editor: SettingsEditor,
    document: StagedDocument,
}
impl PreparedCatalogSettings {
    pub(crate) fn attach(&self, c: &Connection) -> Result<(), String> {
        if fs::canonicalize(c.path().ok_or("Import requires a file database")?).map_err(err)?
            != self.editor.database
        {
            return Err("Catalog stage belongs to another database.".into());
        }
        self.editor.stage.attach(c)
    }
    pub(crate) fn publish(
        &self,
        tx: &Connection,
        starting_revision: i64,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        if starting_revision != self.editor.expected {
            return Err("Catalog stage belongs to another import revision.".into());
        }
        if cancel.load(Ordering::Acquire) {
            return Err("Import cancelled before catalog publication.".into());
        }
        tx.execute("DELETE FROM kv WHERE name='settings'", [])
            .map_err(err)?;
        tx.execute(
            "INSERT INTO kv(name,json) VALUES('settings',zeroblob(?1))",
            [self.document.bytes],
        )
        .map_err(err)?;
        let rowid = tx.last_insert_rowid();
        let mut blob = tx
            .blob_open("main", "kv", "json", rowid, false)
            .map_err(err)?;
        let mut input = BufReader::new(fs::File::open(&self.document.path).map_err(err)?);
        let mut buffer = [0u8; 65536];
        let mut copied = 0u64;
        let mut hash = Sha256::new();
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err("Import cancelled during catalog publication.".into());
            }
            let n = input.read(&mut buffer).map_err(err)?;
            if n == 0 {
                break;
            }
            copied += n as u64;
            if copied > self.document.bytes {
                return Err("Catalog document grew during publication.".into());
            }
            blob.write_all(&buffer[..n]).map_err(err)?;
            hash.update(&buffer[..n]);
        }
        blob.close().map_err(err)?;
        if copied != self.document.bytes || <[u8; 32]>::from(hash.finalize()) != self.document.digest {
            return Err("Catalog document changed during publication.".into());
        }
        copy_tables(tx)
    }
}

impl Storage {
    pub(crate) fn unified_prepare_import_catalog(
        &self,
        source: &Connection,
        prepared: &analiz_import_core::PreparedImport<'_>,
        revision: i64,
        cancellation: Arc<AtomicBool>,
    ) -> Result<Option<PreparedCatalogSettings>, String> {
        PreparedCatalogSettings::prepare(&self.root, &self.db_path(), source, prepared, revision, cancellation)
    }
}

impl PreparedCatalogSettings {
    fn prepare(
        root: &Path,
        database: &Path,
        source: &Connection,
        prepared: &analiz_import_core::PreparedImport<'_>,
        revision: i64,
        cancellation: Arc<AtomicBool>,
    ) -> Result<Option<Self>, String> {
        let cancel = cancellation.as_ref();
        if cancel.load(Ordering::Acquire) {
            return Err("Import cancelled before catalog preparation.".into());
        }
        // A read-only snapshot must never rebuild live management indexes here:
        // an unrelated writer may hold BEGIN IMMEDIATE while the user cancels.
        let snapshot = source.unchecked_transaction().map_err(err)?;
        check_revision(&snapshot, revision)?;
        let settings_row: Option<i64> = snapshot
            .query_row("SELECT rowid FROM kv WHERE name='settings'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(err)?;
        #[derive(Deserialize)]
        struct OrganizationSettings {
            #[serde(rename = "orgNames")]
            names: [String; 4],
        }
        struct CancelReader<'a, R> {
            inner: R,
            cancel: &'a AtomicBool,
        }
        impl<R: Read> Read for CancelReader<'_, R> {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                if self.cancel.load(Ordering::Acquire) {
                    return Err(std::io::Error::other(
                        "Import cancelled during settings read.",
                    ));
                }
                self.inner.read(buffer)
            }
        }
        let names = if let Some(rowid) = settings_row {
            let blob = snapshot
                .blob_open("main", "kv", "json", rowid, true)
                .map_err(err)?;
            let settings: OrganizationSettings = serde_json::from_reader(CancelReader {
                inner: BufReader::with_capacity(65536, blob),
                cancel,
            })
            .map_err(err)?;
            for name in &settings.names {
                label(name)?;
            }
            settings.names.to_vec()
        } else {
            vec!["СМУ16".into(), "АВТО".into(), "ТРАК".into(), "ЦЕНТР".into()]
        };
        // Most workbooks have reports only. Avoid copying settings unless an
        // actual catalog was detected in this exact, source-bound artifact.
        if prepared
            .visit_catalog(&names, cancel, |_, _| Ok(()))
            .map_err(|e| e.to_string())?
            == 0
        {
            return Ok(None);
        }
        let job = Arc::new(ManagementJob::with_cancellation(cancellation.clone()));
        let stage = Stage::create(root, 256 * 1024 * 1024)?;
        let staged = stage.open()?;
        tree::schema(&staged)?;
        job.run(&staged, || {
            let tx = staged.unchecked_transaction().map_err(err)?;
            let root = if let Some(rowid) = settings_row {
                tree::parse(
                    &tx,
                    snapshot
                        .blob_open("main", "kv", "json", rowid, true)
                        .map_err(err)?,
                    &job,
                )?
            } else {
                tree::parse(
                    &tx,
                    std::io::Cursor::new(
                        r#"{"orgNames":["СМУ16","АВТО","ТРАК","ЦЕНТР"],"groups":[],"catalog":[]}"#
                            .as_bytes(),
                    ),
                    &job,
                )?
            };
            tree::derive(&tx, root, &job)?;
            tx.commit().map_err(err)
        })?;
        drop(staged);
        snapshot.commit().map_err(err)?;
        stage.check_quota()?;
        let mut editor = SettingsEditor {
            stage,
            expected: revision,
            database: fs::canonicalize(database).map_err(err)?,
            job,
        };
        let added = editor.unified_merge_catalog_source(|accept| {
            prepared.visit_catalog(&names, cancel, accept)
        })?;
        if added == 0 {
            return Ok(None);
        }
        let staged = editor.stage.open()?;
        let root = settings(&staged)?.0;
        let tx = staged.unchecked_transaction().map_err(err)?;
        tree::derive(&tx, root, &editor.job)?;
        tx.commit().map_err(err)?;
        let document = editor
            .stage
            .write_document(&staged, root, "import-settings.json", &editor.job)?;
        drop(staged);
        editor.stage.check_quota()?;
        Ok(Some(Self { editor, document }))
    }
}

impl SettingsEditor {
    fn edit_group_tree(&mut self, action: GroupTreeEdit) -> Result<bool, String> {
        let job = self.job.clone();
        self.mutate(|c| {
            let list = settings(c)?.1;
            let mut links = read_group_parents(c, &job)?;
            let existing = group_ids(c, list)?;
            validate_group_parents(existing.keys().map(String::as_str), &links)?;
            match action {
                GroupTreeEdit::Create { id, name, parent } => {
                    if id.trim().is_empty() || id.len() > 128 || name.trim().is_empty() || name.len() > 1024 {
                        return Err("Введите допустимое название и идентификатор группы.".into());
                    }
                    let parent = parent.map(|handle| group_id(c, list, handle)).transpose()?;
                    check_group_name(c, list, None, &name)?;
                    let handle = add(c, list, None, "object", None)?;
                    scalar(c, handle, "id", ManagementScalar::String(id.clone()))?;
                    scalar(c, handle, "name", ManagementScalar::String(name.trim().into()))?;
                    add(c, handle, Some("works"), "array", None)?;
                    if let Some(parent) = parent {
                        links.push(GroupParent { child: id, parent });
                    }
                }
                GroupTreeEdit::Move { group, parent } => {
                    let child = group_id(c, list, group)?;
                    let parent = parent.map(|handle| group_id(c, list, handle)).transpose()?;
                    links.retain(|link| link.child != child);
                    if let Some(parent) = parent {
                        links.push(GroupParent { child, parent });
                    }
                }
                GroupTreeEdit::Delete { group, delete_descendants, confirmed } => {
                    if !confirmed {
                        return Err("Подтвердите удаление и выберите, что делать с подгруппами.".into());
                    }
                    let id = group_id(c, list, group)?;
                    if delete_descendants {
                        let mut descendants = HashMap::<&str, Vec<&str>>::new();
                        for link in &links {
                            descendants.entry(&link.parent).or_default().push(&link.child);
                        }
                        let mut deleted = HashSet::new();
                        let mut pending = vec![id.as_str()];
                        while let Some(child) = pending.pop() {
                            if !deleted.insert(child) { continue; }
                            let handle = existing.get(child).ok_or("Дерево ссылается на отсутствующую группу.")?;
                            remove_node(c, *handle)?;
                            pending.extend(descendants.get(child).into_iter().flatten().copied());
                        }
                        let removed: HashSet<String> = deleted.into_iter().map(str::to_owned).collect();
                        links.retain(|link| !removed.contains(&link.child));
                    } else {
                        links = promote_surviving_children(&links, &HashSet::from([id]));
                        remove_node(c, group)?;
                    }
                }
            }
            let ids = group_ids(c, list)?;
            validate_group_parents(ids.keys().map(String::as_str), &links)?;
            let conflicting_assignments: bool = c.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM management_nodes g
                    JOIN management_nodes w ON w.parent=g.handle AND w.field='works' AND w.kind='array'
                    JOIN management_nodes n ON n.parent=w.handle AND n.kind='string'
                    WHERE g.parent=?1 GROUP BY n.value HAVING COUNT(DISTINCT g.handle)>1
                 )",
                [list], |row| row.get(0),
            ).map_err(err)?;
            if conflicting_assignments {
                return Err("Перед созданием дерева оставьте для каждой работы только одну прямую группу.".into());
            }
            write_group_parents(c, Some(&links), &job)?;
            Ok(ids.is_empty())
        })
    }

    fn replace_groups(&mut self, mut doc: GroupsDocument, actual: &str) -> Result<bool, String> {
        let job = self.job.clone();
        self.mutate(|c| {
            let parent=settings(c)?.1;
            let needs_backup = doc.parents.is_some() || group_replacement_needs_backup(c)?;
            write_group_parents(c, doc.parents.as_ref(), &job)?;
            c.execute("WITH RECURSIVE descendants(handle) AS (SELECT handle FROM management_nodes WHERE parent=?1 UNION ALL SELECT n.handle FROM management_nodes n JOIN descendants d ON n.parent=d.handle) DELETE FROM management_nodes WHERE handle IN descendants",[parent]).map_err(err)?;
            for (ordinal,g) in doc.groups.iter_mut().enumerate() {
                if g.id.trim().is_empty() {g.id=format!("import-{}-{}", &actual[..16],ordinal);}
                g.name=g.name.trim().into();
                let serialized=serde_json::to_vec(g).map_err(err)?;
                let group=tree::parse(c,std::io::Cursor::new(serialized),&job)?;
                c.execute("UPDATE management_nodes SET parent=?1,ordinal=?2,field=NULL WHERE handle=?3",params![parent,ordinal as i64,group]).map_err(err)?;
            }
            Ok(needs_backup)
        })
    }

    fn unified_merge_catalog(
        &mut self,
        stage: &analiz_import_core::Stage<'_>,
        names: &[String],
        cancel: &AtomicBool,
    ) -> Result<u64, String> {
        self.unified_merge_catalog_source(|accept| stage.visit_catalog(names, cancel, accept))
    }
    fn unified_merge_catalog_source(
        &mut self,
        visit: impl FnOnce(
            &mut dyn FnMut(&str, &str) -> analiz_import_core::Result<()>,
        ) -> analiz_import_core::Result<u64>,
    ) -> Result<u64, String> {
        self.mutate(|c| {
            c.execute_batch("CREATE TEMP TABLE catalog_seen(org TEXT,work TEXT,PRIMARY KEY(org,work)) WITHOUT ROWID;").map_err(err)?;
            let mut stmt=c.prepare("SELECT organization,work FROM management_catalog").map_err(err)?;
            let mut rows=stmt.query([]).map_err(err)?;
            while let Some(row)=rows.next().map_err(err)? {
                let org:Option<String>=row.get(0).map_err(err)?;let work:String=row.get(1).map_err(err)?;
                c.execute("INSERT OR IGNORE INTO catalog_seen VALUES(?1,?2)",params![unified_normalized(org.as_deref().unwrap_or("")),unified_normalized(&work)]).map_err(err)?;
            }
            drop(rows);drop(stmt);
            let catalog=settings(c)?.2;let mut added=0;
            visit(&mut |org,work| {
                if c.execute("INSERT OR IGNORE INTO catalog_seen VALUES(?1,?2)",params![unified_normalized(org),unified_normalized(work)])? >0 {
                    let h=add(c,catalog,None,"object",None).map_err(|e|std::io::Error::other(e))?;
                    scalar(c,h,"work",ManagementScalar::String(work.into())).map_err(|e|std::io::Error::other(e))?;
                    scalar(c,h,"organization",ManagementScalar::String(org.into())).map_err(|e|std::io::Error::other(e))?;
                    added+=1;
                }
                Ok(())
            }).map_err(|e|e.to_string())?;
            Ok(added)
        })
    }
}

impl Storage {
    pub fn unified_catalog_preview(
        &self,
        revision: i64,
        source: &Path,
    ) -> Result<UnifiedSettingsReview, String> {
        let names = self.management_bootstrap(revision)?.org_names;
        let area = analiz_import_core::StageArea::open(self.root().join("unified-catalog-stages"))
            .map_err(|e| e.to_string())?;
        let cancel = AtomicBool::new(false);
        let stage = area
            .ingest(
                source,
                analiz_import_core::Limits::default(),
                &cancel,
                |_| {},
            )
            .map_err(|e| e.to_string())?;
        let count = stage
            .visit_catalog(&names, &cancel, |_, _| Ok(()))
            .map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("В Excel не найден справочник услуг по настроенным подразделениям.".into());
        }
        Ok(UnifiedSettingsReview {
            hash: stage.source_hash.clone(),
            groups: 0,
            works: 0,
            catalog: count,
            requires_backup: false,
        })
    }
    pub fn unified_catalog_apply(
        &self,
        revision: i64,
        source: &Path,
        hash: &str,
        confirmed: bool,
    ) -> Result<UnifiedCatalogReceipt, String> {
        if !confirmed {
            return Err("Подтвердите добавление справочника.".into());
        }
        let names = self.management_bootstrap(revision)?.org_names;
        let area = analiz_import_core::StageArea::open(self.root().join("unified-catalog-stages"))
            .map_err(|e| e.to_string())?;
        let cancel = AtomicBool::new(false);
        let stage = area
            .ingest(
                source,
                analiz_import_core::Limits::default(),
                &cancel,
                |_| {},
            )
            .map_err(|e| e.to_string())?;
        if stage.source_hash != hash {
            return Err("Excel изменился. Повторите проверку справочника.".into());
        }
        let mut editor = self.management_begin_settings(
            revision,
            256 * 1024 * 1024,
            Arc::new(ManagementJob::new()),
        )?;
        let added = editor.unified_merge_catalog(&stage, &names, &cancel)?;
        let receipt = self.management_commit_settings(editor)?;
        Ok(UnifiedCatalogReceipt {
            revision: receipt.revision,
            added,
            cleanup_warning: receipt.cleanup_error.is_some(),
        })
    }
}
const GROUP_FILE_MIB: usize = 64;
const MAX_IMPORTED_WORKS: usize = 200_000;

fn read_groups(source: &Path) -> Result<(GroupsDocument, String), String> {
    let file = fs::File::open(source).map_err(err)?;
    if !file.metadata().map_err(err)?.is_file() {
        return Err("Выберите обычный файл JSON.".into());
    }
    let mut bytes = Vec::new();
    file.take((GROUP_FILE_MIB * 1024 * 1024 + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.len() > GROUP_FILE_MIB * 1024 * 1024 {
        return Err(format!("Файл JSON больше {GROUP_FILE_MIB} МиБ."));
    }
    let mut doc: GroupsDocument = serde_json::from_slice(&bytes).map_err(err)?;
    if doc.format != "work-groups" || !matches!(doc.version, 1 | 2) {
        return Err("Неверный формат групп.".into());
    }
    if doc.groups.len() > MAX_GROUPS {
        return Err(format!("Допускается до {MAX_GROUPS} групп и подгрупп суммарно."));
    }
    let mut names = HashSet::new();
    let mut works = HashSet::new();
    let mut count = 0;
    for group in &mut doc.groups {
        if group.name.trim().is_empty()
            || group.name.len() > 1024
            || group.id.len() > 128
            || !names.insert(unified_normalized(&group.name))
        {
            return Err("Некорректная группа в JSON.".into());
        }
        if group.works.len() > MAX_GROUP_WORKS {
            return Err(format!("В группе допускается до {MAX_GROUP_WORKS} работ."));
        }
        let mut local = HashSet::new();
        group.works = group
            .works
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| local.insert(s.clone()))
            .collect();
        for work in &group.works {
            count += 1;
            if count > MAX_IMPORTED_WORKS {
                return Err(format!("В файле допускается до {MAX_IMPORTED_WORKS} работ."));
            }
            if work.trim().is_empty() || work.len() > 4096 || !works.insert(work)
            {
                return Err(
                    "Работа повторяется в группах, пуста или название длиннее 4096 байт.".into(),
                );
            }
        }
    }
    if doc.version == 2 {
        let parents = doc.parents.as_ref()
            .ok_or("В JSON отсутствует список связей дерева групп.")?;
        let count = parents.as_array().ok_or("Связи дерева групп должны быть списком.")?.len();
        if count > doc.groups.len() {
            return Err("Число связей дерева превышает число групп.".into());
        }
        let links = Vec::<GroupParent>::deserialize(parents).map_err(err)?;
        validate_group_parents(doc.groups.iter().map(|group| group.id.as_str()), &links)?;
    } else {
        // The flat format historically ignored unknown top-level extensions.
        doc.parents = None;
    }
    Ok((doc, format!("{:x}", Sha256::digest(&bytes))))
}
fn group_replacement_needs_backup(c: &Connection) -> Result<bool, String> {
    let groups = settings(c)?.1;
    let has_groups: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM management_nodes WHERE parent=?1 AND kind='object')",
        [groups], |row| row.get(0),
    ).map_err(err)?;
    Ok(has_groups || group_parents_node(c)?.is_some())
}

impl Storage {
    pub fn unified_group_tree_edit(&self, revision: i64, action: GroupTreeEdit) -> Result<ManagementReceipt, String> {
        self.unified_group_tree_edit_with_backup(revision, action, None)
    }

    pub fn unified_group_tree_edit_with_backup(
        &self,
        revision: i64,
        action: GroupTreeEdit,
        backup: Option<&Path>,
    ) -> Result<ManagementReceipt, String> {
        let mut editor = self.management_begin_settings(
            revision, 256 * 1024 * 1024, Arc::new(ManagementJob::new()),
        )?;
        if editor.edit_group_tree(action)? {
            let backup = backup.ok_or("Для очистки списка нужна проверенная резервная копия SQLite.")?;
            if self.backup_sqlite_verified(backup)? != revision {
                return Err("База изменилась при создании копии. Повторите действие.".into());
            }
        }
        self.management_commit_settings(editor)
    }

    pub fn unified_groups_preview(
        &self,
        revision: i64,
        source: &Path,
    ) -> Result<UnifiedSettingsReview, String> {
        let (doc, hash) = read_groups(source)?;
        let mut c = self.management_connect()?;
        let tx = settings_snapshot(&mut c, revision)?;
        Ok(UnifiedSettingsReview {
            hash,
            groups: doc.groups.len() as u64,
            works: doc.groups.iter().map(|g| g.works.len() as u64).sum(),
            catalog: 0,
            requires_backup: doc.parents.is_some() || group_replacement_needs_backup(&tx)?,
        })
    }
    pub fn unified_groups_import(
        &self,
        revision: i64,
        source: &Path,
        hash: &str,
        confirmed: bool,
    ) -> Result<ManagementReceipt, String> {
        self.unified_groups_import_with_backup(revision, source, hash, confirmed, None)
    }

    pub fn unified_groups_import_with_backup(
        &self,
        revision: i64,
        source: &Path,
        hash: &str,
        confirmed: bool,
        backup: Option<&Path>,
    ) -> Result<ManagementReceipt, String> {
        if !confirmed {
            return Err("Подтвердите замену групп.".into());
        }
        let (doc, actual) = read_groups(source)?;
        if actual != hash {
            return Err("Файл групп изменился. Повторите проверку.".into());
        }
        let mut editor = self.management_begin_settings(
            revision,
            256 * 1024 * 1024,
            Arc::new(ManagementJob::new()),
        )?;
        let needs_backup = editor.replace_groups(doc, &actual)?;
        if needs_backup {
            let path = backup.ok_or("Перед заменой групп создайте проверенную копию SQLite.")?;
            if self.backup_sqlite_verified(path)? != revision {
                return Err("База изменилась. Повторите проверку файла групп.".into());
            }
        }
        self.management_commit_settings(editor)
    }
    pub fn unified_groups_export(&self, revision: i64, destination: &Path) -> Result<(), String> {
        let mut c = self.management_connect()?;
        let tx = settings_snapshot(&mut c, revision)?;
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)
            .map_err(err)?;
        let result = (|| {
            let mut out = BufWriter::new(file);
            let parents = group_parents_node(&tx)?;
            let version = if parents.is_some() { 2 } else { 1 };
            write!(out, "{{\"format\":\"work-groups\",\"version\":{version},\"groups\":[").map_err(err)?;
            let mut groups = tx
                .prepare("SELECT handle,id,name FROM management_groups ORDER BY ordinal,handle")
                .map_err(err)?;
            let mut rows = groups.query([]).map_err(err)?;
            let mut first = true;
            while let Some(row) = rows.next().map_err(err)? {
                if !first {
                    out.write_all(b",").map_err(err)?;
                }
                first = false;
                let handle: i64 = row.get(0).map_err(err)?;
                tree::write(&tx, handle, &mut out, &ManagementJob::new(), 0)?;
            }
            out.write_all(b"]").map_err(err)?;
            if let Some(node) = parents {
                out.write_all(b",\"parents\":").map_err(err)?;
                tree::write(&tx, node, &mut out, &ManagementJob::new(), 0)?;
            }
            out.write_all(b"}\n").map_err(err)?;
            out.flush().map_err(err)?;
            out.get_ref().sync_all().map_err(err)
        })();
        if result.is_err() {
            let _ = fs::remove_file(destination);
        }
        result
    }
}

#[cfg(test)]
mod group_tree_file_tests {
    use super::*;

    fn review(groups: usize, parents: &str) -> Result<GroupsDocument, String> {
        let names: Vec<_> = (0..groups)
            .map(|i| serde_json::json!({"id":i.to_string(),"name":format!("Group {i}"),"works":[]}))
            .collect();
        let file = tempfile::NamedTempFile::new().unwrap();
        fs::write(
            file.path(),
            format!(r#"{{"format":"work-groups","version":2,"groups":{},"parents":{parents}}}"#,
                serde_json::json!(names)),
        ).unwrap();
        read_groups(file.path()).map(|(doc, _)| doc)
    }

    #[test]
    fn import_accepts_group_limit_and_rejects_one_more() {
        assert!(review(MAX_GROUPS, "[]").is_ok());
        assert!(review(MAX_GROUPS + 1, "[]").is_err());
    }

    #[test]
    fn import_checks_per_group_and_total_work_limits() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut groups = Vec::new();
        for start in (0..MAX_IMPORTED_WORKS).step_by(MAX_GROUP_WORKS) {
            let works: Vec<_> = (start..(start + MAX_GROUP_WORKS).min(MAX_IMPORTED_WORKS))
                .map(|i| i.to_string()).collect();
            groups.push(serde_json::json!({"name":start.to_string(),"works":works}));
        }
        let check = |groups: &[Value]| {
            let document = serde_json::json!({"format":"work-groups","version":1,"groups":groups});
            fs::write(file.path(), document.to_string()).unwrap();
            read_groups(file.path()).map(|_| ())
        };
        assert!(check(&groups).is_ok());
        groups.push(serde_json::json!({"name":"Extra","works":["extra"]}));
        assert!(check(&groups).unwrap_err().contains("200000"));
        groups.truncate(1);
        groups[0]["works"].as_array_mut().unwrap().push(serde_json::json!("extra"));
        assert!(check(&groups).unwrap_err().contains("10000"));
    }

    #[test]
    fn tree_group_file_accepts_multiple_levels_and_forward_references() {
        assert!(review(5, r#"[{"child":"0","parent":"1"},{"child":"1","parent":"2"},
            {"child":"2","parent":"3"},{"child":"3","parent":"4"}]"#).is_ok());
    }

    #[test]
    fn tree_group_file_round_trip_requires_verified_backup_and_preserves_reports() {
        let root = tempfile::tempdir().unwrap();
        let store = Storage::new(root.path().join("store")).unwrap();
        let original = serde_json::json!({
            "reports":[{"id":"original","type":"day","start":"2026-09-01","end":"2026-09-01",
                "rows":[{"organization":"A","work":"First","count":4,"amount":null}]}],
            "settings":{"orgNames":["A","B","C","D"],"catalog":[],"groups":[]}
        });
        store.save(original.clone()).unwrap();
        let revision = store.revision().unwrap();
        store.management_index_settings(revision, &ManagementJob::new()).unwrap();
        let document = root.path().join("tree.json");
        let json = serde_json::json!({
            "format":"work-groups","version":2,
            "groups":[{"id":"root","name":"Root","works":[]},{"id":"child","name":"Child","works":["First"]}],
            "parents":[{"child":"child","parent":"root"}]
        });
        fs::write(&document, json.to_string()).unwrap();
        let review = store.unified_groups_preview(revision, &document).unwrap();
        assert!(review.requires_backup);
        assert!(store.unified_groups_import(revision, &document, &review.hash, true).is_err());
        assert_eq!(store.load().unwrap(), original);

        let backup = root.path().join("before-tree.sqlite3");
        let receipt = store.unified_groups_import_with_backup(
            revision, &document, &review.hash, true, Some(&backup),
        ).unwrap();
        assert!(backup.is_file());
        assert_eq!(store.load().unwrap()["reports"], original["reports"]);
        let target = root.path().join("export.json");
        store.unified_groups_export(receipt.revision, &target).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&fs::read(target).unwrap()).unwrap(), json);
    }

    #[test]
    fn tree_edit_rejects_cycles_and_promotes_children_without_touching_reports() {
        let root = tempfile::tempdir().unwrap();
        let store = Storage::new(root.path().join("store")).unwrap();
        let original = serde_json::json!({
            "reports":[{"id":"retained","type":"day","start":"2026-09-01","end":"2026-09-01",
                "rows":[{"organization":"A","work":"First","count":4,"amount":null}]}],
            "settings":{"orgNames":["A","B","C","D"],"catalog":[],"groups":[
                {"id":"root","name":"Root","works":["First"]}]}
        });
        store.save(original.clone()).unwrap();
        let mut revision = store.revision().unwrap();
        store.management_index_settings(revision, &ManagementJob::new()).unwrap();
        let handle = |id: &str, rev| store.management_group_page(rev, "", None, 50).unwrap()
            .into_iter().find(|row| row.id == id).unwrap().handle;
        let root_id = handle("root", revision);
        revision = store.unified_group_tree_edit(revision, GroupTreeEdit::Create {
            id:"branch".into(), name:"Branch".into(), parent:Some(root_id),
        }).unwrap().revision;
        let branch = handle("branch", revision);
        revision = store.unified_group_tree_edit(revision, GroupTreeEdit::Create {
            id:"leaf".into(), name:"Leaf".into(), parent:Some(branch),
        }).unwrap().revision;
        let leaf = handle("leaf", revision);
        let mut conflicting_edit = store.management_begin_settings(
            revision, 16 * 1024 * 1024, Arc::new(ManagementJob::new()),
        ).unwrap();
        assert!(conflicting_edit.rename_group(leaf, " branch ").is_err());
        assert!(conflicting_edit.append_members(leaf, &["First".into()]).is_err());
        drop(conflicting_edit);
        assert!(store.unified_group_tree_edit(revision, GroupTreeEdit::Move {
            group:root_id, parent:Some(leaf),
        }).is_err());
        assert_eq!(store.revision().unwrap(), revision);
        store.unified_group_tree_edit(revision, GroupTreeEdit::Delete {
            group:branch, delete_descendants:false, confirmed:true,
        }).unwrap();
        let after = store.load().unwrap();
        assert_eq!(after["settings"]["workGroupTreeParents"], serde_json::json!([
            {"child":"leaf","parent":"root"}
        ]));
        assert_eq!(after["settings"]["groups"].as_array().unwrap().len(), 2);
        assert_eq!(after["reports"], original["reports"]);
    }

    #[test]
    fn group_tree_page_lists_roots_and_children_and_search_keeps_parent_context() {
        let root = tempfile::tempdir().unwrap();
        let store = Storage::new(root.path().join("store")).unwrap();
        store.save(serde_json::json!({"reports":[],"settings":{
            "orgNames":["A","B","C","D"],"catalog":[],
            "groups":[{"id":"engine","name":"Двигатель","works":[]},
                {"id":"head","name":"ГБЦ","works":[]},{"id":"testing","name":"Опрессовка","works":["A"]}],
            "workGroupTreeParents":[{"child":"head","parent":"engine"},{"child":"testing","parent":"head"}]
        }})).unwrap();
        let revision = store.revision().unwrap();
        store.management_index_settings(revision, &ManagementJob::new()).unwrap();
        let roots = store.unified_management_page(revision, "group_tree", "", None, 0, 50).unwrap();
        assert_eq!(roots.total, 1);
        assert_eq!(roots.rows[0].id.as_deref(), Some("engine"));
        assert_eq!(roots.rows[0].children, Some(1));
        let handle = roots.rows[0].handle.as_deref().unwrap().parse().unwrap();
        let nested = store.unified_management_page(revision, "group_tree", "", Some(handle), 0, 50).unwrap();
        assert_eq!(nested.total, 1);
        assert_eq!(nested.rows[0].id.as_deref(), Some("head"));
        let search = store.unified_management_page(revision, "group_tree", "опресс", None, 0, 50).unwrap();
        assert_eq!(search.rows[0].id.as_deref(), Some("testing"));
        assert_eq!(search.rows[0].group, nested.rows[0].handle);
    }

    #[test]
    fn tree_group_file_rejects_cycles_bad_links_and_excessive_depth() {
        for links in [
            r#"[{"child":"0","parent":"0"}]"#,
            r#"[{"child":"0","parent":"1"},{"child":"1","parent":"0"}]"#,
            r#"[{"child":"0","parent":"9"}]"#,
            r#"[{"child":"0","parent":"1"},{"child":"0","parent":"2"}]"#,
        ] {
            assert!(review(3, links).is_err(), "{links}");
        }
        let parents: Vec<_> = (0..64).map(|i| serde_json::json!({
            "child":i.to_string(),"parent":(i+1).to_string()
        })).collect();
        assert!(review(64, &serde_json::json!(parents[..63]).to_string()).is_ok());
        assert!(review(65, &serde_json::json!(parents).to_string()).is_err());
    }
}
