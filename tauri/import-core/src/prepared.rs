//! Owned, complete disk artifacts. No public constructor from caller file lists.
use crate::{
    check_cancel, fail, fingerprint, ReportInfo, ReportIssue, ReportOrganization, ReportRow,
    ReportTotal, Result, Stage,
};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Write},
    path::Path,
    sync::atomic::AtomicBool,
};

#[derive(Clone, Copy, Debug)]
pub struct PrepareProgress {
    pub phase: &'static str,
    pub bytes: u64,
    pub reports: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportMode {
    #[default]
    Manual,
    Folder,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct QualityCounts {
    pub reports: u64,
    pub rows: u64,
    pub issues: u64,
    pub invalid: u64,
    pub skipped: u64,
    pub mismatches: u64,
    pub other_notices: u64,
    pub controls: u64,
    pub mismatch_reports: u64,
}
impl QualityCounts {
    pub fn requires_acknowledgment(&self) -> bool {
        self.issues != 0 || self.mismatch_reports != 0
    }
}
/// Complete artifact lifetime also holds the StageArea recovery lock.
pub struct PreparedImport<'a> {
    stage: Stage<'a>,
    operation: String,
    database_hash: String,
    source_name: String,
    source_folder: String,
    reports: u64,
    rows: u64,
    mode: ImportMode,
    quality: QualityCounts,
    database_stamp: (u64, std::time::SystemTime),
}
#[derive(Debug)]
pub struct PreparedReport {
    pub ordinal: u64,
    pub id: String,
    pub kind: String,
    pub start: String,
    pub end: String,
    pub rows: u64,
    pub bytes: u64,
    pub sha256: String,
    pub info: ReportInfo,
    pub quality: QualityCounts,
    local_id: String,
}
/// Raw JSON strings share D1's 64 KiB cap; bytes and hashing never buffer a document.
struct Document<'a> {
    output: BufWriter<File>,
    hash: Sha256,
    bytes: u64,
    max: u64,
    quoted: bool,
    escaped: bool,
    token: usize,
    cancel: &'a AtomicBool,
    progress: &'a mut dyn FnMut(PrepareProgress),
    base_bytes: u64,
    reports: u64,
    next_progress: u64,
}
impl Write for Document<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        check_cancel(self.cancel).map_err(io::Error::other)?;
        if self.bytes + bytes.len() as u64 > self.max {
            return Err(io::Error::other("prepared document/staging byte limit"));
        }
        for &b in bytes {
            if self.quoted {
                self.token += 1;
                if self.token > 65536 {
                    return Err(io::Error::other("prepared JSON token exceeds 64 KiB"));
                }
                if self.escaped {
                    self.escaped = false;
                } else if b == b'\\' {
                    self.escaped = true;
                } else if b == b'"' {
                    self.quoted = false;
                }
            } else if b == b'"' {
                self.quoted = true;
                self.token = 1;
            }
        }
        self.output.write_all(bytes)?;
        self.hash.update(bytes);
        self.bytes += bytes.len() as u64;
        if self.bytes >= self.next_progress {
            (self.progress)(PrepareProgress {
                phase: "prepare",
                bytes: self.base_bytes + self.bytes,
                reports: self.reports,
            });
            self.next_progress = self.bytes + 65536;
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}
fn field(w: &mut Document<'_>, name: &str, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *w, name)?;
    w.write_all(b":")?;
    serde_json::to_writer(&mut *w, value)?;
    w.write_all(b",")?;
    Ok(())
}
fn array<T: Serialize>(
    w: &mut Document<'_>,
    name: &str,
    mut next: impl FnMut(u64) -> Result<Vec<T>>,
    cursor: impl Fn(&T) -> u64,
) -> Result<u64> {
    serde_json::to_writer(&mut *w, name)?;
    w.write_all(b":[")?;
    let mut after = 0;
    let mut count = 0;
    loop {
        let page = next(after)?;
        if page.is_empty() {
            break;
        }
        for value in page {
            if count != 0 {
                w.write_all(b",")?;
            }
            after = cursor(&value);
            serde_json::to_writer(&mut *w, &value)?;
            count += 1;
        }
    }
    w.write_all(b"],")?;
    Ok(count)
}
fn utf8(path: &Path) -> Result<String> {
    let text = path
        .to_str()
        .ok_or_else(|| fail("source path is not UTF-8"))?;
    if text.len() > 65534 {
        return Err(fail("source metadata exceeds 64 KiB"));
    }
    Ok(text.into())
}
impl<'a> Stage<'a> {
    // Include actual documents/journal plus projected dirty DB pages. Reserve one
    // 64 KiB block before each document for the small manifest update.
    fn preparation_bytes(&self) -> Result<u64> {
        let pages: u64 = self.db.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let size: u64 = self.db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        let physical = fs::metadata(self.dir.path().join("stage.sqlite3"))?.len();
        let mut bytes = (pages * size).saturating_sub(physical);
        for entry in fs::read_dir(self.dir.path())? {
            bytes += entry?.metadata()?.len();
        }
        Ok(bytes)
    }

