//! Additive, revision-checked SQLite read model. See storage-core/README.md.
use super::*;
use rusqlite::{types::Value as SqlValue, OptionalExtension};
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::io::{self, BufReader, Read};

pub const MAX_PAGE_ROWS: usize = 500;
const MAX_TOKEN_BYTES: usize = 64 * 1024;

pub(super) fn initialize(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "PRAGMA temp_store=FILE;
        PRAGMA cache_size=-2048;
        CREATE TABLE IF NOT EXISTS storage_revision (
          singleton INTEGER PRIMARY KEY CHECK(singleton=1),
          revision INTEGER NOT NULL CHECK(typeof(revision)='integer' AND revision>=0));
        INSERT OR IGNORE INTO storage_revision VALUES(1,0);
        CREATE TABLE IF NOT EXISTS report_row_index (
          report_id TEXT PRIMARY KEY, row_count INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS report_rows (
          report_id TEXT NOT NULL, row_index INTEGER NOT NULL,
          organization TEXT NOT NULL, work TEXT NOT NULL,
          count REAL NOT NULL, amount REAL NOT NULL,
          PRIMARY KEY(report_id,row_index)) WITHOUT ROWID;
        CREATE INDEX IF NOT EXISTS report_rows_by_work ON report_rows(work,organization,report_id);
        CREATE TRIGGER IF NOT EXISTS read_model_report_insert AFTER INSERT ON reports BEGIN
          UPDATE storage_revision SET revision=revision+1 WHERE singleton=1;
          DELETE FROM report_row_index WHERE report_id=NEW.id;
          DELETE FROM report_rows WHERE report_id=NEW.id;
        END;
        CREATE TRIGGER IF NOT EXISTS read_model_report_update AFTER UPDATE ON reports BEGIN
          UPDATE storage_revision SET revision=revision+1 WHERE singleton=1;
          DELETE FROM report_row_index WHERE report_id IN (OLD.id,NEW.id);
          DELETE FROM report_rows WHERE report_id IN (OLD.id,NEW.id);
        END;
        CREATE TRIGGER IF NOT EXISTS read_model_report_delete AFTER DELETE ON reports BEGIN
          UPDATE storage_revision SET revision=revision+1 WHERE singleton=1;
          DELETE FROM report_row_index WHERE report_id=OLD.id;
          DELETE FROM report_rows WHERE report_id=OLD.id;
        END;
        CREATE TRIGGER IF NOT EXISTS read_model_kv_insert AFTER INSERT ON kv BEGIN
          UPDATE storage_revision SET revision=revision+1 WHERE singleton=1;
        END;
        CREATE TRIGGER IF NOT EXISTS read_model_kv_update AFTER UPDATE ON kv BEGIN
          UPDATE storage_revision SET revision=revision+1 WHERE singleton=1;
        END;
        CREATE TRIGGER IF NOT EXISTS read_model_kv_delete AFTER DELETE ON kv BEGIN
          UPDATE storage_revision SET revision=revision+1 WHERE singleton=1;
        END;",
    )
    .map_err(err)
}

