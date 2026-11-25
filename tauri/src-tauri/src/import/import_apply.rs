//! Prepared import publication. No Tauri commands and no full-history load.
use super::{err, import_source::source_plan, read_model, ensure_import_active, Storage};
use analiz_import_core::{ImportMode, PreparedImport, QualityCounts};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard, TryLockError,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ImportCounts {
    pub new_reports: u64,
    pub replacements: u64,
    pub cross_type_overlaps: u64,
}
#[derive(Debug)]
pub struct ImportPreview {
    operation: String,
    database: std::path::PathBuf,
    revision: i64,
    counts: ImportCounts,
    generation: String,
    quality: QualityCounts,
    plan_hash: String,
}
impl ImportPreview {
    pub fn quality(&self) -> &QualityCounts {
        &self.quality
    }
    pub fn revision(&self) -> i64 {
        self.revision
    }
    pub fn counts(&self) -> &ImportCounts {
        &self.counts
    }
    // A deterministic review identity for native UI revalidation. A second
    // preparation must compare both the live DB generation and the conflict plan.
    pub fn review_identity(&self) -> (&str, &str) {
        (&self.generation, &self.plan_hash)
    }
}
/// Folder replacement and quality notices require independent explicit acknowledgments.
#[derive(Clone, Copy, Default)]
pub struct ImportConfirmation {
    pub replace_exact: bool,
    pub accept_cross_type: bool,
    pub replace_source: bool,
    pub accept_quality: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ImportReceipt {
    pub operation_id: String,
    pub revision: i64,
    pub reports: u64,
    pub rows: u64,
    pub replaced: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct ImportProgress {
    pub phase: &'static str,
    pub bytes: u64,
    pub reports: u64,
    pub elapsed_millis: u128,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum JobState {
    Open,
    Cancelled,
    Committed,
}
/// Share by reference/Arc with a worker. One job per apply attempt.
/// Cancellation and commit serialize on state; a successful commit always wins over late cancel.
pub struct ImportJob {
    state: Mutex<JobState>,
    cancelled: Arc<AtomicBool>,
    check_source: Option<Box<dyn Fn() -> Result<(), String> + Send + Sync>>,
}
impl Default for ImportJob {
    fn default() -> Self {
        Self::new()
    }
}
impl ImportJob {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(JobState::Open),
            cancelled: Arc::new(AtomicBool::new(false)),
            check_source: None,
        }
    }
    pub(super) fn for_checked_source(
        check_source: impl Fn() -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        Self { check_source: Some(Box::new(check_source)), ..Self::new() }
    }
    pub fn cancellation(&self) -> &AtomicBool {
        &self.cancelled
    }
    pub fn cancellation_handle(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }
    /// False means commit already succeeded; true means cancellation is accepted.
    pub fn cancel(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if *state == JobState::Committed {
            return false;
        }
        *state = JobState::Cancelled;
        self.cancelled.store(true, Ordering::Release);
        true
    }
    fn completed(&self) {
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = JobState::Committed;
    }
    fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            Err("Import cancelled before commit.".into())
        } else {
            Ok(())
        }
    }
}
pub(super) fn initialize(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS import_receipts(operation_id TEXT PRIMARY KEY, revision INTEGER NOT NULL, reports INTEGER NOT NULL, rows INTEGER NOT NULL, replaced INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS import_generation(singleton INTEGER PRIMARY KEY CHECK(singleton=1), generation TEXT NOT NULL);
    INSERT OR IGNORE INTO import_generation VALUES(1,lower(hex(randomblob(32))));
    CREATE TABLE IF NOT EXISTS report_source_ownership(report_id TEXT PRIMARY KEY, source_name TEXT NOT NULL, folder TEXT NOT NULL, mode TEXT NOT NULL);
    CREATE TRIGGER IF NOT EXISTS source_ownership_insert AFTER INSERT ON reports BEGIN DELETE FROM report_source_ownership WHERE report_id=NEW.id; END;
    CREATE TRIGGER IF NOT EXISTS source_ownership_update AFTER UPDATE ON reports BEGIN DELETE FROM report_source_ownership WHERE report_id IN (OLD.id,NEW.id); END;
    CREATE TRIGGER IF NOT EXISTS source_ownership_delete AFTER DELETE ON reports BEGIN DELETE FROM report_source_ownership WHERE report_id=OLD.id; END;
    -- Explicit links are needed because only the first wide day stores full sourceIssues.
    -- Protect against manual exact replacement, legacy bulk saves and direct SQL deletes.
    CREATE TABLE IF NOT EXISTS report_source_diagnostic_links(report_id TEXT PRIMARY KEY, anchor_id TEXT NOT NULL);
    CREATE INDEX IF NOT EXISTS diagnostic_anchor_lookup ON report_source_diagnostic_links(anchor_id);
    CREATE TRIGGER IF NOT EXISTS diagnostic_anchor_delete BEFORE DELETE ON reports
      WHEN EXISTS(SELECT 1 FROM report_source_diagnostic_links l WHERE l.anchor_id=OLD.id AND l.report_id<>OLD.id)
      BEGIN SELECT RAISE(ABORT,'Shared source diagnostics anchor has surviving reports; replace the whole source'); END;
    CREATE TRIGGER IF NOT EXISTS diagnostic_protected_update BEFORE UPDATE OF id,json ON reports
      WHEN (NEW.id IS NOT OLD.id OR NEW.json IS NOT OLD.json)
       AND EXISTS(SELECT 1 FROM report_source_diagnostic_links l WHERE l.report_id=OLD.id)
      BEGIN SELECT RAISE(ABORT,'Shared source diagnostics require a prepared replacement'); END;
    CREATE TRIGGER IF NOT EXISTS diagnostic_link_delete AFTER DELETE ON reports
      BEGIN DELETE FROM report_source_diagnostic_links WHERE report_id=OLD.id; END;").map_err(err)
}
fn receipt(conn: &Connection, operation: &str) -> Result<Option<ImportReceipt>, String> {
    if operation.len() != 64 || !operation.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid import operation ID.".into());
    }
    conn.query_row(
        "SELECT revision,reports,rows,replaced FROM import_receipts WHERE operation_id=?1",
        [operation],
        |r| {
            Ok(ImportReceipt {
                operation_id: operation.into(),
                revision: r.get(0)?,
                reports: r.get(1)?,
                rows: r.get(2)?,
                replaced: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(err)
}
fn lock_import_gate<'a>(gate: &'a Mutex<()>, cancel: &AtomicBool) -> Result<MutexGuard<'a, ()>, String> {
    loop {
        ensure_import_active(cancel)?;
        match gate.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(e)) => return Err(err(e)),
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}
/// The SQLite callback owns its cancellation flag. A scoped monitor bridges the
/// existing borrowed-token API without unsafe pointers or a 'static borrow.
fn with_sql_cancel<T>(
    conn: &Connection,
    cancel: &AtomicBool,
    work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    struct Hooks<'a> {
        conn: &'a Connection,
        done: Arc<AtomicBool>,
    }
    impl Drop for Hooks<'_> {
        fn drop(&mut self) {
            self.conn.progress_handler(0, None::<fn() -> bool>);
            self.done.store(true, Ordering::Release);
        }
    }
    std::thread::scope(|scope| {
        let interrupted = Arc::new(AtomicBool::new(cancel.load(Ordering::Acquire)));
        let done = Arc::new(AtomicBool::new(false));
        let observed = interrupted.clone();
        let finished = done.clone();
        scope.spawn(move || {
            while !finished.load(Ordering::Acquire) {
                if cancel.load(Ordering::Acquire) {
                    observed.store(true, Ordering::Release);
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        conn.progress_handler(1000, Some(move || interrupted.load(Ordering::Acquire)));
        let _hooks = Hooks { conn, done };
        work()
    })
}
fn generation(conn: &Connection) -> Result<String, String> {
    conn.query_row(
        "SELECT generation FROM import_generation WHERE singleton=1",
        [],
        |r| r.get(0),
    )
    .map_err(err)
}
/// Scan in rowid chunks rather than one uninterruptible aggregate over history.
fn conflicts(
    conn: &Connection,
    prepared: &PreparedImport<'_>,
    cancel: &AtomicBool,
) -> Result<(ImportCounts, String), String> {
    let mut counts = ImportCounts::default();
    let plan_hash = source_plan(conn, prepared, cancel)?;
    if prepared.mode() == ImportMode::Folder {
        counts.replacements = conn
            .query_row("SELECT COUNT(*) FROM temp.import_source_plan", [], |r| {
                r.get(0)
            })
            .map_err(err)?;
    }
    let mut after = 0;
    while let Some(report) = prepared.next_report(after).map_err(err)? {
        ensure_import_active(cancel)?;
        let mut exact = 0;
        let mut cursor = i64::MIN;
        let mut first = true;
        loop {
            ensure_import_active(cancel)?;
            let mut stmt = conn.prepare("SELECT r.rowid, CASE WHEN octet_length(r.id)>65536 OR octet_length(r.report_type)>65536 OR octet_length(r.period_start)>65536 OR octet_length(r.period_end)>65536 OR octet_length(r.source_name)>65536 THEN 1 ELSE 0 END, CASE WHEN octet_length(r.report_type)<=65536 THEN r.report_type END, CASE WHEN octet_length(r.period_start)<=65536 THEN r.period_start END, CASE WHEN octet_length(r.period_end)<=65536 THEN r.period_end END, CASE WHEN octet_length(r.id)<=65536 THEN EXISTS(SELECT 1 FROM temp.import_source_plan p WHERE p.report_id=r.id) ELSE 0 END FROM reports r WHERE r.rowid>=?1 AND (?2 OR r.rowid<>?1) ORDER BY r.rowid LIMIT 100").map_err(err)?;
            let mut rows = stmt.query(params![cursor, first]).map_err(err)?;
            first = false;
            let mut seen = 0;
            while let Some(row) = rows.next().map_err(err)? {
                cursor = row.get(0).map_err(err)?;
                seen += 1;
                if row.get::<_, bool>(1).map_err(err)? {
                    return Err("Report metadata exceeds 64 KiB.".into());
                }
                let owned = row.get::<_, bool>(5).map_err(err)?;
                let kind: String = row.get(2).map_err(err)?;
                let start: String = row.get(3).map_err(err)?;
                let end: String = row.get(4).map_err(err)?;
                if owned {
                    if kind == report.kind && start == report.start && end == report.end {
                        exact += 1;
                    }
                    continue;
                }
                if start > report.end || end < report.start {
                    continue;
                }
                if kind != report.kind {
                    counts.cross_type_overlaps += 1;
                    continue;
                }
                if prepared.mode() == ImportMode::Folder {
                    return Err(
                        "Source replacement cannot replace another source's overlapping period."
                            .into(),
                    );
                }
                if start != report.start || end != report.end {
                    return Err(
                        "Overlapping same-type non-identical periods cannot be replaced.".into(),
                    );
                }
                exact += 1;
            }
            if seen < 100 {
                break;
            }
        }
        if exact == 0 {
            counts.new_reports += 1;
        }
        if prepared.mode() == ImportMode::Manual {
            counts.replacements += exact;
        }
        after = report.ordinal;
    }
    ensure_import_active(cancel)?;
    Ok((counts, plan_hash))
}
impl Storage {
    // Import calls must not trigger the legacy full-history migration/load path.
    // Normal startup (or revision()) initializes first; import only opens that DB.
    fn import_connection(&self) -> Result<Connection, String> {
        let conn = Connection::open_with_flags(
            self.db_path(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(|e| format!("Initialize storage before importing: {e}"))?;
        conn.busy_timeout(Duration::from_millis(10)).map_err(err)?;
        conn.execute_batch(
            "PRAGMA synchronous=FULL; PRAGMA cache_size=-2048; PRAGMA temp_store=FILE;",
        )
        .map_err(err)?;
        let ready: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM kv WHERE name='migration_v1')",
                [],
                |r| r.get(0),
            )
            .map_err(err)?;
        if !ready {
            return Err("Initialize storage before importing.".into());
        }
        read_model::revision(&conn)?;
        conn.prepare("SELECT operation_id FROM import_receipts LIMIT 0")
            .map_err(err)?;
        Ok(conn)
    }
    /// Required after restoring/replacing a database, before exposing it to import workers.
    /// Restore coordinator must hold exclusive lifecycle access around restore + this call.
    pub fn invalidate_import_previews_after_restore(&self) -> Result<(), String> {
        let _guard = self.gate.lock().map_err(err)?;
        let conn = self.import_connection()?;
        conn.execute(
            "UPDATE import_generation SET generation=lower(hex(randomblob(32))) WHERE singleton=1",
            [],
        )
        .map_err(err)?;
        Ok(())
    }
    pub fn committed_import(&self, operation: &str) -> Result<Option<ImportReceipt>, String> {
        let _guard = self.gate.lock().map_err(err)?;
        receipt(&self.import_connection()?, operation)
    }
    /// Storage initialization/migration should precede user preview; snapshot itself is read-only.
    pub fn preview_import(&self, prepared: &PreparedImport<'_>) -> Result<ImportPreview, String> {
        self.preview_import_with_cancel(prepared, &AtomicBool::new(false))
    }
    pub fn preview_import_with_cancel(
        &self,
        prepared: &PreparedImport<'_>,
        cancel: &AtomicBool,
    ) -> Result<ImportPreview, String> {
        prepared.validate(cancel).map_err(err)?;
        let _guard = lock_import_gate(&self.gate, cancel)?;
        let conn = self.import_connection()?;
        with_sql_cancel(&conn, cancel, || {
            let tx = rusqlite::Transaction::new_unchecked(&conn, TransactionBehavior::Deferred)
                .map_err(err)?;
            let revision = read_model::revision(&tx)?;
            let generation = generation(&tx)?;
            let (counts, plan_hash) = conflicts(&tx, prepared, cancel)?;
            ensure_import_active(cancel)?;
            Ok(ImportPreview {
                operation: prepared.operation_id().into(),
                database: std::fs::canonicalize(self.db_path()).map_err(err)?,
                revision,
                generation,
                plan_hash,
                quality: prepared.quality().clone(),
                counts,
            })
        })
    }
    pub fn apply_import(
        &self,
        prepared: &PreparedImport<'_>,
        preview: &ImportPreview,
        confirmation: ImportConfirmation,
        job: &ImportJob,
        progress: impl FnMut(ImportProgress),
    ) -> Result<ImportReceipt, String> {
        // A completed receipt is still authoritative for a cancelled retry.
        let _guard = match lock_import_gate(&self.gate, &job.cancelled) {
            Ok(g) => g,
            Err(e) => {
                if let Some(done) = receipt(&self.import_connection()?, prepared.operation_id())? {
                    job.completed();
                    return Ok(done);
                }
                return Err(e);
            }
        };
        let conn = self.import_connection()?;
        // Durable receipt wins over stale revision, missing stage/source, and a late cancel.
        if let Some(done) = receipt(&conn, prepared.operation_id())? {
            job.completed();
            return Ok(done);
        }
        if preview.operation != prepared.operation_id() {
            return Err("Preview belongs to a different prepared import.".into());
        }
        job.check()?;
        prepared.validate(&job.cancelled).map_err(err)?;
        let catalog=match self.unified_prepare_import_catalog(&conn,prepared,preview.revision,job.cancellation_handle()) {
            Ok(catalog)=>catalog,
            Err(error)=>{
                // A concurrent caller may have completed this exact operation
                // after our first probe while catalog settings were staged.
                if matches!(std::fs::canonicalize(self.db_path()),Ok(database) if database==preview.database) {
                    if let Ok(Some(done))=receipt(&conn,prepared.operation_id()) {
                        job.completed();
                        return Ok(done);
                    }
                }
                return Err(error);
            },
        };
        let result=(|| {
            if let Some(settings)=&catalog {settings.attach(&conn)?;}
            with_sql_cancel(&conn,&job.cancelled,||apply_in_connection_inner(&conn,prepared,preview,confirmation,job,progress,catalog.as_ref()))
        })();
        // Release the attached private database before Stage::drop removes it
        // (Windows does not permit unlinking an open SQLite file).
        drop(conn);
        drop(catalog);
        result
    }
}
/// Shared actual publication path, allowing real constrained SQLite connections in unit tests.
#[cfg(test)]
fn apply_in_connection(
    conn: &mut Connection,
    prepared: &PreparedImport<'_>,
    preview: &ImportPreview,
    confirmation: ImportConfirmation,
    job: &ImportJob,
    progress: impl FnMut(ImportProgress),
) -> Result<ImportReceipt, String> {
    with_sql_cancel(conn, &job.cancelled, || {
        apply_in_connection_inner(conn, prepared, preview, confirmation, job, progress,None)
    })
}
fn apply_in_connection_inner(
    conn: &Connection,
    prepared: &PreparedImport<'_>,
    preview: &ImportPreview,
    confirmation: ImportConfirmation,
    job: &ImportJob,
    mut progress: impl FnMut(ImportProgress),
    catalog:Option<&super::management::PreparedCatalogSettings>,
) -> Result<ImportReceipt, String> {
    if std::fs::canonicalize(conn.path().ok_or("Import requires a file database")?).map_err(err)?
        != preview.database
    {
        return Err("Preview belongs to a different database.".into());
    }
    let started = Instant::now();
    let waiting = Instant::now();
    let tx = loop {
        job.check()?;
        match rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate) {
            Ok(tx) => break tx,
            Err(e)
                if matches!(
                    e.sqlite_error_code(),
                    Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
                ) && waiting.elapsed() < Duration::from_secs(5) =>
            {
                continue
            }
            Err(e) => return Err(err(e)),
        }
    };
    if let Some(done) = receipt(&tx, prepared.operation_id())? {
        job.completed();
        return Ok(done);
    }
    job.check()?;
    if preview.operation != prepared.operation_id() {
        return Err("Preview belongs to a different prepared import.".into());
    }
    if generation(&tx)? != preview.generation {
        return Err("Database generation changed; preview again.".into());
    }
    read_model::check_revision(&tx, preview.revision)?;
    let (counts, plan_hash) = conflicts(&tx, prepared, &job.cancelled)?;
    if counts != preview.counts
        || plan_hash != preview.plan_hash
        || prepared.quality() != &preview.quality
    {
        return Err("Import conflicts changed; preview again.".into());
    }
    if prepared.quality().requires_acknowledgment() && !confirmation.accept_quality {
        return Err("Quality notices need explicit acknowledgment.".into());
    }
    if prepared.mode() == ImportMode::Manual && confirmation.replace_source {
        return Err("Source replacement requires an explicitly folder-owned artifact.".into());
    }
    if prepared.mode() == ImportMode::Folder && confirmation.replace_exact {
        return Err("Manual exact-period replacement is separate from source replacement.".into());
    }
    if prepared.mode() == ImportMode::Folder
        && counts.replacements != 0
        && !confirmation.replace_source
    {
        return Err("Whole-source replacement needs explicit confirmation.".into());
    }
    if prepared.mode() == ImportMode::Manual
        && counts.replacements != 0
        && !confirmation.replace_exact
    {
        return Err("Exact-period replacement needs explicit confirmation.".into());
    }
    if counts.cross_type_overlaps != 0 && !confirmation.accept_cross_type {
        return Err(
            "Cross-type overlap needs explicit acceptance; totals are not distributed across days."
                .into(),
        );
    }
    // Wide documents share one diagnostic anchor. Deletion triggers forbid an
    // anchor disappearing while any dependent report survives. A whole-source
    // replacement must therefore remove its non-anchor days first.
    let diagnostic_anchor = prepared.next_report(0).map_err(err)?
        .and_then(|first| first.provenance.map(|_| first.id));
    let deletion_plan=if prepared.mode()==ImportMode::Folder {
        Some("import_source_plan")
    } else if confirmation.replace_exact {
        // Plan ALL old exact matches before removing an anchor. Incoming report
        // order must not make a complete manual replacement look like a partial
        // source deletion to the diagnostic-link guard.
        tx.execute_batch("CREATE TEMP TABLE IF NOT EXISTS import_exact_plan(report_id TEXT PRIMARY KEY) WITHOUT ROWID;DELETE FROM temp.import_exact_plan;").map_err(err)?;
        let mut after=0;
        while let Some(report)=prepared.next_report(after).map_err(err)? {
            job.check()?;
            tx.execute("INSERT OR IGNORE INTO temp.import_exact_plan SELECT id FROM reports WHERE report_type=?1 AND period_start=?2 AND period_end=?3",params![report.kind,report.start,report.end]).map_err(err)?;
            after=report.ordinal;
        }
        Some("import_exact_plan")
    } else {None};
    // Delete one planned old report at a time, non-anchors before anchors. The
    // existing trigger still rejects an anchor with any UNPLANNED survivor and
    // the transaction restores all preceding deletions on that error or cancel.
    if let Some(plan)=deletion_plan {
        loop {
            job.check()?;
            let target: Option<String> = tx
                .query_row(
                    &format!("SELECT p.report_id FROM temp.{plan} p LEFT JOIN report_source_diagnostic_links l ON l.report_id=p.report_id ORDER BY CASE WHEN l.anchor_id=p.report_id THEN 1 ELSE 0 END, p.report_id LIMIT 1"),
                    [],
                    |r| r.get(0),
                )
                .optional()
                .map_err(err)?;
            let Some(target) = target else {
                break;
            };
            tx.execute("DELETE FROM reports WHERE id=?1", [&target])
                .map_err(err)?;
            tx.execute(
                &format!("DELETE FROM temp.{plan} WHERE report_id=?1"),
                [&target],
            )
            .map_err(err)?;
        }
    }
    let mut after = 0;
    let mut bytes = 0;
    let mut reports = 0;
    let mut all_rows = 0;
    while let Some(report) = prepared.next_report(after).map_err(err)? {
        job.check()?;
        if (report.rows == 0 && !report.is_explicit_zero())
            || report.bytes == 0
            || report.bytes > i32::MAX as u64
        {
            return Err("Invalid or empty prepared report.".into());
        }
        tx.execute("INSERT INTO reports(id,report_type,period_start,period_end,source_name,json) VALUES(?1,?2,?3,?4,?5,zeroblob(?6))", params![report.id, report.kind, report.start, report.end, prepared.source_name(), report.bytes]).map_err(err)?;
        let rowid = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO report_source_ownership VALUES(?1,?2,?3,?4)",
            params![
                report.id,
                prepared.source_name(),
                prepared.source_folder(),
                if prepared.mode() == ImportMode::Folder {
                    "folder"
                } else {
                    "manual"
                }
            ],
        )
        .map_err(err)?;
        if report.provenance.is_some() {
            let anchor = diagnostic_anchor.as_ref().ok_or(
                "Mixed prepared wide reports cannot safely publish shared source diagnostics."
            )?;
            tx.execute(
                "INSERT INTO report_source_diagnostic_links(report_id,anchor_id) VALUES(?1,?2)",
                params![report.id, anchor],
            ).map_err(err)?;
        }
        {
            let mut blob = tx
                .blob_open("main", "reports", "json", rowid, false)
                .map_err(err)?;
            let mut file = prepared.open_document(&report).map_err(err)?;
            let mut buf = [0u8; 65536];
            let mut copied = 0;
            let mut hash = Sha256::new();
            loop {
                job.check()?;
                let n = file.read(&mut buf).map_err(err)?;
                if n == 0 {
                    break;
                }
                if copied + n as u64 > report.bytes {
                    return Err("Prepared document grew during copy.".into());
                }
                blob.write_all(&buf[..n]).map_err(err)?;
                hash.update(&buf[..n]);
                copied += n as u64;
                bytes += n as u64;
                progress(ImportProgress {
                    phase: "copy",
                    bytes,
                    reports,
                    elapsed_millis: started.elapsed().as_millis(),
                });
            }
            if copied != report.bytes || format!("{:x}", hash.finalize()) != report.sha256 {
                return Err("Prepared document changed or incomplete during copy.".into());
            }
            blob.close().map_err(err)?;
        }
        let mut cursor = 0;
        let mut rows = 0;
        let mut insert = tx
            .prepare("INSERT INTO report_rows VALUES(?1,?2,?3,?4,?5,?6)")
            .map_err(err)?;
        loop {
            job.check()?;
            let page = prepared.rows(&report, cursor).map_err(err)?;
            if page.is_empty() {
                break;
            }
            for row in page {
                if row.row_index != rows + 1
                    || !row.count.is_finite()
                    || row.amount.is_some_and(|amount| !amount.is_finite())
                    || row.organization.len() > 65536
                    || row.work.len() > 65536
                {
                    return Err("Invalid prepared normalized row.".into());
                }
                insert
                    .execute(params![
                        report.id,
                        rows,
                        row.organization,
                        row.work,
                        row.count,
                        row.amount
                    ])
                    .map_err(err)?;
                rows += 1;
                cursor = row.row_index;
            }
            if rows % 100 == 0 {
                progress(ImportProgress {
                    phase: "index",
                    bytes,
                    reports,
                    elapsed_millis: started.elapsed().as_millis(),
                });
            }
        }
        if rows != report.rows {
            return Err("Prepared normalized row count mismatch.".into());
        }
        drop(insert);
        tx.execute(
            "INSERT INTO report_row_index VALUES(?1,?2)",
            params![report.id, rows],
        )
        .map_err(err)?;
        all_rows += rows;
        reports += 1;
        after = report.ordinal;
    }
    if reports != prepared.report_count() || all_rows != prepared.row_count() {
        return Err("Prepared report manifest mismatch.".into());
    }
    if let Some(settings)=catalog {settings.publish(&tx,preview.revision,&job.cancelled)?;}
    // Detect source/index changes during I/O; external filesystem changes after this
    // observation cannot be synchronized with SQLite and are not claimed atomic.
    prepared
        .validate_source_and_database(&job.cancelled)
        .map_err(err)?;
    let committed = ImportReceipt {
        operation_id: prepared.operation_id().into(),
        revision: read_model::revision(&tx)?,
        reports,
        rows: all_rows,
        replaced: counts.replacements,
    };
    tx.execute(
        "INSERT INTO import_receipts VALUES(?1,?2,?3,?4,?5)",
        params![
            committed.operation_id,
            committed.revision,
            reports,
            all_rows,
            counts.replacements
        ],
    )
    .map_err(err)?;
    progress(ImportProgress {
        phase: "before_commit",
        bytes,
        reports,
        elapsed_millis: started.elapsed().as_millis(),
    });
    if let Some(check_source) = &job.check_source {
        check_source()?;
    }
    let mut state = job.state.lock().map_err(err)?;
    match *state {
        JobState::Cancelled => return Err("Import cancelled before commit.".into()),
        JobState::Committed => return Err("Import job was already used.".into()),
        JobState::Open => {}
    }
    tx.commit().map_err(err)?;
    *state = JobState::Committed;
    // No fallible cleanup or callback after commit: receipt is the only outcome.
    Ok(committed)
}

#[cfg(test)]
#[path = "import_apply_fault_tests.rs"]
mod fault_tests;
