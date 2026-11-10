//! Validate ZIP32 framing before the library allocates its central index.
//! ZIP64, spanning, prefix archives and non-UTF8 names are explicitly unsupported.

use std::collections::HashSet;

use crate::{Error, ErrorKind, Limits};

#[derive(Debug)]
pub(crate) struct Entry {
    pub name: String,
    pub method: u16,
    pub flags: u16,
    pub crc32: u32,
    pub compressed_size: u64,
    pub size: u64,
    pub data_start: usize,
    pub central_start: usize,
}

fn bad(code: &'static str, message: &str, offset: usize) -> Error {
    Error::new(ErrorKind::Format, code, "container", message).offset(offset)
}

fn unsupported(message: &str, offset: usize) -> Error {
    Error::new(
        ErrorKind::Unsupported,
        "zip.unsupported_layout",
        "container",
        message,
    )
    .offset(offset)
}

fn bytes(data: &[u8], at: usize, count: usize) -> Result<&[u8], Error> {
    data.get(
        at..at
            .checked_add(count)
            .ok_or_else(|| bad("zip.bounds", "ZIP offset overflow", at))?,
    )
    .ok_or_else(|| bad("zip.truncated", "truncated ZIP record", at))
}
fn u16_at(data: &[u8], at: usize) -> Result<u16, Error> {
    let b = bytes(data, at, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}
fn u32_at(data: &[u8], at: usize) -> Result<u32, Error> {
    let b = bytes(data, at, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

pub(crate) fn parse(data: &[u8], limits: Limits) -> Result<Vec<Entry>, Error> {
    let lower = data.len().saturating_sub(65535 + 22);
    let mut ends = Vec::new();
    for at in lower..data.len().saturating_sub(21) {
        if data.get(at..at + 4) == Some(b"PK\x05\x06")
            && at + 22 + u16_at(data, at + 20)? as usize == data.len()
        {
            ends.push(at);
        }
    }
    if ends.len() != 1 {
        return Err(bad(
            "zip.end_record",
            "missing or ambiguous end-of-central-directory record",
            lower,
        ));
    }
    let end = ends[0];
    if u16_at(data, end + 4)? != 0
        || u16_at(data, end + 6)? != 0
        || u16_at(data, end + 8)? != u16_at(data, end + 10)?
    {
        return Err(unsupported("multi-disk ZIP is unsupported", end));
    }
    let count = u16_at(data, end + 10)?;
    let central_size = u32_at(data, end + 12)?;
    let central = u32_at(data, end + 16)?;
    if count == u16::MAX || central_size == u32::MAX || central == u32::MAX {
        return Err(unsupported("ZIP64 is not yet supported", end));
    }
    if u64::from(count) > limits.max_entries() {
        return Err(Error::new(
            ErrorKind::ResourceLimit,
            "limit.entries",
            "container",
            "member count exceeds max_entries",
        )
        .offset(end));
    }
    if u64::from(central) + u64::from(central_size) != end as u64 {
        return Err(bad(
            "zip.central_bounds",
            "central directory bounds do not match the end record",
            end,
        ));
    }
    let mut at = central as usize;
    let mut entries = Vec::new();
    let mut names = HashSet::new();
    let mut ranges = Vec::new();
    for _ in 0..count {
        if bytes(data, at, 4)? != b"PK\x01\x02" {
            return Err(bad(
                "zip.central_header",
                "invalid central directory header",
                at,
            ));
        }
        bytes(data, at, 46)?;
        let name_len = u16_at(data, at + 28)? as usize;
        let extra_len = u16_at(data, at + 30)? as usize;
        let comment_len = u16_at(data, at + 32)? as usize;
        let raw_name = bytes(data, at + 46, name_len)?;
        let name = std::str::from_utf8(raw_name)
            .map_err(|_| unsupported("non-UTF8 ZIP member name", at))?
            .to_owned();
        validate_name(&name).map_err(|error| error.offset(at).member(&name))?;
        if !names.insert(name.clone()) {
            return Err(bad("zip.duplicate_member", "duplicate ZIP member name", at).member(&name));
        }
        let flags = u16_at(data, at + 8)?;
        if flags & 0x800 == 0 && !raw_name.is_ascii() {
            return Err(unsupported("non-ASCII names require the UTF-8 flag", at).member(&name));
        }
        let method = u16_at(data, at + 10)?;
        let crc32 = u32_at(data, at + 16)?;
        let compressed = u32_at(data, at + 20)?;
        let size = u32_at(data, at + 24)?;
        let local = u32_at(data, at + 42)?;
        if [compressed, size, local].contains(&u32::MAX) || u16_at(data, at + 34)? != 0 {
            return Err(unsupported("ZIP64 or multi-disk entry", at).member(&name));
        }
        // Unix symlinks are data, not archive files supported by this reader.
        if (u32_at(data, at + 38)? >> 16) & 0xf000 == 0xa000 {
            return Err(unsupported("symbolic-link ZIP entries are unsupported", at).member(&name));
        }
        let next = at + 46 + name_len + extra_len + comment_len;
        if next > end {
            return Err(bad(
                "zip.central_bounds",
                "central entry exceeds directory",
                at,
            ));
        }
        let local = local as usize;
        if bytes(data, local, 4)? != b"PK\x03\x04" {
            return Err(bad("zip.local_header", "invalid local header", local).member(&name));
        }
        bytes(data, local, 30)?;
        if u32_at(data, local + 18)? == u32::MAX || u32_at(data, local + 22)? == u32::MAX {
            return Err(
                unsupported("ZIP64 local headers are not yet supported", local).member(&name),
            );
        }
        let local_name_len = u16_at(data, local + 26)? as usize;
        let local_extra_len = u16_at(data, local + 28)? as usize;
        if u16_at(data, local + 6)? != flags
            || u16_at(data, local + 8)? != method
            || bytes(data, local + 30, local_name_len)? != raw_name
        {
            return Err(bad(
                "zip.header_mismatch",
                "local and central headers disagree",
                local,
            )
            .member(&name));
        }
        let data_start = local + 30 + local_name_len + local_extra_len;
        let data_end = data_start
            .checked_add(compressed as usize)
            .ok_or_else(|| bad("zip.bounds", "compressed range overflow", local))?;
        let mut range_end = data_end;
        if flags & 8 != 0 {
            let descriptor = if bytes(data, data_end, 4)? == b"PK\x07\x08" {
                data_end + 4
            } else {
                data_end
            };
            if u32_at(data, descriptor)? != crc32
                || u32_at(data, descriptor + 4)? != compressed
                || u32_at(data, descriptor + 8)? != size
            {
                return Err(bad(
                    "zip.descriptor_mismatch",
                    "data descriptor and central header disagree",
                    descriptor,
                )
                .member(&name));
            }
            range_end = descriptor + 12;
        } else if u32_at(data, local + 14)? != crc32
            || u32_at(data, local + 18)? != compressed
            || u32_at(data, local + 22)? != size
        {
            return Err(
                bad("zip.header_mismatch", "local sizes or CRC disagree", local).member(&name),
            );
        }
        if range_end > central as usize {
            return Err(bad(
                "zip.member_bounds",
                "member overlaps central directory",
                local,
            )
            .member(&name));
        }
        ranges.push((local, range_end));
        entries.push(Entry {
            name,
            method,
            flags,
            crc32,
            compressed_size: compressed.into(),
            size: size.into(),
            data_start,
            central_start: at,
        });
        at = next;
    }
    if at != end {
        return Err(bad(
            "zip.central_count",
            "central entry count or size is inconsistent",
            at,
        ));
    }
    ranges.sort_unstable();
    if ranges.first().is_some_and(|r| r.0 != 0) {
        return Err(unsupported("prefixed ZIP archives are unsupported", 0));
    }
    if ranges.windows(2).any(|r| r[0].1 > r[1].0) {
        return Err(bad(
            "zip.overlapping_members",
            "local member ranges overlap",
            0,
        ));
    }
    Ok(entries)
}

fn validate_name(name: &str) -> Result<(), Error> {
    let path = name.strip_suffix('/').unwrap_or(name);
    if path.is_empty()
        || path.contains(['\\', '\0', ':'])
        || path.starts_with('/')
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(Error::new(
            ErrorKind::Format,
            "zip.unsafe_path",
            "container",
            "unsafe or ambiguous ZIP member path",
        ));
    }
    Ok(())
}