/// All types/dates/organizations/works by default. Dates require full period containment.
/// Organization/work lists are exact, case-sensitive OR matches; both lists combine with AND.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReadFilter {
    pub report_type: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    #[serde(default)]
    pub organizations: Vec<String>,
    #[serde(default)]
    pub works: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct ReportHeader {
    pub id: String,
    pub report_type: String,
    pub period_start: String,
    pub period_end: String,
    pub source_name: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct IndexedRow {
    pub report_id: String,
    pub row_index: i64,
    pub organization: String,
    pub work: String,
    pub count: f64,
    pub amount: f64,
}
#[derive(Debug, Serialize)]
pub struct Aggregate {
    pub rows: i64,
    pub count: f64,
    pub amount: f64,
    /// SUM(amount) / SUM(count), None when the denominator is zero.
    pub weighted_average: Option<f64>,
}

fn revision(conn: &Connection) -> Result<i64, String> {
    conn.query_row(
        "SELECT revision FROM storage_revision WHERE singleton=1",
        [],
        |r| r.get(0),
    )
    .map_err(err)
}
fn check_revision(conn: &Connection, expected: i64) -> Result<(), String> {
    if revision(conn)? != expected {
        return Err("Stale storage revision; refresh the selection.".into());
    }
    Ok(())
}
fn limit(limit: usize) -> Result<i64, String> {
    if !(1..=MAX_PAGE_ROWS).contains(&limit) {
        return Err("Page limit must be 1..=500.".into());
    }
    Ok(limit as i64)
}
fn filters(filter: &ReadFilter) -> Result<(String, Vec<SqlValue>), String> {
    if filter
        .report_type
        .as_deref()
        .is_some_and(|t| !REPORT_TYPES.contains(&t))
        || filter.start.as_deref().is_some_and(|d| !valid_date(d))
        || filter.end.as_deref().is_some_and(|d| !valid_date(d))
        || matches!((&filter.start,&filter.end),(Some(a),Some(b)) if a>b)
        || filter.organizations.len() > 100
        || filter.works.len() > 100
    {
        return Err("Invalid read filter (maximum 100 organizations/works).".into());
    }
    let mut clauses = vec!["1=1".to_string()];
    let mut values = Vec::new();
    for (column, value, operator) in [
        ("r.report_type", &filter.report_type, "="),
        ("r.period_start", &filter.start, ">="),
        ("r.period_end", &filter.end, "<="),
    ] {
        if let Some(value) = value {
            clauses.push(format!("{column}{operator}?"));
            values.push(value.clone().into());
        }
    }
    for (column, items) in [
        ("organization", &filter.organizations),
        ("work", &filter.works),
    ] {
        if !items.is_empty() {
            if items.iter().any(|v| v.len() > MAX_TOKEN_BYTES) {
                return Err("Filter string exceeds 64 KiB.".into());
            }
            clauses.push(format!(
                "x.{column} IN ({})",
                vec!["?"; items.len()].join(",")
            ));
            values.extend(items.iter().cloned().map(SqlValue::from));
        }
    }
    Ok((clauses.join(" AND "), values))
}
// Check validity for every selected report, including empty reports and reports with
// no matching organization/work. An absent manifest must never silently mean zero.
fn check_index(conn: &Connection, filter: &ReadFilter) -> Result<(), String> {
    let metadata = ReadFilter {
        organizations: vec![],
        works: vec![],
        ..filter.clone()
    };
    let (predicate, values) = filters(&metadata)?;
    let missing: bool = conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM reports r
        LEFT JOIN report_row_index i ON i.report_id=r.id WHERE {predicate} AND i.report_id IS NULL)"),
        rusqlite::params_from_iter(values), |r| r.get(0)).map_err(err)?;
    if missing {
        return Err("Selection contains unindexed reports; backfill before querying rows.".into());
    }
    Ok(())
}

