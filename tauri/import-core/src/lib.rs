//! Disk-backed XLSX ingestion. No report conversion or live-database writes here.
mod prepared;
mod report_stage;
pub use prepared::{
    ImportMode, PrepareProgress, PreparedImport, PreparedReport, QualityCounts, SourceDiagnostics,
};
mod wide;
pub use wide::{Coverage, ReportProvenance, SourceIssue, WideMapping, WideOptions, WideSheet};
mod zip_index;
use fs2::FileExt;
use quick_xml::{
    events::{BytesStart, Event},
    NsReader,
};
pub use report_stage::{
    ConversionSummary, LegacyOptions, ReportInfo, ReportIssue, ReportOrganization, ReportRow,
    ReportTotal, TripleMapping,
};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell as Counter,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};
use tempfile::TempDir;
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn fail(s: &str) -> Box<dyn std::error::Error + Send + Sync> {
    io::Error::other(s).into()
}
#[derive(Clone, Copy)]
pub struct Limits {
    pub archive_bytes: u64,
    pub stage_bytes: u64,
    pub component_bytes: u64,
    pub total_bytes: u64,
    pub token_bytes: u64,
    pub text_bytes: usize,
    pub entries: usize,
    pub ratio: u64,
    pub rows: u32,
    pub columns: u32,
    pub cells: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            archive_bytes: 30 * 1024 * 1024,
            stage_bytes: 2 * 1024 * 1024 * 1024,
            component_bytes: 48 * 1024 * 1024,
            total_bytes: 192 * 1024 * 1024,
            token_bytes: 1024 * 1024,
            text_bytes: 65536,
            entries: 4096,
            ratio: 1000,
            rows: 60000,
            columns: 2048,
            cells: 3_000_000,
        }
    }
}
#[derive(Default, Debug, Clone)]
pub struct Stats {
    pub cells: u64,
    pub empty_cells: u64,
    pub strings: u64,
    pub unpacked_bytes: u64,
    pub components: Vec<(String, u64)>,
}
#[derive(Debug)]
pub struct Cell {
    pub address: String,
    pub value: String,
    pub kind: String,
    pub style: String,
    pub formula: bool,
}
pub struct StageArea {
    root: PathBuf,
    _lock: File,
}
pub struct Stage<'a> {
    db: Connection,
    dir: TempDir,
    _area: &'a StageArea,
    pub stats: Stats,
    pub source_hash: String,
    source_path: PathBuf,
    bound_hash: String,
    limits: Limits,
}
impl StageArea {
    /// One manager owns a private staging root. Recovery runs under its OS lock.
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        if fs::symlink_metadata(&root)?.file_type().is_symlink() {
            return Err(fail("staging root is a symlink"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("manager.lock"))?;
        lock.try_lock_exclusive()?;
        for item in fs::read_dir(&root)? {
            let item = item?;
            if item.file_name().to_string_lossy().starts_with("import-") {
                if item.file_type()?.is_dir() {
                    fs::remove_dir_all(item.path())?;
                } else {
                    fs::remove_file(item.path())?;
                }
            }
        }
        Ok(Self { root, _lock: lock })
    }
    pub fn ingest(
        &self,
        source: &Path,
        limits: Limits,
        cancel: &AtomicBool,
        mut progress: impl FnMut(&Stats),
    ) -> Result<Stage<'_>> {
        check_cancel(cancel)?;
        let source_path = fs::canonicalize(source)?;
        let mut file = File::open(&source_path)?;
        if file.metadata()?.len() > limits.archive_bytes {
            return Err(fail("XLSX archive byte limit"));
        }
        let hash = hash_reader(&mut file, cancel, limits.archive_bytes)?;
        file.seek(SeekFrom::Start(0))?;
        let mut zip = zip_index::Index::open(file, limits, cancel)?;
        let dir = tempfile::Builder::new()
            .prefix("import-")
            .rand_bytes(24)
            .tempdir_in(&self.root)?;
        let db = Connection::open(dir.path().join("stage.sqlite3"))?;
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA cache_size=-2048; PRAGMA temp_store=FILE; PRAGMA max_page_count=524288;
          CREATE TABLE cells(sheet TEXT,row_num INTEGER,col_num INTEGER,address TEXT,kind TEXT,style TEXT,value TEXT,formula INTEGER,PRIMARY KEY(sheet,row_num,col_num)) WITHOUT ROWID;
          CREATE TABLE strings(id INTEGER PRIMARY KEY,value TEXT NOT NULL);
          CREATE TABLE metadata(component TEXT,seq INTEGER,event TEXT,name TEXT,value TEXT,PRIMARY KEY(component,seq)) WITHOUT ROWID;
          CREATE TABLE nodes(component TEXT,seq INTEGER,tag TEXT,parent TEXT,PRIMARY KEY(component,seq)) WITHOUT ROWID;
          CREATE TABLE attributes(component TEXT,seq INTEGER,key TEXT,value TEXT,namespace_uri TEXT,PRIMARY KEY(component,seq,key)) WITHOUT ROWID;
          CREATE INDEX node_tag ON nodes(component,tag,seq);
          CREATE TABLE merges(sheet TEXT,seq INTEGER,r INTEGER,c INTEGER,end_row INTEGER,end_col INTEGER,PRIMARY KEY(sheet,seq)) WITHOUT ROWID;
          CREATE INDEX merge_position ON merges(sheet,c,end_col,r,end_row);
          CREATE TABLE manifest(name TEXT PRIMARY KEY,value TEXT); BEGIN;")?;
        let pages = (limits.stage_bytes / 4096).min(524288);
        if pages < 16 {
            return Err(fail("staging disk budget too small"));
        }
        db.pragma_update(None, "max_page_count", pages)?;
        let mut stage = Stage {
            dir,
            db,
            _area: self,
            stats: Stats::default(),
            source_hash: hash.clone(),
            bound_hash: hash,
            source_path,
            limits,
        };
        for info in &zip.entries {
            check_cancel(cancel)?;
            if info.name.ends_with('/') {
                continue;
            }
            zip.file.seek(SeekFrom::Start(info.data))?;
            let mut stream = info.header().chain((&mut zip.file).take(info.compressed));
            let mut entry = zip::read::read_zipfile_from_stream(&mut stream)?
                .ok_or_else(|| fail("missing ZIP component"))?;
            let name = info.name.clone();
            let count = Rc::new(Counter::new(0));
            let total_left = limits
                .total_bytes
                .checked_sub(stage.stats.unpacked_bytes)
                .ok_or_else(|| fail("unpacked book byte limit"))?;
            let actual_limit = limits
                .component_bytes
                .min(total_left)
                .min(info.compressed.max(1).saturating_mul(limits.ratio));
            let counted = Counted {
                inner: &mut entry,
                count: count.clone(),
                max: actual_limit,
                cancel,
            };
            if name.ends_with(".xml") || name.ends_with(".rels") {
                parse_component(&mut stage, &name, counted, limits, cancel, &mut progress)
                    .map_err(|e| fail(&format!("{name}: {e}")))?;
            } else {
                let mut input = counted;
                io::copy(&mut input, &mut io::sink())?;
            }
            if count.get() != info.unpacked {
                return Err(fail("ZIP actual uncompressed size mismatch"));
            }
            stage.stats.unpacked_bytes += count.get();
            stage.stats.components.push((name, count.get()));
            progress(&stage.stats);
        }
        check_cancel(cancel)?;
        if fingerprint(source, cancel, limits.archive_bytes)? != stage.source_hash {
            return Err(fail("source changed during ingestion"));
        }
        let missing:i64=stage.db.query_row("SELECT COUNT(*) FROM cells c LEFT JOIN strings s ON s.id=CAST(c.value AS INTEGER) WHERE c.kind='s' AND (s.id IS NULL OR CAST(c.value AS INTEGER)<0)",[],|r|r.get(0))?;
        if missing != 0 {
            return Err(fail("missing shared string reference"));
        }
        stage.db.execute(
            "INSERT INTO manifest VALUES('source_hash',?1)",
            [&stage.source_hash],
        )?;
        stage
            .db
            .execute("INSERT INTO manifest VALUES('complete','1')", [])?;
        stage.db.execute_batch("COMMIT;")?;
        Ok(stage)
    }
}
impl Stage<'_> {
    pub fn path(&self) -> &Path {
        self.dir.path()
    }
    /// Keyset page: the cursor is row * 2048 + column, zero-based.
    pub fn page(&self, sheet: &str, after: i64, limit: usize) -> Result<Vec<Cell>> {
        if !(1..=100).contains(&limit) || after < 0 {
            return Err(fail("invalid preview page"));
        }
        let mut stmt=self.db.prepare("SELECT c.address,CASE WHEN c.kind='s' THEN s.value ELSE c.value END,c.kind,c.style,c.formula FROM cells c LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER) WHERE c.sheet=?1 AND (c.row_num,c.col_num)>(?2,?3) ORDER BY c.row_num,c.col_num LIMIT ?4")?;
        let rows = stmt.query_map(
            params![sheet, after / 2048 + 1, after % 2048, limit as i64],
            |r| {
                Ok(Cell {
                    address: r.get(0)?,
                    value: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    kind: r.get(2)?,
                    style: r.get(3)?,
                    formula: r.get(4)?,
                })
            },
        )?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}
fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(fail("cancelled"))
    } else {
        Ok(())
    }
}
pub fn fingerprint(path: &Path, cancel: &AtomicBool, max: u64) -> Result<String> {
    hash_reader(&mut File::open(path)?, cancel, max)
}
fn hash_reader(file: &mut impl Read, cancel: &AtomicBool, max: u64) -> Result<String> {
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    let mut bytes = 0u64;
    loop {
        check_cancel(cancel)?;
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > max {
            return Err(fail("source byte limit while hashing"));
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
struct Counted<'a, R> {
    inner: R,
    count: Rc<Counter<u64>>,
    max: u64,
    cancel: &'a AtomicBool,
}
impl<R: Read> Read for Counted<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(io::Error::other("cancelled"));
        }
        let max = buf
            .len()
            .min((self.max.saturating_sub(self.count.get()) + 1) as usize);
        let n = self.inner.read(&mut buf[..max])?;
        self.count.set(self.count.get() + n as u64);
        if self.count.get() > self.max {
            return Err(io::Error::other("actual decompression byte/ratio limit"));
        }
        Ok(n)
    }
}
// Bound a single XML event even when quick-xml would otherwise grow its buffer.
struct EventBudget<R> {
    inner: R,
    used: u64,
    max: u64,
}
impl<R: BufRead> BufRead for EventBudget<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.used >= self.max {
            return Err(io::Error::other("XML token byte limit"));
        }
        let left = (self.max - self.used) as usize;
        let b = self.inner.fill_buf()?;
        Ok(&b[..b.len().min(left)])
    }
    fn consume(&mut self, n: usize) {
        self.used += n as u64;
        self.inner.consume(n)
    }
}
impl<R: BufRead> Read for EventBudget<R> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        let a = self.fill_buf()?;
        let n = a.len().min(b.len());
        b[..n].copy_from_slice(&a[..n]);
        self.consume(n);
        Ok(n)
    }
}
fn attributes(e: &BytesStart<'_>) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for a in e.attributes() {
        let a = a?;
        out.push((
            String::from_utf8(a.key.as_ref().to_vec())?,
            a.unescape_value()?.into_owned(),
        ));
    }
    Ok(out)
}
fn address(s: &str, l: Limits) -> Result<(u32, u32)> {
    let mut col = 0u32;
    let mut row = 0u32;
    let mut digits = false;
    for b in s.bytes() {
        if b.is_ascii_uppercase() && !digits {
            col = col
                .checked_mul(26)
                .and_then(|n| n.checked_add((b - b'A' + 1) as u32))
                .ok_or_else(|| fail("invalid cell address"))?;
        } else if b.is_ascii_digit() {
            digits = true;
            row = row
                .checked_mul(10)
                .and_then(|n| n.checked_add((b - b'0') as u32))
                .ok_or_else(|| fail("invalid cell address"))?;
        } else {
            return Err(fail("invalid cell address"));
        }
    }
    if row == 0 || col == 0 || row > l.rows || col > l.columns {
        return Err(fail("cell coordinate limit"));
    }
    Ok((row, col))
}
struct Pending {
    address: String,
    row: u32,
    col: u32,
    kind: String,
    style: String,
    value: String,
    formula: bool,
}
fn parse_component<R: Read>(
    stage: &mut Stage<'_>,
    name: &str,
    input: R,
    l: Limits,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(&Stats),
) -> Result<()> {
    let mut reader = NsReader::from_reader(EventBudget {
        inner: BufReader::with_capacity(32768, input),
        used: 0,
        max: l.token_bytes,
    });
    let mut buf = Vec::new();
    let mut cell: Option<Pending> = None;
    let mut text = String::new();
    let mut shared = false;
    let mut capture = false;
    let mut seq = 0i64;
    let mut depth = 0usize;
    let mut roots = 0usize;
    reader.config_mut().check_comments = true;
    let mut sheet = false;
    let mut tags: Vec<String> = Vec::new();
    let mut merge_count = 0;
    let strings = name == "xl/sharedStrings.xml";
    loop {
        check_cancel(cancel)?;
        reader.get_mut().used = 0;
        let event = reader.read_event_into(&mut buf)?;
        match &event {
            Event::DocType(_) => return Err(fail("DTD/XXE forbidden")),
            Event::Start(e) | Event::Empty(e) => {
                let empty = matches!(event, Event::Empty(_));
                if depth == 0 {
                    roots += 1;
                    if roots > 1 {
                        return Err(fail("multiple XML roots"));
                    }
                }
                if !empty {
                    depth += 1;
                    if depth > 64 {
                        return Err(fail("XML nesting limit"));
                    }
                }
                let local = e.local_name();
                let tag = std::str::from_utf8(local.as_ref())?;
                if depth == if empty { 0 } else { 1 } {
                    sheet = tag == "worksheet";
                }
                if matches!(
                    reader.resolve_element(e.name()).0,
                    quick_xml::name::ResolveResult::Unknown(_)
                ) {
                    return Err(fail("undefined XML element prefix"));
                }
                let attrs = attributes(e)?;
                for (key, _) in &attrs {
                    if key != "xmlns"
                        && !key.starts_with("xmlns:")
                        && matches!(
                            reader
                                .resolve_attribute(quick_xml::name::QName(key.as_bytes()))
                                .0,
                            quick_xml::name::ResolveResult::Unknown(_)
                        )
                    {
                        return Err(fail("undefined XML attribute prefix"));
                    }
                }

                let get = |key: &str| {
                    attrs
                        .iter()
                        .find(|(k, _)| k == key)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default()
                };
                if sheet && tag == "c" {
                    if cell.is_some() {
                        return Err(fail("nested worksheet cell"));
                    }
                    let a = get("r");
                    let (row, col) = address(&a, l)?;
                    cell = Some(Pending {
                        address: a,
                        row,
                        col,
                        kind: get("t"),
                        style: get("s"),
                        value: String::new(),
                        formula: false,
                    });
                    if empty {
                        write_cell(stage, name, cell.take().unwrap(), l, progress)?;
                    }
                }
                if sheet && tag == "row" {
                    let r = get("r");
                    if !r.is_empty() {
                        let n = r.parse::<u32>()?;
                        if n == 0 || n > l.rows {
                            return Err(fail("row coordinate limit"));
                        }
                    }
                }
                if strings && tag == "si" {
                    shared = true;
                    text.clear();
                    if empty {
                        stage.db.execute(
                            "INSERT INTO strings VALUES(?1,'')",
                            [stage.stats.strings as i64],
                        )?;
                        stage.stats.strings += 1;
                        shared = false;
                    }
                }
                if tag == "v" || tag == "t" {
                    capture = !empty;
                }
                if tag == "f" {
                    if let Some(c) = cell.as_mut() {
                        c.formula = true;
                    }
                }
                if !sheet && !strings || tag == "mergeCell" {
                    if tag == "mergeCell" {
                        merge_count += 1;
                        if merge_count > 20000 {
                            return Err(fail("worksheet merge limit"));
                        }
                        let range = get("ref");
                        let ends = range.split(':').collect::<Vec<_>>();
                        if ends.len() != 2 {
                            return Err(fail("invalid merge range"));
                        }
                        let (r, c) = address(ends[0], l)?;
                        let (er, ec) = address(ends[1], l)?;
                        if er < r || ec < c {
                            return Err(fail("reversed merge range"));
                        }
                        stage.db.execute(
                            "INSERT INTO merges VALUES(?1,?2,?3,?4,?5,?6)",
                            params![name, merge_count, r, c, er, ec],
                        )?;
                    }
                    stage.db.execute(
                        "INSERT INTO nodes VALUES(?1,?2,?3,?4)",
                        params![name, seq, tag, tags.last().map(String::as_str)],
                    )?;
                    for (key, value) in &attrs {
                        let uri = if key == "xmlns" || key.starts_with("xmlns:") {
                            Some("http://www.w3.org/2000/xmlns/".to_owned())
                        } else {
                            match reader
                                .resolve_attribute(quick_xml::name::QName(key.as_bytes()))
                                .0
                            {
                                quick_xml::name::ResolveResult::Bound(ns) => {
                                    Some(std::str::from_utf8(ns.as_ref())?.to_owned())
                                }
                                quick_xml::name::ResolveResult::Unbound => None,
                                quick_xml::name::ResolveResult::Unknown(_) => {
                                    return Err(fail("undefined XML attribute prefix"))
                                }
                            }
                        };
                        stage.db.execute(
                            "INSERT INTO attributes VALUES(?1,?2,?3,?4,?5)",
                            params![name, seq, key, value, uri],
                        )?;
                    }
                    stage.db.execute(
                        "INSERT INTO metadata VALUES(?1,?2,?3,?4,?5)",
                        params![
                            name,
                            seq,
                            if empty { "empty" } else { "start" },
                            tag,
                            format!("{attrs:?}")
                        ],
                    )?;
                    seq += 1;
                }
                if !empty {
                    tags.push(tag.into());
                }
            }
            Event::End(e) => {
                if depth == 0 {
                    return Err(fail("unbalanced XML"));
                }
                depth -= 1;
                tags.pop();
                match e.local_name().as_ref() {
                    b"c" if sheet => {
                        let c = cell.take().ok_or_else(|| fail("invalid cell end"))?;
                        write_cell(stage, name, c, l, progress)?;
                    }
                    b"si" if strings => {
                        stage.db.execute(
                            "INSERT INTO strings VALUES(?1,?2)",
                            params![stage.stats.strings as i64, text],
                        )?;
                        stage.stats.strings += 1;
                        shared = false;
                    }
                    b"t" | b"v" => capture = false,
                    _ => {}
                }
            }
            Event::Text(e) => {
                let value = e.decode()?;
                if depth == 0 && !value.trim().is_empty() {
                    return Err(fail("text outside XML root"));
                }
                if capture {
                    append_value(&mut cell, &mut text, shared, &value, l)?;
                }
            }
            Event::GeneralRef(e) => {
                let raw = format!("&{};", e.decode()?);
                let value = quick_xml::escape::unescape(&raw)?;
                if depth == 0 {
                    return Err(fail("entity outside XML root"));
                }
                if capture {
                    append_value(&mut cell, &mut text, shared, &value, l)?;
                }
            }
            Event::CData(e) => {
                if depth == 0 {
                    return Err(fail("CDATA outside XML root"));
                }
                if capture {
                    append_value(&mut cell, &mut text, shared, &e.decode()?, l)?;
                }
            }
            Event::Eof => {
                if roots != 1 || depth != 0 || cell.is_some() || shared {
                    return Err(fail("truncated XML"));
                }
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}
fn append_value(
    cell: &mut Option<Pending>,
    text: &mut String,
    shared: bool,
    value: &str,
    l: Limits,
) -> Result<()> {
    if shared {
        if text.len() + value.len() > l.text_bytes {
            return Err(fail("shared string byte limit"));
        }
        text.push_str(value);
    } else if let Some(c) = cell {
        if c.value.len() + value.len() > l.text_bytes {
            return Err(fail("cell text byte limit"));
        }
        c.value.push_str(value);
    }
    Ok(())
}
fn write_cell(
    stage: &mut Stage<'_>,
    sheet: &str,
    c: Pending,
    l: Limits,
    progress: &mut impl FnMut(&Stats),
) -> Result<()> {
    if stage.stats.cells >= l.cells {
        return Err(fail("native processed cell budget"));
    }
    if c.kind == "s" {
        c.value
            .parse::<i64>()
            .map_err(|_| fail("invalid shared string index"))?;
    }
    stage.db.execute(
        "INSERT INTO cells VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![sheet, c.row, c.col, c.address, c.kind, c.style, c.value, c.formula],
    )?;
    stage.stats.cells += 1;
    if c.value.trim().is_empty() && !c.formula {
        stage.stats.empty_cells += 1;
    }
    if stage.stats.cells % 2048 == 0 {
        stage.db.execute_batch("COMMIT; BEGIN;")?;
        progress(&stage.stats);
    }
    Ok(())
}
