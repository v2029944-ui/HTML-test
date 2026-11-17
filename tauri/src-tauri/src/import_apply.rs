//! Prepared import publication. No Tauri commands and no full-history load.
use super::{err, read_model, Storage};
use analiz_import_core::PreparedImport;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Instant,
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
}
impl ImportPreview {
    pub fn revision(&self) -> i64 {
        self.revision
    }
    pub fn counts(&self) -> &ImportCounts {
        &self.counts
    }
}
/// Only manual exact-period replacement exists. No folder/source replacement mode.
#[derive(Clone, Copy, Default)]
pub struct ImportConfirmation {
    pub replace_exact: bool,
    pub accept_cross_type: bool,
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
    cancelled: AtomicBool,
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
            cancelled: AtomicBool::new(false),
        }
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
    conn.execute_batch("CREATE TABLE IF NOT EXISTS import_receipts(operation_id TEXT PRIMARY KEY, revision INTEGER NOT NULL, reports INTEGER NOT NULL, rows INTEGER NOT NULL, replaced INTEGER NOT NULL);").map_err(err)
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
fn conflicts(conn: &Connection, prepared: &PreparedImport<'_>) -> Result<ImportCounts, String> {
    read_model::check_metadata_bounds(conn)?;
    let mut counts = ImportCounts::default();
    let mut after = 0;
    while let Some(report) = prepared.next_report(after).map_err(err)? {
        let (exact, overlap, cross): (u64,u64,u64) = conn.query_row(
            "SELECT COALESCE(SUM(report_type=?1 AND period_start=?2 AND period_end=?3),0), COALESCE(SUM(report_type=?1 AND NOT(period_start=?2 AND period_end=?3)),0), COALESCE(SUM(report_type<>?1),0) FROM reports WHERE period_start<=?3 AND period_end>=?2",
            params![report.kind, report.start, report.end], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(err)?;
        if overlap != 0 {
            return Err("Overlapping same-type non-identical periods cannot be replaced.".into());
        }
        if exact == 0 {
            counts.new_reports += 1;
        }
        counts.replacements += exact;
        counts.cross_type_overlaps += cross;
        after = report.ordinal;
    }
    Ok(counts)
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
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(err)?;
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
        let _guard = self.gate.lock().map_err(err)?;
        let mut conn = self.import_connection()?;
        let tx = conn.transaction().map_err(err)?;
        let revision = read_model::revision(&tx)?;
        let counts = conflicts(&tx, prepared)?;
        Ok(ImportPreview {
            operation: prepared.operation_id().into(),
            database: std::fs::canonicalize(self.db_path()).map_err(err)?,
            revision,
            counts,
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
        let _guard = self.gate.lock().map_err(err)?;
        let mut conn = self.import_connection()?;
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
        apply_in_connection(&mut conn, prepared, preview, confirmation, job, progress)
    }
}
/// Shared actual publication path, allowing real constrained SQLite connections in unit tests.
fn apply_in_connection(
    conn: &mut Connection,
    prepared: &PreparedImport<'_>,
    preview: &ImportPreview,
    confirmation: ImportConfirmation,
    job: &ImportJob,
    mut progress: impl FnMut(ImportProgress),
) -> Result<ImportReceipt, String> {
    if std::fs::canonicalize(conn.path().ok_or("Import requires a file database")?).map_err(err)?
        != preview.database
    {
        return Err("Preview belongs to a different database.".into());
    }
    let started = Instant::now();
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)?;
    if let Some(done) = receipt(&tx, prepared.operation_id())? {
        job.completed();
        return Ok(done);
    }
    job.check()?;
    if preview.operation != prepared.operation_id() {
        return Err("Preview belongs to a different prepared import.".into());
    }
    read_model::check_revision(&tx, preview.revision)?;
    let counts = conflicts(&tx, prepared)?;
    if counts != preview.counts {
        return Err("Import conflicts changed; preview again.".into());
    }
    if counts.replacements != 0 && !confirmation.replace_exact {
        return Err("Exact-period replacement needs explicit confirmation.".into());
    }
    if counts.cross_type_overlaps != 0 && !confirmation.accept_cross_type {
        return Err(
            "Cross-type overlap needs explicit acceptance; totals are not distributed across days."
                .into(),
        );
    }
    let mut after = 0;
    let mut bytes = 0;
    let mut reports = 0;
    let mut all_rows = 0;
    while let Some(report) = prepared.next_report(after).map_err(err)? {
        job.check()?;
        if report.rows == 0 || report.bytes == 0 || report.bytes > i32::MAX as u64 {
            return Err("Invalid or empty prepared report.".into());
        }
        if confirmation.replace_exact {
            tx.execute(
                "DELETE FROM reports WHERE report_type=?1 AND period_start=?2 AND period_end=?3",
                params![report.kind, report.start, report.end],
            )
            .map_err(err)?;
        }
        tx.execute("INSERT INTO reports(id,report_type,period_start,period_end,source_name,json) VALUES(?1,?2,?3,?4,?5,zeroblob(?6))", params![report.id, report.kind, report.start, report.end, prepared.source_name(), report.bytes]).map_err(err)?;
        let rowid = tx.last_insert_rowid();
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
                    || !row.amount.is_finite()
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