    /// Consumes a converted stage. Failures drop its private files; no resumable incomplete handle.
    pub fn prepare(
        self,
        cancel: &AtomicBool,
        progress: impl FnMut(PrepareProgress),
    ) -> Result<PreparedImport<'a>> {
        self.prepare_with_mode(ImportMode::Manual, cancel, progress)
    }
    pub fn prepare_with_mode(
        self,
        mode: ImportMode,
        cancel: &AtomicBool,
        mut progress: impl FnMut(PrepareProgress),
    ) -> Result<PreparedImport<'a>> {
        check_cancel(cancel)?;
        let tx = rusqlite::Transaction::new_unchecked(
            &self.db,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let complete: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM manifest WHERE name='conversion_complete')",
            [],
            |r| r.get(0),
        )?;
        if !complete {
            return Err(fail("Report conversion is not complete"));
        }
        // Bound persisted metadata before reading values, including a damaged private stage.
        for (table, columns) in [
            ("converted_reports", "id kind start end sheet"),
            (
                "converted_rows",
                "report_id organization work work_cell count_cell amount_cell",
            ),
            (
                "converted_issues",
                "report_id kind reason organization work_cell count_cell amount_cell",
            ),
            ("converted_totals", "report_id kind organization"),
            ("converted_organizations", "report_id organization"),
        ] {
            check_cancel(cancel)?;
            let checks = columns
                .split_whitespace()
                .map(|c| format!("octet_length({c})>65536"))
                .collect::<Vec<_>>()
                .join(" OR ");
            let oversized: bool = self.db.query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {checks})"),
                [],
                |r| r.get(0),
            )?;
            if oversized {
                return Err(fail("converted metadata exceeds 64 KiB"));
            }
        }
        let source_name = utf8(Path::new(
            self.source_path
                .file_name()
                .ok_or_else(|| fail("missing source filename"))?,
        ))?;
        let source_folder = utf8(
            self.source_path
                .parent()
                .ok_or_else(|| fail("missing source folder"))?,
        )?;
        if fingerprint(&self.source_path, cancel, self.limits.archive_bytes)? != self.bound_hash {
            return Err(fail("source changed since ingestion"));
        }
        // Random 24-character stage name supplies operation uniqueness; hash gives bounded IDs.
        let operation = format!(
            "{:x}",
            Sha256::digest(self.dir.path().to_string_lossy().as_bytes())
        );
        self.db.execute_batch("CREATE TABLE prepared_reports(ordinal INTEGER PRIMARY KEY, local_id TEXT NOT NULL, id TEXT UNIQUE NOT NULL, kind TEXT NOT NULL, start TEXT NOT NULL, end TEXT NOT NULL, rows INTEGER NOT NULL, bytes INTEGER NOT NULL, hash TEXT NOT NULL, quality TEXT NOT NULL);")?;
        let mut after = 0;
        let mut reports = 0;
        let mut rows = 0;
        let mut bytes = 0u64;
        let mut quality = QualityCounts::default();
        loop {
            let page = self.report_page(after, 1)?;
            let Some(info) = page.into_iter().next() else {
                break;
            };
            if !["day", "week", "month"].contains(&info.kind.as_str())
                || [&info.start, &info.end].iter().any(|d| {
                    d.len() != 10 || chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err()
                })
                || info.start > info.end
                || !info.count.is_finite()
                || !info.amount.is_finite()
            {
                return Err(fail("invalid converted report metadata"));
            }
            if info.row_count == 0 {
                return Err(fail("empty reports cannot be prepared"));
            }
            let id = format!("native-{operation}-{}", info.ordinal);
            let used = self.preparation_bytes()?;
            let max = self
                .limits
                .stage_bytes
                .checked_sub(used + 65536)
                .ok_or_else(|| fail("prepared staging byte limit"))?
                .min(i32::MAX as u64);
            let output = OpenOptions::new().write(true).create_new(true).open(
                self.dir
                    .path()
                    .join(format!("report-{}.json", info.ordinal)),
            )?;
            let mut w = Document {
                output: BufWriter::with_capacity(65536, output),
                hash: Sha256::new(),
                bytes: 0,
                max,
                quoted: false,
                escaped: false,
                token: 0,
                cancel,
                progress: &mut progress,
                base_bytes: bytes,
                reports,
                next_progress: 65536,
            };
            w.write_all(b"{")?;
            field(&mut w, "id", &id)?;
            field(&mut w, "type", &info.kind)?;
            field(&mut w, "start", &info.start)?;
            field(&mut w, "end", &info.end)?;
            field(&mut w, "count", &info.count)?;
            field(&mut w, "amount", &info.amount)?;
            field(&mut w, "hasMismatch", &info.has_mismatch)?;
            field(&mut w, "sourceImportMode", &mode)?;
            field(&mut w, "desktopSource", &source_name)?;
            field(&mut w, "desktopFolder", &source_folder)?;
            field(&mut w, "sourceHash", &self.bound_hash)?;
            field(&mut w, "sourceSheet", &info.sheet)?;
            field(&mut w, "sourceMapping", &info.mapping)?;
            field(&mut w, "rowCount", &info.row_count)?;
            let organizations = self.report_organizations(&info.id)?;
            field(
                &mut w,
                "organizations",
                &organizations
                    .iter()
                    .map(|o| o.organization.as_str())
                    .collect::<Vec<_>>(),
            )?;
            field(&mut w, "organizationTotals", &organizations)?;
            let actual = array(
                &mut w,
                "rows",
                |c| {
                    let page = self.report_rows(&info.id, c, 100)?;
                    for (offset, row) in page.iter().enumerate() {
                        if row.row_index != c + offset as u64 + 1
                            || !row.count.is_finite()
                            || !row.amount.is_finite()
                        {
                            return Err(fail("invalid converted row"));
                        }
                    }
                    Ok(page)
                },
                |r| r.row_index,
            )?;
            if actual != info.row_count {
                return Err(fail("incomplete converted report rows"));
            }
            let mut report_quality = QualityCounts {
                reports: 1,
                rows: actual,
                mismatch_reports: u64::from(info.has_mismatch),
                ..Default::default()
            };
            let issues = array(
                &mut w,
                "issues",
                |c| {
                    let page = self.report_issues(&info.id, c, 100)?;
                    for issue in &page {
                        match issue.kind.as_str() {
                            "invalid" => report_quality.invalid += 1,
                            "skipped" => report_quality.skipped += 1,
                            "mismatch" => report_quality.mismatches += 1,
                            _ => report_quality.other_notices += 1,
                        }
                    }
                    Ok(page)
                },
                |r| r.sequence,
            )?;
            let totals = array(
                &mut w,
                "totals",
                |c| self.report_totals(&info.id, c, 100),
                |r| r.sequence,
            )?;
            report_quality.issues = issues;
            report_quality.controls = totals;
            quality.reports += 1;
            quality.rows += actual;
            quality.issues += issues;
            quality.controls += totals;
            quality.invalid += report_quality.invalid;
            quality.skipped += report_quality.skipped;
            quality.mismatches += report_quality.mismatches;
            quality.other_notices += report_quality.other_notices;
            quality.mismatch_reports += report_quality.mismatch_reports;
            // Warnings retain native reasons, not a claim of exact legacy presentation.
            w.write_all(b"\"warnings\":[")?;
            let mut cursor = 0;
            loop {
                let page = self.report_issues(&info.id, cursor, 100)?;
                if page.is_empty() {
                    break;
                }
                for issue in page {
                    if cursor != 0 {
                        w.write_all(b",")?;
                    }
                    serde_json::to_writer(&mut w, &issue.reason)?;
                    cursor = issue.sequence;
                }
            }
            w.write_all(b"],")?;
            field(&mut w, "issueCount", &issues)?;
            w.write_all(b"\"controlCount\":")?;
            serde_json::to_writer(&mut w, &totals)?;
            w.write_all(b"}")?;
            w.flush()?;
            w.output.get_ref().sync_all()?;
            bytes += w.bytes;
            rows += actual;
            reports += 1;
            let hash = format!("{:x}", w.hash.finalize());
            self.db.pragma_update(
                None,
                "max_page_count",
                ((self.limits.stage_bytes - bytes) / 4096).min(524288),
            )?;
            self.db.execute(
                "INSERT INTO prepared_reports VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    info.ordinal,
                    info.id,
                    id,
                    info.kind,
                    info.start,
                    info.end,
                    actual,
                    w.bytes,
                    hash,
                    serde_json::to_string(&report_quality)?
                ],
            )?;
            if self.preparation_bytes()? > self.limits.stage_bytes {
                return Err(fail("prepared staging byte limit"));
            }
            after = info.ordinal;
            progress(PrepareProgress {
                phase: "prepare",
                bytes,
                reports,
            });
        }
        if reports == 0 {
            return Err(fail("no complete reports to prepare"));
        }
        let overlaps: bool = self.db.query_row("SELECT EXISTS(SELECT 1 FROM prepared_reports a JOIN prepared_reports b ON a.ordinal<b.ordinal AND a.start<=b.end AND b.start<=a.end)", [], |r| r.get(0))?;
        if overlaps {
            return Err(fail(
                "overlapping prepared periods require a separate import",
            ));
        }
        self.db.execute(
            "INSERT INTO manifest VALUES('prepared_complete',?1)",
            [reports.to_string()],
        )?;
        if self.preparation_bytes()? > self.limits.stage_bytes {
            return Err(fail("prepared staging byte limit"));
        }
        check_cancel(cancel)?;
        tx.commit()?;
        let database_hash = fingerprint(
            &self.dir.path().join("stage.sqlite3"),
            cancel,
            self.limits.stage_bytes,
        )?;
        let metadata = fs::metadata(self.dir.path().join("stage.sqlite3"))?;
        let database_stamp = (metadata.len(), metadata.modified()?);
        let artifact = PreparedImport {
            mode,
            quality,
            database_stamp,
            stage: self,
            operation,
            database_hash,
            source_name,
            source_folder,
            reports,
            rows,
        };
        artifact.validate(cancel)?;
        Ok(artifact)
    }
}
impl PreparedImport<'_> {
    /// Explicit cleanup outcome, independent of any durable application receipt.
    /// Ordinary Drop remains best effort; restart recovery can discard leftovers.
    pub fn discard(self) -> Result<()> {
        let Stage { db, dir, .. } = self.stage;
        drop(db);
        dir.close()?;
        Ok(())
    }

    pub fn operation_id(&self) -> &str {
        &self.operation
    }
    pub fn source_name(&self) -> &str {
        &self.source_name
    }
    pub fn source_folder(&self) -> &str {
        &self.source_folder
    }
    pub fn source_hash(&self) -> &str {
        &self.stage.bound_hash
    }
    pub fn directory(&self) -> &Path {
        self.stage.dir.path()
    }
    pub fn report_count(&self) -> u64 {
        self.reports
    }
    pub fn row_count(&self) -> u64 {
        self.rows
    }
    pub fn validate_source_and_database(&self, cancel: &AtomicBool) -> Result<()> {
        if fingerprint(
            &self.stage.source_path,
            cancel,
            self.stage.limits.archive_bytes,
        )? != self.stage.bound_hash
        {
            return Err(fail("source changed since ingestion"));
        }
        if fingerprint(
            &self.directory().join("stage.sqlite3"),
            cancel,
            self.stage.limits.stage_bytes,
        )? != self.database_hash
        {
            return Err(fail("prepared database changed or incomplete"));
        }
        Ok(())
    }
    pub fn validate(&self, cancel: &AtomicBool) -> Result<()> {
        self.validate_source_and_database(cancel)?;
        let mut after = 0;
        let mut count = 0;
        let mut rows = 0;
        while let Some(report) = self.next_report(after)? {
            let path = self
                .directory()
                .join(format!("report-{}.json", report.ordinal));
            if fs::metadata(&path)?.len() != report.bytes
                || fingerprint(&path, cancel, report.bytes)? != report.sha256
            {
                return Err(fail("prepared document changed or incomplete"));
            }
            after = report.ordinal;
            count += 1;
            rows += report.rows;
        }
        if count != self.reports || rows != self.rows {
            return Err(fail("prepared manifest incomplete"));
        }
        Ok(())
    }
    fn check_stamp(&self) -> Result<()> {
        let m = fs::metadata(self.directory().join("stage.sqlite3"))?;
        if (m.len(), m.modified()?) != self.database_stamp {
            return Err(fail("prepared database changed; review unavailable"));
        }
        Ok(())
    }
    pub fn mode(&self) -> ImportMode {
        self.mode
    }
    pub fn quality(&self) -> &QualityCounts {
        &self.quality
    }
    /// Bounded report page, preserving source sheet, mapping, ordinal and complete scalar quality.
    pub fn reports(&self, after: u64, limit: usize) -> Result<Vec<PreparedReport>> {
        if !(1..=100).contains(&limit) {
            return Err(fail("report page limit is 1..100"));
        }
        self.check_stamp()?;
        let mut result = Vec::new();
        let mut cursor = after;
        while result.len() < limit {
            let Some(report) = self.next_report(cursor)? else {
                break;
            };
            cursor = report.ordinal;
            result.push(report);
        }
        Ok(result)
    }
    /// Bounded one-report cursor; callers cannot provide a replacement file list.
    pub fn next_report(&self, after: u64) -> Result<Option<PreparedReport>> {
        self.check_stamp()?;
        let row = self.stage.db.query_row("SELECT ordinal,local_id,id,kind,start,end,rows,bytes,hash,quality FROM prepared_reports WHERE ordinal>?1 ORDER BY ordinal LIMIT 1", [after], |r| Ok((r.get::<_,u64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,u64>(6)?,r.get::<_,u64>(7)?,r.get::<_,String>(8)?,r.get::<_,String>(9)?))).optional()?;
        let Some((ordinal, local_id, id, kind, start, end, rows, bytes, sha256, quality)) = row
        else {
            return Ok(None);
        };
        let info = self
            .stage
            .report_page(ordinal - 1, 1)?
            .into_iter()
            .next()
            .ok_or_else(|| fail("prepared report missing conversion"))?;
        let quality = serde_json::from_str(&quality)?;
        Ok(Some(PreparedReport {
            ordinal,
            local_id,
            id,
            kind,
            start,
            end,
            rows,
            bytes,
            sha256,
            info,
            quality,
        }))
    }
    pub fn open_document(&self, report: &PreparedReport) -> Result<File> {
        Ok(File::open(
            self.directory()
                .join(format!("report-{}.json", report.ordinal)),
        )?)
    }
    pub fn rows(&self, report: &PreparedReport, after: u64) -> Result<Vec<ReportRow>> {
        self.check_stamp()?;
        self.stage.report_rows(&report.local_id, after, 100)
    }
    pub fn issues(
        &self,
        report: &PreparedReport,
        after: u64,
        limit: usize,
    ) -> Result<Vec<ReportIssue>> {
        self.check_stamp()?;
        self.stage.report_issues(&report.local_id, after, limit)
    }
    pub fn controls(
        &self,
        report: &PreparedReport,
        after: u64,
        limit: usize,
    ) -> Result<Vec<ReportTotal>> {
        self.check_stamp()?;
        self.stage.report_totals(&report.local_id, after, limit)
    }
    pub fn organizations(&self, report: &PreparedReport) -> Result<Vec<ReportOrganization>> {
        self.check_stamp()?;
        self.stage.report_organizations(&report.local_id)
    }
}
