//! Shared SQLite presentation functions and revision-bound pagination.
use super::{err, AnalysisScope, Connection, Deserialize, Serialize, Value};
use rusqlite::functions::FunctionFlags;
use sha2::{Digest, Sha256};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    fingerprint: String,
    offset: i64,
}

pub(super) fn normalized(value: &str) -> String {
    // Same order as the classic normalized(): trailing punctuation is removed
    // before whitespace is collapsed and trimmed.
    value
        .to_lowercase()
        .trim_end_matches(['.', ',', ';', ':'])
        .split(|c: char| c.is_whitespace() || c == '\u{feff}')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
pub(super) fn presentation_functions(c: &Connection) -> Result<(), String> {
    c.create_scalar_function(
        "classic_normalized",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| Ok(normalized(&ctx.get::<String>(0)?)),
    )
    .map_err(err)?;
    let locale: icu_provider::DataLocale = "ru".parse().map_err(err)?;
    let collator = icu_collator::Collator::try_new(&locale, Default::default()).map_err(err)?;
    c.create_collation("classic_ru", move |a, b| collator.compare(a, b))
        .map_err(err)
}
pub(super) fn fingerprint(
    c: &Connection,
    revision: i64,
    s: &AnalysisScope,
    value: Value,
) -> Result<String, String> {
    let mut hash = Sha256::new();
    hash.update(serde_json::to_vec(&(revision, &s.report_type, value)).map_err(err)?);
    for id in [Some(s.organizations.as_str()), s.names.as_deref()] {
        hash.update(if id.is_some() {
            b"selected".as_slice()
        } else {
            b"all".as_slice()
        });
        if let Some(id) = id {
            let mut stmt = c
                .prepare("SELECT item FROM analysis_filter_items WHERE id=?1 ORDER BY ordinal")
                .map_err(err)?;
            let mut rows = stmt.query([id]).map_err(err)?;
            while let Some(row) = rows.next().map_err(err)? {
                let item: String = row.get(0).map_err(err)?;
                hash.update((item.len() as u64).to_le_bytes());
                hash.update(item.as_bytes());
            }
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub(super) fn page_offset(after: Option<&str>, fingerprint: &str) -> Result<i64, String> {
    let Some(after) = after else { return Ok(0) };
    if after.len() > 512 {
        return Err("Invalid analysis cursor".into());
    }
    let cursor: Cursor =
        serde_json::from_str(after).map_err(|_| "Invalid analysis cursor".to_string())?;
    if cursor.fingerprint != fingerprint || cursor.offset < 0 || cursor.offset > i64::from(i32::MAX)
    {
        return Err("Analysis cursor belongs to another query or revision".into());
    }
    Ok(cursor.offset)
}
pub(super) fn next_cursor(fingerprint: &str, offset: i64) -> Result<String, String> {
    serde_json::to_string(&Cursor {
        fingerprint: fingerprint.into(),
        offset,
    })
    .map_err(err)
}
