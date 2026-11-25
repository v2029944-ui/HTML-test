//! File-based settings transfers. Reports never enter these documents or IPC.
use super::*;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
struct GroupsDocument {
    format: String,
    version: u32,
    groups: Vec<GroupDocument>,
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
    fn replace_groups(&mut self, mut groups: Vec<GroupDocument>, actual: &str) -> Result<(), String> {
        let job = self.job.clone();
        self.mutate(|c| {
            let parent=settings(c)?.1;
            c.execute("WITH RECURSIVE descendants(handle) AS (SELECT handle FROM management_nodes WHERE parent=?1 UNION ALL SELECT n.handle FROM management_nodes n JOIN descendants d ON n.parent=d.handle) DELETE FROM management_nodes WHERE handle IN descendants",[parent]).map_err(err)?;
            for (ordinal,g) in groups.iter_mut().enumerate() {
                if g.id.trim().is_empty() {g.id=format!("import-{}-{}", &actual[..16],ordinal);}
                g.name=g.name.trim().into();
                let serialized=serde_json::to_vec(g).map_err(err)?;
                let group=tree::parse(c,std::io::Cursor::new(serialized),&job)?;
                c.execute("UPDATE management_nodes SET parent=?1,ordinal=?2,field=NULL WHERE handle=?3",params![parent,ordinal as i64,group]).map_err(err)?;
            }
            Ok(())
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
fn read_groups(source: &Path) -> Result<(GroupsDocument, String), String> {
    let file = fs::File::open(source).map_err(err)?;
    if !file.metadata().map_err(err)?.is_file() {
        return Err("Выберите обычный файл JSON.".into());
    }
    let mut bytes = Vec::new();
    file.take(40 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.len() > 40 * 1024 * 1024 {
        return Err("Файл JSON больше 40 МБ.".into());
    }
    let mut doc: GroupsDocument = serde_json::from_slice(&bytes).map_err(err)?;
    if doc.format != "work-groups" || doc.version != 1 || doc.groups.len() > 10000 {
        return Err("Неверный формат групп или более 10000 групп.".into());
    }
    let mut names = HashSet::new();
    let mut works = HashSet::new();
    let mut count = 0;
    for group in &mut doc.groups {
        if group.name.trim().is_empty()
            || group.name.len() > 1024
            || group.id.len() > 128
            || !names.insert(unified_normalized(&group.name))
            || group.works.len() > 5000
        {
            return Err("Некорректная группа в JSON.".into());
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
            if count > 100000 || work.trim().is_empty() || work.len() > 4096 || !works.insert(work)
            {
                return Err(
                    "Работа повторяется в группах, пуста или превышен лимит 100000 работ.".into(),
                );
            }
        }
    }
    Ok((doc, format!("{:x}", Sha256::digest(&bytes))))
}
impl Storage {
    pub fn unified_groups_preview(
        &self,
        revision: i64,
        source: &Path,
    ) -> Result<UnifiedSettingsReview, String> {
        let (doc, hash) = read_groups(source)?;
        let c = self.management_connect()?;
        checked(&c, revision)?;
        Ok(UnifiedSettingsReview {
            hash,
            groups: doc.groups.len() as u64,
            works: doc.groups.iter().map(|g| g.works.len() as u64).sum(),
            catalog: 0,
        })
    }
    pub fn unified_groups_import(
        &self,
        revision: i64,
        source: &Path,
        hash: &str,
        confirmed: bool,
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
        editor.replace_groups(doc.groups, &actual)?;
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
            out.write_all(b"{\"format\":\"work-groups\",\"version\":1,\"groups\":[")
                .map_err(err)?;
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
            out.write_all(b"]}\n").map_err(err)?;
            out.flush().map_err(err)?;
            out.get_ref().sync_all().map_err(err)
        })();
        if result.is_err() {
            let _ = fs::remove_file(destination);
        }
        result
    }
}
