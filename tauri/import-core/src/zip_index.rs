//! Strict ZIP32 directory validation. The zip crate is used only for an individual
//! bounded component's decompression/CRC, never for footer discovery or indexing.
use crate::{check_cancel, fail, Limits, Result};
use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
    sync::atomic::AtomicBool,
};
pub(crate) struct Entry {
    pub name: String,
    pub compressed: u64,
    pub unpacked: u64,
    pub data: u64,
    header: Vec<u8>,
    local: u64,
    end: u64,
}
impl Entry {
    pub fn header(&self) -> &[u8] {
        &self.header
    }
}
pub(crate) struct Index {
    pub file: File,
    pub entries: Vec<Entry>,
}
fn u16at(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes(b[p..p + 2].try_into().unwrap())
}
fn u32at(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}
fn extra(file: &mut File, len: u64) -> Result<()> {
    let end = file
        .stream_position()?
        .checked_add(len)
        .ok_or_else(|| fail("ZIP preflight extra overflow"))?;
    while file.stream_position()? < end {
        let mut h = [0u8; 4];
        if end - file.stream_position()? < 4 {
            return Err(fail("ZIP preflight malformed extra field"));
        }
        file.read_exact(&mut h)?;
        if u16at(&h, 0) == 1 {
            return Err(fail("ZIP64 extra field unsupported"));
        }
        let next = file.stream_position()? + u16at(&h, 2) as u64;
        if next > end {
            return Err(fail("ZIP preflight extra field length"));
        }
        file.seek(SeekFrom::Start(next))?;
    }
    Ok(())
}
impl Index {
    pub fn open(mut file: File, l: Limits, cancel: &AtomicBool) -> Result<Self> {
        let len = file.metadata()?.len();
        if len > l.archive_bytes {
            return Err(fail("ZIP preflight archive size"));
        }
        let n = len.min(65557) as usize;
        file.seek(SeekFrom::End(-(n as i64)))?;
        let mut tail = vec![0u8; n];
        file.read_exact(&mut tail)?;
        let p = (0..n.saturating_sub(21))
            .rev()
            .find(|&p| {
                tail[p..].starts_with(b"PK\x05\x06") && p + 22 + u16at(&tail, p + 20) as usize == n
            })
            .ok_or_else(|| fail("ZIP preflight missing terminal footer"))?;
        let end = len - n as u64 + p as u64;
        let h = &tail[p..];
        let count = u16at(h, 10) as usize;
        let offset = u32at(h, 16) as u64;
        let size = u32at(h, 12) as u64;
        if count == 65535
            || size == u32::MAX as u64
            || offset == u32::MAX as u64
            || (p >= 20 && tail[p - 20..p - 16] == *b"PK\x06\x07")
        {
            return Err(fail("ZIP64 unsupported"));
        }
        if u16at(h, 4) != 0
            || u16at(h, 6) != 0
            || u16at(h, 8) as usize != count
            || count > l.entries
            || offset.checked_add(size) != Some(end)
        {
            return Err(fail("ZIP preflight directory bounds/count"));
        }
        let mut entries = Vec::with_capacity(count);
        let mut names = HashSet::new();
        file.seek(SeekFrom::Start(offset))?;
        for _ in 0..count {
            check_cancel(cancel)?;
            let pos = file.stream_position()?;
            if pos + 46 > end {
                return Err(fail("ZIP preflight central header bounds"));
            }
            let mut h = [0u8; 46];
            file.read_exact(&mut h)?;
            if &h[..4] != b"PK\x01\x02" {
                return Err(fail("ZIP preflight central header signature"));
            }
            let flags = u16at(&h, 8);
            let method = u16at(&h, 10);
            let name_len = u16at(&h, 28) as usize;
            let extra_len = u16at(&h, 30) as u64;
            let comment_len = u16at(&h, 32) as u64;
            let local = u32at(&h, 42) as u64;
            let compressed = u32at(&h, 20) as u64;
            let unpacked = u32at(&h, 24) as u64;
            if [local, compressed, unpacked].contains(&(u32::MAX as u64)) {
                return Err(fail("ZIP64 unsupported"));
            }
            let next = pos + 46 + name_len as u64 + extra_len + comment_len;
            if name_len == 0
                || name_len > 2048
                || next > end
                || local + 30 > offset
                || u16at(&h, 34) != 0
                || flags & 0x41 != 0
                || ![0, 8].contains(&method)
                || unpacked > l.component_bytes
            {
                return Err(fail("ZIP preflight unsupported or oversized entry"));
            }
            let mut bytes = vec![0; name_len];
            file.read_exact(&mut bytes)?;
            let name = String::from_utf8(bytes)?;
            if name.starts_with('/')
                || name.contains(['\\', ':'])
                || name.chars().any(char::is_control)
                || name
                    .trim_end_matches('/')
                    .split('/')
                    .any(|s| s.is_empty() || s == "." || s == "..")
                || !names.insert(name.clone())
            {
                return Err(fail("ZIP preflight unsafe or duplicate path"));
            }
            extra(&mut file, extra_len)?;
            file.seek(SeekFrom::Start(local))?;
            let mut header = vec![0u8; 30];
            file.read_exact(&mut header)?;
            if &header[..4] != b"PK\x03\x04"
                || u16at(&header, 6) != flags
                || u16at(&header, 8) != method
                || u16at(&header, 26) as usize != name_len
            {
                return Err(fail("ZIP preflight local/central mismatch"));
            }
            let local_extra = u16at(&header, 28) as u64;
            let data = local + 30 + name_len as u64 + local_extra;
            if data
                .checked_add(compressed)
                .is_none_or(|last| last > offset)
            {
                return Err(fail("ZIP preflight payload bounds"));
            }
            let mut local_name = vec![0; name_len];
            file.read_exact(&mut local_name)?;
            if local_name != name.as_bytes() {
                return Err(fail("ZIP preflight local name mismatch"));
            }
            extra(&mut file, local_extra)?;
            if flags & 8 == 0
                && (u32at(&header, 14) != u32at(&h, 16)
                    || u32at(&header, 18) as u64 != compressed
                    || u32at(&header, 22) as u64 != unpacked)
            {
                return Err(fail("ZIP preflight local size/CRC mismatch"));
            }
            let mut payload_end = data + compressed;
            if flags & 8 != 0 {
                if payload_end + 12 > offset {
                    return Err(fail("ZIP preflight missing data descriptor"));
                }
                file.seek(SeekFrom::Start(payload_end))?;
                let mut descriptor = [0u8; 16];
                file.read_exact(&mut descriptor[..12])?;
                let expected = &h[16..28];
                if descriptor[..12] == *expected {
                    payload_end += 12;
                } else if descriptor[..4] == *b"PK\x07\x08" && payload_end + 16 <= offset {
                    file.read_exact(&mut descriptor[12..16])?;
                    if descriptor[4..16] != *expected {
                        return Err(fail("ZIP preflight data descriptor mismatch"));
                    }
                    payload_end += 16;
                } else {
                    return Err(fail("ZIP preflight data descriptor mismatch"));
                }
            }
            // Supply only this validated component to the library's stream reader.
            // Descriptor values come from the authoritative central directory.
            header[6..8].copy_from_slice(&((flags & !8) | 0x800).to_le_bytes());
            header[14..18].copy_from_slice(&h[16..20]);
            header[18..22].copy_from_slice(&h[20..24]);
            header[22..26].copy_from_slice(&h[24..28]);
            header[28..30].copy_from_slice(&0u16.to_le_bytes());
            header.extend_from_slice(name.as_bytes());
            entries.push(Entry {
                name,
                compressed,
                unpacked,
                data,
                header,
                local,
                end: payload_end,
            });
            file.seek(SeekFrom::Start(next))?;
        }
        if file.stream_position()? != end {
            return Err(fail("ZIP preflight central directory size mismatch"));
        }
        // Reject overlapping payloads, including aliasing of a local header.
        let mut ranges = entries.iter().map(|e| (e.local, e.end)).collect::<Vec<_>>();
        ranges.sort_unstable();
        if ranges.windows(2).any(|r| r[0].1 > r[1].0) {
            return Err(fail("ZIP preflight overlapping entries"));
        }
        Ok(Self { file, entries })
    }
}