#[derive(Deserialize)]
struct IndexInputRow {
    organization: String,
    work: String,
    count: f64,
    amount: f64,
}
impl IndexInputRow {
    fn validate(&self) -> Result<(), String> {
        if self.organization.len() > MAX_TOKEN_BYTES
            || self.work.len() > MAX_TOKEN_BYTES
            || !self.count.is_finite()
            || !self.amount.is_finite()
        {
            return Err("Invalid index row: finite numbers and <=64 KiB labels required.".into());
        }
        Ok(())
    }
}
// Limits raw JSON tokens before serde_json can grow its scratch buffer. This also
// protects IgnoredAny metadata (including huge unknown strings/numeric literals).
struct TokenBounded<R> {
    inner: R,
    string: bool,
    escape: bool,
    token: usize,
}
impl<R: Read> Read for TokenBounded<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        for &b in &buf[..n] {
            if self.string {
                self.token += 1;
                if self.escape {
                    self.escape = false;
                } else if b == b'\\' {
                    self.escape = true;
                } else if b == b'"' {
                    self.string = false;
                    self.token = 0;
                }
            } else if b == b'"' {
                self.string = true;
                self.token = 0;
            } else if matches!(
                b,
                b' ' | b'\n' | b'\r' | b'\t' | b',' | b':' | b'[' | b']' | b'{' | b'}'
            ) {
                self.token = 0;
            } else {
                self.token += 1;
            }
            if self.token > MAX_TOKEN_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "JSON token exceeds 64 KiB",
                ));
            }
        }
        Ok(n)
    }
}
struct ReportSeed<'a> {
    conn: &'a Connection,
    metadata: [&'a str; 4],
}
struct RowsSeed<'a> {
    conn: &'a Connection,
}
impl<'de> DeserializeSeed<'de> for RowsSeed<'_> {
    type Value = i64;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<i64, D::Error> {
        struct RowsVisitor<'a>(&'a Connection);
        impl<'de> Visitor<'de> for RowsVisitor<'_> {
            type Value = i64;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a report row array")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<i64, A::Error> {
                use serde::de::Error;
                let mut stmt = self
                    .0
                    .prepare("INSERT INTO temp.row_stage VALUES(?1,?2,?3,?4,?5)")
                    .map_err(A::Error::custom)?;
                let mut count: i64 = 0;
                while let Some(row) = seq.next_element::<IndexInputRow>()? {
                    row.validate().map_err(A::Error::custom)?;
                    stmt.execute(params![
                        count,
                        row.organization,
                        row.work,
                        row.count,
                        row.amount
                    ])
                    .map_err(A::Error::custom)?;
                    count = count
                        .checked_add(1)
                        .ok_or_else(|| A::Error::custom("Too many rows"))?;
                }
                Ok(count)
            }
        }
        d.deserialize_seq(RowsVisitor(self.conn))
    }
}
impl<'de> DeserializeSeed<'de> for ReportSeed<'_> {
    type Value = i64;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<i64, D::Error> {
        struct ReportVisitor<'a> {
            conn: &'a Connection,
            metadata: [&'a str; 4],
        }
        impl<'de> Visitor<'de> for ReportVisitor<'_> {
            type Value = i64;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a report object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<i64, A::Error> {
                use serde::de::Error;
                let mut rows = None;
                let mut seen = [false; 4];
                while let Some(key) = map.next_key::<String>()? {
                    if key == "rows" {
                        if rows.is_some() {
                            return Err(A::Error::custom("Duplicate rows field"));
                        }
                        rows = Some(map.next_value_seed(RowsSeed { conn: self.conn })?);
                    } else if let Some(n) = ["id", "type", "start", "end"]
                        .iter()
                        .position(|k| *k == key)
                    {
                        if seen[n] {
                            return Err(A::Error::custom("Duplicate report metadata"));
                        }
                        seen[n] = true;
                        let value = map.next_value::<String>()?;
                        if value != self.metadata[n] {
                            return Err(A::Error::custom(
                                "JSON metadata differs from report columns",
                            ));
                        }
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                if seen.iter().any(|v| !v) {
                    return Err(A::Error::custom("Missing report metadata"));
                }
                rows.ok_or_else(|| A::Error::custom("Missing rows field"))
            }
        }
        d.deserialize_map(ReportVisitor {
            conn: self.conn,
            metadata: self.metadata,
        })
    }
}

pub(super) fn index_value(tx: &Transaction<'_>, id: &str, report: &Value) -> Result<(), String> {
    let rows = report["rows"].as_array().ok_or("Missing rows")?;
    // Original saves permit arbitrarily long labels within the existing payload
    // budget. Preserve that compatibility: keep their JSON and omit the manifest,
    // so bounded queries fail explicitly rather than reject or truncate the save.
    if id.len() > MAX_TOKEN_BYTES
        || rows.iter().any(|row| {
            row["organization"]
                .as_str()
                .is_some_and(|v| v.len() > MAX_TOKEN_BYTES)
                || row["work"]
                    .as_str()
                    .is_some_and(|v| v.len() > MAX_TOKEN_BYTES)
        })
    {
        return Ok(());
    }
    let mut stmt = tx
        .prepare("INSERT INTO report_rows VALUES(?1,?2,?3,?4,?5,?6)")
        .map_err(err)?;
    for (n, value) in rows.iter().enumerate() {
        let row = IndexInputRow {
            organization: value["organization"]
                .as_str()
                .ok_or("Missing organization")?
                .into(),
            work: value["work"].as_str().ok_or("Missing work")?.into(),
            count: value["count"].as_f64().ok_or("Invalid count")?,
            amount: value["amount"].as_f64().ok_or("Invalid amount")?,
        };
        row.validate()?;
        stmt.execute(params![
            id,
            n as i64,
            row.organization,
            row.work,
            row.count,
            row.amount
        ])
        .map_err(err)?;
    }
    tx.execute(
        "INSERT INTO report_row_index VALUES(?1,?2)",
        params![id, rows.len() as i64],
    )
    .map_err(err)?;
    Ok(())
}

