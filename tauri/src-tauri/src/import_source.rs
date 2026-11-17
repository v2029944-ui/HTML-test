//! Disk-staged whole-source ownership discovery. Never materialize a report Value.
use super::*;
use serde::de::{self, IgnoredAny, MapAccess, Visitor};
use serde::Deserialize;
use std::{
    fmt,
    io::{self, BufReader},
    path::{Component, Path, PathBuf},
};

#[derive(Default)]
struct SourceMetadata {
    name: Option<String>,
    folder: Option<String>,
    mode: Option<String>,
    has_hash: bool,
}
impl<'de> Deserialize<'de> for SourceMetadata {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct MetadataVisitor;
        impl<'de> Visitor<'de> for MetadataVisitor {
            type Value = SourceMetadata;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a report object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut out = SourceMetadata::default();
                let mut seen = [false; 4];
                while let Some(key) = map.next_key::<String>()? {
                    let index = match key.as_str() {
                        "desktopSource" => Some(0),
                        "desktopFolder" => Some(1),
                        "sourceImportMode" => Some(2),
                        "sourceHash" => Some(3),
                        _ => None,
                    };
                    if let Some(i) = index {
                        if seen[i] {
                            return Err(de::Error::custom("duplicate source identity"));
                        }
                        seen[i] = true;
                        match i {
                            0 => out.name = map.next_value()?,
                            1 => out.folder = map.next_value()?,
                            2 => out.mode = map.next_value()?,
                            _ => {
                                map.next_value::<IgnoredAny>()?;
                                out.has_hash = true;
                            }
                        }
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(out)
            }
        }
        d.deserialize_map(MetadataVisitor)
    }
}
struct CancelRead<'a, R> {
    inner: R,
    cancel: &'a AtomicBool,
}
impl<R: Read> Read for CancelRead<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        cancelled(self.cancel).map_err(io::Error::other)?;
        self.inner.read(buf)
    }
}
fn canonical_folder(folder: &str) -> Result<PathBuf, String> {
    let path = Path::new(folder);
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(
            "Unknown or malformed legacy folder identity blocks source replacement.".into(),
        );
    }
    Ok(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
}
fn legacy_owner(
    conn: &Connection,
    rowid: i64,
    prepared: &PreparedImport<'_>,
    cancel: &AtomicBool,
) -> Result<bool, String> {
    let blob = conn
        .blob_open("main", "reports", "json", rowid, true)
        .map_err(err)?;
    let reader = read_model::TokenBounded::new(CancelRead {
        inner: blob,
        cancel,
    });
    let mut de = serde_json::Deserializer::from_reader(BufReader::with_capacity(8192, reader));
    let metadata = SourceMetadata::deserialize(&mut de).map_err(|e| {
        format!("Unknown or malformed legacy folder identity blocks source replacement: {e}")
    })?;
    de.end().map_err(err)?;
    // C1 carried desktopFolder + sourceHash even for manual imports. Never adopt it.
    if metadata.mode.as_deref() == Some("manual") || metadata.mode.is_none() && metadata.has_hash {
        return Ok(false);
    }
    if metadata.mode.as_deref().is_some_and(|m| m != "folder")
        || metadata.name.as_deref() != Some(prepared.source_name())
    {
        return Err(
            "Unknown or malformed legacy folder identity blocks source replacement.".into(),
        );
    }
    let folder = metadata
        .folder
        .ok_or("Unknown legacy folder identity blocks source replacement.")?;
    Ok(canonical_folder(&folder)? == Path::new(prepared.source_folder()))
}
/// Build a private temporary target table on every snapshot. Preview never updates live ownership.
pub(super) fn source_plan(
    conn: &Connection,
    prepared: &PreparedImport<'_>,
    cancel: &AtomicBool,
) -> Result<String, String> {
    conn.execute_batch("CREATE TEMP TABLE IF NOT EXISTS import_source_plan(report_id TEXT PRIMARY KEY) WITHOUT ROWID; DELETE FROM temp.import_source_plan;").map_err(err)?;
    let mut hash = Sha256::new();
    if prepared.mode() == ImportMode::Manual {
        return Ok(format!("{:x}", hash.finalize()));
    }
    let mut cursor = i64::MIN;
    let mut first = true;
    loop {
        cancelled(cancel)?;
        // Rowid chunks bound SQL work even when almost none of history matches this filename.
        let mut stmt = conn.prepare("SELECT rowid, CASE WHEN octet_length(id)<=65536 THEN id END, CASE WHEN octet_length(source_name)<=65536 THEN source_name END, COALESCE(octet_length(source_name)>65536,0) FROM reports WHERE rowid>=?1 AND (?2 OR rowid<>?1) ORDER BY rowid LIMIT 100").map_err(err)?;
        let mut rows = stmt.query(params![cursor, first]).map_err(err)?;
        first = false;
        let mut seen = 0;
        while let Some(row) = rows.next().map_err(err)? {
            cancelled(cancel)?;
            cursor = row.get(0).map_err(err)?;
            seen += 1;
            if row.get::<_, bool>(3).map_err(err)? {
                return Err("Report metadata exceeds 64 KiB.".into());
            }
            let name: Option<String> = row.get(2).map_err(err)?;
            if name.as_deref() != Some(prepared.source_name()) {
                continue;
            }
            let id: String = row
                .get::<_, Option<String>>(1)
                .map_err(err)?
                .ok_or("Report metadata exceeds 64 KiB.")?;
            let indexed: Option<(String,String,String)> = conn.query_row("SELECT CASE WHEN octet_length(source_name)<=65536 THEN source_name END, CASE WHEN octet_length(folder)<=65536 THEN folder END, CASE WHEN octet_length(mode)<=65536 THEN mode END FROM report_source_ownership WHERE report_id=?1", [&id], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(err)?;
            let owned = if let Some((name, folder, mode)) = indexed {
                if name != prepared.source_name() {
                    return Err("Source ownership index is inconsistent.".into());
                }
                match mode.as_str() {
                    "manual" => false,
                    "folder" => canonical_folder(&folder)? == Path::new(prepared.source_folder()),
                    _ => return Err("Unknown source ownership mode.".into()),
                }
            } else {
                legacy_owner(conn, cursor, prepared, cancel)?
            };
            if owned {
                conn.execute("INSERT INTO temp.import_source_plan VALUES(?1)", [&id])
                    .map_err(err)?;
                hash.update((id.len() as u64).to_le_bytes());
                hash.update(id.as_bytes());
            }
        }
        if seen < 100 {
            break;
        }
    }
    cancelled(cancel)?;
    Ok(format!("{:x}", hash.finalize()))
}
