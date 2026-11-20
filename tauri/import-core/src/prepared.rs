//! Owned, complete disk artifacts. No public constructor from caller file lists.
use crate::{
    check_cancel, fail, fingerprint, ReportInfo, ReportIssue, ReportOrganization, ReportProvenance,
    ReportRow, ReportTotal, Result, SourceIssue, Stage,
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
    pub source_issues: u64,
    pub relevant_source_issues: u64,
    pub deferred_source_issues: u64,
}
impl QualityCounts {
    pub fn requires_acknowledgment(&self) -> bool {
        self.issues != 0 || self.mismatch_reports != 0 || self.relevant_source_issues != 0
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct SourceDiagnostics {
    pub issues: u64,
    pub relevant: u64,
    pub deferred: u64,
    pub sha256: String,
}
struct DigestWriter(Sha256);
impl Write for DigestWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
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
    source_diagnostics: SourceDiagnostics,
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
    pub provenance: Option<ReportProvenance>,
    validated_zero: bool,
    local_id: String,
}
impl PreparedReport {
    pub fn is_explicit_zero(&self) -> bool {
        self.validated_zero
    }
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
        // Only explicitly supported conversion markers may publish documents.
        // Wide provenance, quality and zero proof are validated below.
        let converted: Option<String> = self.db.query_row(
            "SELECT value FROM manifest WHERE name='conversion_complete'",
            [],
            |r| r.get(0),
        ).optional()?;
        if !matches!(converted.as_deref(), Some("legacy" | "daily-wide")) {
            return Err(fail("Unsupported or incomplete report conversion"));
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
                "report_id kind reason organization work_cell count_cell amount_cell scope date",
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
        let wide = converted.as_deref() == Some("daily-wide");
        if wide {
            for (table, columns) in [
                (
                    "wide_provenance",
                    "report_id sheet_name source_control_date coverage_start coverage_end",
                ),
                (
                    "wide_source_issues",
                    "sheet report_id kind reason date organization count_cell amount_cell",
                ),
            ] {
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
                    return Err(fail("wide metadata exceeds 64 KiB"));
                }
            }
        }
        let mut source_diagnostics = SourceDiagnostics::default();
        let mut digest = DigestWriter(Sha256::new());
        let mut cursor = 0;
        loop {
            check_cancel(cancel)?;
            let page = self.source_issue_page(cursor, 100)?;
            if page.is_empty() {
                break;
            }
            for issue in page {
                source_diagnostics.issues += 1;
                if issue.relevant {
                    source_diagnostics.relevant += 1;
                } else {
                    source_diagnostics.deferred += 1;
                }
                cursor = issue.sequence;
                serde_json::to_writer(&mut digest, &issue)?;
                digest.write_all(b"\n")?;
            }
        }
        source_diagnostics.sha256 = format!("{:x}", digest.0.finalize());
        if wide {
            let (source_count, relevant_count): (u64, u64) = self.db.query_row(
                "SELECT COUNT(*),COALESCE(SUM(relevant=1),0) FROM wide_source_issues",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if source_count != source_diagnostics.issues
                || relevant_count != source_diagnostics.relevant
            {
                return Err(fail("source diagnostic cursor is incomplete"));
            }
            let (attached,unique):(u64,u64)=self.db.query_row("SELECT COUNT(*),COUNT(DISTINCT source_issue_seq) FROM converted_issues WHERE scope='source-period'",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
            let invalid:bool=self.db.query_row("SELECT EXISTS(SELECT 1 FROM converted_issues i LEFT JOIN wide_source_issues s ON s.seq=i.source_issue_seq WHERE i.scope='source-period' AND (s.seq IS NULL OR s.relevant<>1 OR s.report_id IS NOT i.report_id OR i.date IS NOT s.date OR i.kind IS NOT s.kind OR i.reason IS NOT s.reason OR i.source_row IS NOT s.source_row OR i.organization IS NOT s.organization OR i.work_cell IS NOT NULL OR i.skipped IS NOT NULL OR i.count_cell IS NOT s.count_cell OR i.amount_cell IS NOT s.amount_cell OR i.expected_count IS NOT s.expected_count OR i.expected_amount IS NOT s.expected_amount OR i.actual_count IS NOT s.actual_count OR i.actual_amount IS NOT s.actual_amount)) OR EXISTS(SELECT 1 FROM wide_source_issues WHERE relevant NOT IN (0,1))",[],|r|r.get(0))?;
            if attached != source_diagnostics.relevant || unique != attached || invalid {
                return Err(fail("source diagnostic attachment is incomplete"));
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
        if self.current_source_hash(cancel)? != self.bound_hash {
            return Err(fail("source changed since ingestion"));
        }
        // Random 24-character stage name supplies operation uniqueness; hash gives bounded IDs.
        let operation = format!(
            "{:x}",
            Sha256::digest(self.dir.path().to_string_lossy().as_bytes())
        );
        self.db.execute_batch("CREATE TABLE prepared_reports(ordinal INTEGER PRIMARY KEY, local_id TEXT NOT NULL, id TEXT UNIQUE NOT NULL, kind TEXT NOT NULL, start TEXT NOT NULL, end TEXT NOT NULL, rows INTEGER NOT NULL, bytes INTEGER NOT NULL, hash TEXT NOT NULL, quality TEXT NOT NULL, provenance TEXT, validated_zero INTEGER NOT NULL);")?;
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
            let provenance = if wide {
                Some(
                    self.report_provenance(&info.id)?
                        .ok_or_else(|| fail("missing wide report provenance"))?,
                )
            } else {
                None
            };
            let validated_zero = validate_provenance_and_zero(&self, &info, provenance.as_ref())?;
            if info.row_count == 0 && !validated_zero {
                return Err(fail(
                    "empty reports require proven native wide zero controls",
                ));
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
            if let Some(provenance) = &provenance {
                field(&mut w, "sheetName", &provenance.sheet_name)?;
                field(&mut w, "sourceReportId", &info.id)?;
                field(&mut w, "fileName", &source_name)?;
                field(&mut w, "sourceFormat", &provenance.source_format)?;
                field(&mut w, "sourceBlock", &provenance.source_block)?;
                field(&mut w, "sourceCoverage", &provenance.coverage)?;
                field(&mut w, "sourceControlDate", &provenance.source_control_date)?;
                field(&mut w, "explicitZero", &provenance.explicit_zero)?;
                field(&mut w, "sourceCol", &(info.mapping.work_col - 1))?;
                field(&mut w, "sourceCountCol", &(info.mapping.count_col - 1))?;
                field(&mut w, "sourceAmountCol", &(info.mapping.amount_col - 1))?;
                let first: u64 =
                    self.db
                        .query_row("SELECT MIN(ordinal) FROM converted_reports", [], |r| {
                            r.get(0)
                        })?;
                field(
                    &mut w,
                    "sourceDiagnosticsReport",
                    &format!("native-{operation}-{first}"),
                )?;
                if reports == 0 {
                    field(&mut w, "sourceDiagnostics", &source_diagnostics)?;
                    let actual = array(
                        &mut w,
                        "sourceIssues",
                        |c| self.source_issue_page(c, 100),
                        |i| i.sequence,
                    )?;
                    if actual != source_diagnostics.issues {
                        return Err(fail("incomplete source diagnostic document"));
                    }
                }
            }

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
                source_issues: if reports == 0 {
                    source_diagnostics.issues
                } else {
                    0
                },
                relevant_source_issues: if reports == 0 {
                    source_diagnostics.relevant
                } else {
                    0
                },
                deferred_source_issues: if reports == 0 {
                    source_diagnostics.deferred
                } else {
                    0
                },
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
            quality.source_issues += report_quality.source_issues;
            quality.relevant_source_issues += report_quality.relevant_source_issues;
            quality.deferred_source_issues += report_quality.deferred_source_issues;
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
                "INSERT INTO prepared_reports VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
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
                    serde_json::to_string(&report_quality)?,
                    provenance.as_ref().map(serde_json::to_string).transpose()?,
                    validated_zero
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
        let (expected_reports, expected_rows): (u64, u64) = self.db.query_row(
            "SELECT COUNT(*),COALESCE(SUM(row_count),0) FROM converted_reports",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let actual_rows: u64 =
            self.db
                .query_row("SELECT COUNT(*) FROM converted_rows", [], |r| r.get(0))?;
        if expected_reports != reports || expected_rows != rows || actual_rows != rows {
            return Err(fail("incomplete converted report manifest"));
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
        self.db.execute(
            "INSERT INTO manifest VALUES('prepared_source_diagnostics',?1)",
            [serde_json::to_string(&source_diagnostics)?],
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
            source_diagnostics,
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
    /// Catalog extraction only creates temporary SQLite tables; it does not
    /// mutate the sealed prepared artifact or its source/database identity.
    pub fn visit_catalog(&self, organizations:&[String], cancel:&AtomicBool, accept:impl FnMut(&str,&str)->Result<()>) -> Result<u64> {
        self.stage.visit_catalog(organizations,cancel,accept)
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
        if self.stage.current_source_hash(cancel)? != self.stage.bound_hash
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
    pub fn source_diagnostics(&self) -> &SourceDiagnostics {
        &self.source_diagnostics
    }
    pub fn source_issues(&self, after: u64, limit: usize) -> Result<Vec<SourceIssue>> {
        self.check_stamp()?;
        self.stage.source_issue_page(after, limit)
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
        let row = self.stage.db.query_row("SELECT ordinal,local_id,id,kind,start,end,rows,bytes,hash,quality,provenance,validated_zero FROM prepared_reports WHERE ordinal>?1 ORDER BY ordinal LIMIT 1", [after], |r| Ok((r.get::<_,u64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,u64>(6)?,r.get::<_,u64>(7)?,r.get::<_,String>(8)?,r.get::<_,String>(9)?,r.get::<_,Option<String>>(10)?,r.get::<_,bool>(11)?))).optional()?;
        let Some((
            ordinal,
            local_id,
            id,
            kind,
            start,
            end,
            rows,
            bytes,
            sha256,
            quality,
            provenance,
            validated_zero,
        )) = row
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
            provenance: provenance
                .map(|json| serde_json::from_str(&json))
                .transpose()?,
            validated_zero,
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

fn valid_date(date: &str) -> bool {
    date.len() == 10 && chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok()
}
fn validate_provenance_and_zero(
    stage: &Stage<'_>,
    info: &crate::ReportInfo,
    provenance: Option<&ReportProvenance>,
) -> Result<bool> {
    let Some(p) = provenance else {
        return Ok(false);
    };
    if p.source_format != "daily-wide"
        || p.sheet_name.is_empty()
        || p.sheet_name.len() > 65536
        || info.kind != "day"
        || info.start != info.end
        || !(1..=1024).contains(&p.source_block)
        || !valid_date(&p.coverage.start)
        || !valid_date(&p.coverage.end)
        // Declared title coverage is source-period evidence, independent of
        // valid explicit daily headers; missing-day diagnostics preserve the mismatch.
        || p.coverage.start > p.coverage.end
        || p.source_control_date
            .as_ref()
            .is_some_and(|d| !valid_date(d))
        || [
            info.mapping.work_col,
            info.mapping.count_col,
            info.mapping.amount_col,
        ]
        .iter()
        .any(|c| *c == 0 || *c > stage.limits.columns)
        || info.mapping.work_col == info.mapping.count_col
        || info.mapping.work_col == info.mapping.amount_col
        || info.mapping.count_col == info.mapping.amount_col
        || info.mapping.header_row == 0 || info.mapping.header_row > stage.limits.rows
    {
        return Err(fail("invalid wide report provenance"));
    }
    if let Some(control) = &p.source_control_date {
        let first:String=stage.db.query_row("SELECT r.start FROM converted_reports r JOIN wide_provenance p ON p.report_id=r.id WHERE p.sheet_name=?1 ORDER BY r.ordinal LIMIT 1",[&p.sheet_name],|r|r.get(0))?;
        if control != &first {
            return Err(fail("invalid wide source control date"));
        }
    }
    if p.explicit_zero && info.row_count != 0 {
        return Err(fail("zero provenance has detail rows"));
    }
    if info.row_count != 0 {
        return Ok(false);
    }
    if !p.explicit_zero || info.count != 0.0 || info.amount != 0.0 {
        return Err(fail("empty report lacks explicit zero proof"));
    }
    let proof:bool=stage.db.query_row("SELECT EXISTS(SELECT 1 FROM converted_totals WHERE report_id=?1 AND kind='org') AND NOT EXISTS(SELECT 1 FROM converted_totals WHERE report_id=?1 AND (count IS NULL OR amount IS NULL OR count<>0 OR amount<>0 OR kind NOT IN ('org','all'))) AND (SELECT COUNT(*) FROM converted_organizations WHERE report_id=?1) BETWEEN 1 AND 4 AND NOT EXISTS(SELECT 1 FROM converted_organizations o WHERE o.report_id=?1 AND NOT EXISTS(SELECT 1 FROM converted_totals t WHERE t.report_id=?1 AND t.kind='org' AND t.organization=o.organization)) AND NOT EXISTS(SELECT 1 FROM converted_totals t WHERE t.report_id=?1 AND t.kind='org' AND NOT EXISTS(SELECT 1 FROM converted_organizations o WHERE o.report_id=?1 AND o.organization=t.organization)) AND NOT EXISTS(SELECT 1 FROM converted_organizations WHERE report_id=?1 AND (count<>0 OR amount<>0 OR count IS NULL OR amount IS NULL)) AND NOT EXISTS(SELECT 1 FROM converted_rows WHERE report_id=?1)",[&info.id],|r|r.get(0))?;
    if !proof {
        return Err(fail("empty report zero controls are not proven"));
    }
    Ok(true)
}
