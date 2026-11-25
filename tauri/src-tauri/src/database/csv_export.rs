//! Shared formula-safe CSV encoding and atomic file publication.
use super::err;
use std::{fs, io::Write, path::Path, time::UNIX_EPOCH};

pub(super) enum Cell<'a> {
    Text(&'a str),
    Number(Option<f64>),
}
pub(super) fn number_text(n: f64) -> String {
    if n == 0. {
        return "0".into();
    }
    let a = n.abs();
    if (1e-6..1e21).contains(&a) {
        return n.to_string().replace('.', ",");
    }
    let raw = format!("{n:e}");
    let (m, e) = raw.split_once('e').unwrap();
    let exponent: i32 = e.parse().unwrap();
    format!(
        "{}e{}{exponent}",
        m.replace('.', ","),
        if exponent >= 0 { "+" } else { "" }
    )
}
pub(super) fn csv_record(w: &mut impl Write, cells: &[Cell<'_>]) -> Result<(), String> {
    for (i, c) in cells.iter().enumerate() {
        if i > 0 {
            w.write_all(b";").map_err(err)?;
        }
        w.write_all(b"\"").map_err(err)?;
        let text = match c {
            Cell::Text(s) => {
                let leading = s.trim_start_matches(|c: char| {
                    c <= '\u{001f}'
                        || matches!(
                            c,
                            ' ' | '\u{00a0}' | '\u{1680}' | '\u{2000}'
                                ..='\u{200a}'
                                    | '\u{2028}'
                                    | '\u{2029}'
                                    | '\u{202f}'
                                    | '\u{205f}'
                                    | '\u{3000}'
                                    | '\u{feff}'
                        )
                });
                if leading.starts_with(['=', '+', '-', '@']) {
                    format!("'{s}")
                } else {
                    s.to_string()
                }
            }
            Cell::Number(Some(n)) if n.is_finite() => number_text(*n),
            Cell::Number(Some(_)) => return Err("Analysis numeric overflow".into()),
            Cell::Number(None) => String::new(),
        };
        for part in text.split_inclusive('"') {
            w.write_all(part.as_bytes()).map_err(err)?;
            if part.ends_with('"') {
                w.write_all(b"\"").map_err(err)?;
            }
        }
        w.write_all(b"\"").map_err(err)?;
    }
    w.write_all(b"\r\n").map_err(err)
}
pub(super) fn export(
    destination: &Path,
    write: impl FnOnce(&mut std::io::BufWriter<fs::File>) -> Result<(), String>,
) -> Result<(), String> {
    export_before(destination, write, || {})
}
pub(super) fn export_before(
    destination: &Path,
    write: impl FnOnce(&mut std::io::BufWriter<fs::File>) -> Result<(), String>,
    before: impl FnOnce(),
) -> Result<(), String> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err("Export destination already exists".into());
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stamp = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(err)?
        .as_nanos();
    let mut staging = None;
    for attempt in 0..100 {
        let path = parent.join(format!(
            ".analiz-export-{}-{stamp}-{attempt}.csv",
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
        {
            Ok(f) => {
                staging = Some((path, f));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(err(e)),
        }
    }
    let (path, file) = staging.ok_or("Could not reserve export staging file")?;
    let result = (|| {
        let mut w = std::io::BufWriter::with_capacity(8192, file);
        w.write_all(b"\xef\xbb\xbf").map_err(err)?;
        write(&mut w)?;
        w.flush().map_err(err)?;
        w.get_ref().sync_all().map_err(err)?;
        drop(w);
        before();
        fs::hard_link(&path, destination).map_err(err)
    })();
    let _ = fs::remove_file(&path);
    result
}