impl Storage {
    pub fn revision(&self) -> Result<i64, String> {
        let _guard = self.gate.lock().map_err(err)?;
        revision(&self.connect()?)
    }
    /// Backfill ONE report, with an 8 KiB reader, <=64 KiB tokens, one row at a
    /// time, and a FILE temp table. No main-table writes occur before validation.
    pub fn index_report(&self, id: &str, expected_revision: i64) -> Result<i64, String> {
        self.index_report_before_publish(id, expected_revision, || {})
    }
    pub(super) fn index_report_before_publish(
        &self,
        id: &str,
        expected_revision: i64,
        before_publish: impl FnOnce(),
    ) -> Result<i64, String> {
        if id.len() > MAX_TOKEN_BYTES {
            return Err("Report id exceeds 64 KiB".into());
        }
        let _guard = self.gate.lock().map_err(err)?;
        let mut conn = self.connect()?;
        conn.execute_batch("CREATE TEMP TABLE row_stage(row_index INTEGER PRIMARY KEY, organization TEXT NOT NULL, work TEXT NOT NULL, count REAL NOT NULL, amount REAL NOT NULL);
            PRAGMA temp.cache_size=-2048;") .map_err(err)?;
        let tx = conn.transaction().map_err(err)?;
        check_revision(&tx, expected_revision)?;
        let (rowid,kind,start,end): (i64,String,String,String)=tx.query_row("SELECT rowid,report_type,period_start,period_end FROM reports WHERE id=?1 AND length(CAST(report_type AS BLOB))<=65536 AND length(CAST(period_start AS BLOB))<=65536 AND length(CAST(period_end AS BLOB))<=65536",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(err)?.ok_or("Report not found or metadata exceeds limits")?;
        if !REPORT_TYPES.contains(&kind.as_str()) || !valid_date(&start) || !valid_date(&end) {
            return Err("Invalid report metadata".into());
        }
        let count = {
            let blob = tx
                .blob_open("main", "reports", "json", rowid, true)
                .map_err(err)?;
            let bounded = TokenBounded {
                inner: blob,
                string: false,
                escape: false,
                token: 0,
            };
            let mut de =
                serde_json::Deserializer::from_reader(BufReader::with_capacity(8192, bounded));
            let count = ReportSeed {
                conn: &tx,
                metadata: [id, &kind, &start, &end],
            }
            .deserialize(&mut de)
            .map_err(err)?;
            de.end().map_err(err)?;
            count
        };
        before_publish();
        // Upgrading a WAL snapshot after an external write fails SQLITE_BUSY_SNAPSHOT;
        // the transaction rolls back rather than publishing rows for old JSON.
        tx.execute("DELETE FROM report_rows WHERE report_id=?1", [id])
            .map_err(err)?;
        tx.execute("INSERT INTO report_rows SELECT ?1,row_index,organization,work,count,amount FROM temp.row_stage",[id]).map_err(err)?;
        tx.execute("INSERT INTO report_row_index VALUES(?1,?2) ON CONFLICT(report_id) DO UPDATE SET row_count=excluded.row_count",params![id,count]).map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(count)
    }
    /// Keyset order: (report_id BINARY,row_index). Caller carries the revision.
    pub fn row_page(
        &self,
        expected_revision: i64,
        filter: &ReadFilter,
        after: Option<(&str, i64)>,
        page_size: usize,
    ) -> Result<Vec<IndexedRow>, String> {
        let cap = limit(page_size)?;
        let (mut predicate, mut values) = filters(filter)?;
        if let Some((id, n)) = after {
            predicate.push_str(" AND (x.report_id,x.row_index)>(?,?)");
            values.extend([SqlValue::from(id.to_string()), n.into()]);
        }
        values.push(cap.into());
        let _guard = self.gate.lock().map_err(err)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(err)?;
        check_revision(&tx, expected_revision)?;
        check_index(&tx, filter)?;
        let mut stmt=tx.prepare(&format!("SELECT x.report_id,x.row_index,x.organization,x.work,x.count,x.amount FROM report_rows x JOIN reports r ON r.id=x.report_id WHERE {predicate} ORDER BY x.report_id,x.row_index LIMIT ?")).map_err(err)?;
        let result = stmt
            .query_map(rusqlite::params_from_iter(values), |r| {
                Ok(IndexedRow {
                    report_id: r.get(0)?,
                    row_index: r.get(1)?,
                    organization: r.get(2)?,
                    work: r.get(3)?,
                    count: r.get(4)?,
                    amount: r.get(5)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(result)
    }
    /// Report metadata only. Organization/work filters select reports having a matching row.
    pub fn report_page(
        &self,
        expected_revision: i64,
        filter: &ReadFilter,
        after: Option<(&str, &str)>,
        page_size: usize,
    ) -> Result<Vec<ReportHeader>, String> {
        let cap = limit(page_size)?;
        let (predicate, mut values) = filters(filter)?;
        let mut predicate =
            format!("EXISTS(SELECT 1 FROM report_rows x WHERE x.report_id=r.id AND {predicate})");
        // Include valid empty reports when no row predicates were requested.
        if filter.organizations.is_empty() && filter.works.is_empty() {
            let f = filters(filter)?;
            predicate = f.0;
            values = f.1;
        }
        if let Some((date, id)) = after {
            predicate.push_str(" AND (r.period_start,r.id)>(?,?)");
            values.extend([date.to_string().into(), id.to_string().into()]);
        }
        values.push(cap.into());
        let _guard = self.gate.lock().map_err(err)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(err)?;
        check_revision(&tx, expected_revision)?;
        // Headers without row predicates are available before backfilling.
        if !filter.organizations.is_empty() || !filter.works.is_empty() {
            check_index(&tx, filter)?;
        }
        let mut stmt=tx.prepare(&format!("SELECT CASE WHEN length(CAST(r.id AS BLOB))<=65536 THEN r.id END,
          CASE WHEN length(CAST(r.report_type AS BLOB))<=65536 THEN r.report_type END,
          CASE WHEN length(CAST(r.period_start AS BLOB))<=65536 THEN r.period_start END,
          CASE WHEN length(CAST(r.period_end AS BLOB))<=65536 THEN r.period_end END,
          CASE WHEN length(CAST(r.source_name AS BLOB))<=65536 OR r.source_name IS NULL THEN r.source_name ELSE 0 END
          FROM reports r WHERE {predicate} ORDER BY r.period_start,r.id LIMIT ?")).map_err(err)?;
        let result = stmt
            .query_map(rusqlite::params_from_iter(values), |r| {
                Ok(ReportHeader {
                    id: r.get(0)?,
                    report_type: r.get(1)?,
                    period_start: r.get(2)?,
                    period_end: r.get(3)?,
                    source_name: r.get(4)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(result)
    }
    pub fn aggregate(
        &self,
        expected_revision: i64,
        filter: &ReadFilter,
    ) -> Result<Aggregate, String> {
        let (predicate, values) = filters(filter)?;
        let _guard = self.gate.lock().map_err(err)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(err)?;
        check_revision(&tx, expected_revision)?;
        check_index(&tx, filter)?;
        let (rows,count,amount):(i64,f64,f64)=tx.query_row(&format!("SELECT COUNT(*),COALESCE(SUM(x.count),0),COALESCE(SUM(x.amount),0) FROM report_rows x JOIN reports r ON r.id=x.report_id WHERE {predicate}"),rusqlite::params_from_iter(values),|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(err)?;
        let average = if count == 0.0 {
            None
        } else {
            Some(amount / count)
        };
        if !count.is_finite() || !amount.is_finite() || average.is_some_and(|n| !n.is_finite()) {
            return Err("Aggregate numeric overflow".into());
        }
        Ok(Aggregate {
            rows,
            count,
            amount,
            weighted_average: average,
        })
    }
    /// Complete, consistent SQLite backup, including unindexed reports and all kv.
    /// Destination must not exist. Backs up 64 pages per step; no whole-history Values.
    pub fn backup_sqlite(&self, destination: &Path) -> Result<(), String> {
        let _guard = self.gate.lock().map_err(err)?;
        let source = self.connect()?;
        if fs::symlink_metadata(destination).is_ok() {
            return Err("Backup destination already exists".into());
        }
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let stamp = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(err)?
            .as_nanos();
        let mut temporary = None;
        for attempt in 0..100 {
            let path = parent.join(format!(
                ".analiz-backup-{}-{stamp}-{attempt}.sqlite3",
                std::process::id()
            ));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => {
                    drop(file);
                    temporary = Some(path);
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(err(e)),
            }
        }
        let temporary = temporary.ok_or("Unable to reserve a backup staging file")?;
        let result = (|| {
            {
                let mut target = Connection::open(&temporary).map_err(err)?;
                let backup = rusqlite::backup::Backup::new(&source, &mut target).map_err(err)?;
                backup
                    .run_to_completion(64, std::time::Duration::from_millis(10), None)
                    .map_err(err)?;
            }
            // Atomic no-clobber publication also protects a destination created
            // by another process while the backup was running.
            fs::hard_link(&temporary, destination).map_err(err)
        })();
        let _ = fs::remove_file(&temporary);
        result
    }
}
