//! Bounded F3D manifest framing. Grammar references: cadmpeg format notes §1.3
//! (CC BY 4.0, cadmpeg project) and ezf3d's MIT-licensed format notes.
//! Uninterpreted data is retained by a source-bound decompressed-member span.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{Diagnostic, Error, ErrorKind};

pub(crate) const MAX_MANIFEST: u64 = 1024 * 1024;
const MAX_ITEMS: u32 = 256;
const MAX_UNITS: u32 = 65536;
const MAX_CANDIDATES: usize = 256;
const SUFFIX: &[u8] = b"H\0A\0A\0H\0A\0";

#[derive(Clone, Debug, Serialize)]
pub struct RawSpan {
    pub source_sha256: String,
    pub member: String,
    pub offset: u64,
    pub length: u64,
    pub sha256: String,
    pub purpose: String,
}

pub(crate) fn span(
    data: &[u8],
    start: usize,
    end: usize,
    member: &str,
    source: &str,
    purpose: &str,
) -> RawSpan {
    RawSpan {
        source_sha256: source.into(),
        member: member.into(),
        offset: start as u64,
        length: (end - start) as u64,
        sha256: Sha256::digest(&data[start..end])
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        purpose: purpose.into(),
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct TerminalInfo {
    pub raw: RawSpan,
    pub form: String,
    pub flag: Option<u8>,
    pub marker: Option<String>,
    pub display_name: Option<String>,
    pub lineage_urn: Option<String>,
    pub suffix: Option<RawSpan>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ManifestTail {
    pub offset: u64,
    pub active_asset_guid: String,
    pub primary_asset_bases: Vec<String>,
    pub secondary_asset_bases: Vec<String>,
    pub terminal: TerminalInfo,
}

#[derive(Clone, Debug, Serialize)]
pub struct DocumentManifest {
    pub format_version: String,
    pub document_kind: String,
    pub extension: String,
    pub name: String,
    pub description: String,
    pub document_guid: String,
    pub document_asset_guid: String,
    pub generation_form: &'static str,
    pub generation_words: Vec<u32>,
    pub schema: BTreeMap<String, u32>,
    pub generation_extension: Option<RawSpan>,
    pub tail: Option<ManifestTail>,
    pub framing_complete: bool,
    pub unknown_regions: Vec<RawSpan>,
    pub issues: Vec<Diagnostic>,
}

pub(crate) struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    member: &'a str,
    work: &'a mut usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8], pos: usize, member: &'a str, work: &'a mut usize) -> Self {
        Self {
            data,
            pos,
            member,
            work,
        }
    }
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
    pub fn error(&self, kind: ErrorKind, code: &'static str, message: impl Into<String>) -> Error {
        Error::new(
            kind,
            code,
            if self.member == "Manifest.dat" {
                "manifest"
            } else {
                "asset_manifest"
            },
            message,
        )
        .member(self.member)
        .offset(self.pos)
    }
    pub fn take(&mut self, size: usize) -> Result<&'a [u8], Error> {
        *self.work = self.work.checked_add(size).ok_or_else(|| {
            self.error(
                ErrorKind::ResourceLimit,
                "limit.manifest_work",
                "manifest work overflow",
            )
        })?;
        if *self.work > MAX_MANIFEST as usize * 8 {
            return Err(self.error(
                ErrorKind::ResourceLimit,
                "limit.manifest_work",
                "manifest framing work limit exceeded",
            ));
        }
        let end = self.pos.checked_add(size).ok_or_else(|| {
            self.error(
                ErrorKind::Format,
                "manifest.bounds",
                "manifest offset overflow",
            )
        })?;
        let result = self.data.get(self.pos..end).ok_or_else(|| {
            self.error(
                ErrorKind::Format,
                "manifest.truncated",
                "truncated manifest field",
            )
        })?;
        self.pos = end;
        Ok(result)
    }
    pub fn u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    pub fn count(&mut self) -> Result<u32, Error> {
        let n = self.u32()?;
        if n > MAX_ITEMS {
            return Err(self.error(
                ErrorKind::ResourceLimit,
                "limit.manifest_items",
                "manifest count exceeds 256",
            ));
        }
        Ok(n)
    }
    pub fn string(&mut self, wide: bool) -> Result<String, Error> {
        let count = self.u32()?;
        if count > MAX_UNITS {
            return Err(self.error(
                ErrorKind::ResourceLimit,
                "limit.manifest_string",
                "manifest string exceeds 65536 code units",
            ));
        }
        let bytes = self.take(count as usize * if wide { 2 } else { 1 })?;
        if wide {
            let words: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect();
            String::from_utf16(&words).map_err(|_| {
                self.error(
                    ErrorKind::Format,
                    "manifest.invalid_utf16",
                    "invalid UTF-16LE string",
                )
            })
        } else {
            if !bytes.is_ascii() {
                return Err(self.error(
                    ErrorKind::Format,
                    "manifest.invalid_ascii",
                    "non-ASCII manifest string",
                ));
            }
            Ok(String::from_utf8(bytes.to_vec()).expect("ASCII is UTF-8"))
        }
    }
    pub fn guid(&mut self) -> Result<String, Error> {
        let value = self.string(true)?;
        if !valid_guid(&value) {
            return Err(self.error(
                ErrorKind::Format,
                "manifest.invalid_guid",
                "expected canonical hyphenated GUID",
            ));
        }
        Ok(value)
    }
    pub fn registry(&mut self) -> Result<BTreeMap<String, u32>, Error> {
        let mut result = BTreeMap::new();
        for _ in 0..self.count()? {
            let key = self.string(false)?;
            if key.is_empty() || result.contains_key(&key) {
                return Err(self.error(
                    ErrorKind::Format,
                    "manifest.duplicate_schema",
                    "empty or duplicate registry key",
                ));
            }
            let value = self.u32()?;
            result.insert(key, value);
        }
        Ok(result)
    }
}

pub(crate) fn valid_guid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

pub(crate) fn valid_base(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".." && !value.contains(['/', '\\', ':', '\0'])
}

fn lineage(value: &str) -> bool {
    value.len() > 4
        && value
            .get(..4)
            .is_some_and(|s| s.eq_ignore_ascii_case("urn:"))
        && value.bytes().all(|c| (33..=126).contains(&c))
}

fn parse_tail(
    data: &[u8],
    at: usize,
    member: &str,
    source: &str,
    work: &mut usize,
) -> Result<(ManifestTail, Vec<Diagnostic>), Error> {
    let mut r = Reader::new(data, at, member, work);
    let active_asset_guid = r.guid()?;
    let mut seen = HashSet::new();
    let mut groups = Vec::new();
    for primary in [true, false] {
        let count = r.count()?;
        if primary && count == 0 {
            return Err(r.error(
                ErrorKind::Format,
                "manifest.empty_assets",
                "primary asset list is empty",
            ));
        }
        let mut names = Vec::new();
        for _ in 0..count {
            let name = r.string(true)?;
            if !valid_base(&name) || !seen.insert(name.clone()) {
                return Err(r.error(
                    ErrorKind::Format,
                    "manifest.invalid_asset_base",
                    "unsafe or duplicate asset base",
                ));
            }
            names.push(name);
        }
        groups.push(names);
    }
    let mut terminal = TerminalInfo {
        raw: span(data, r.pos, data.len(), member, source, "terminal"),
        form: "no-terminal".into(),
        flag: None,
        marker: None,
        display_name: None,
        lineage_urn: None,
        suffix: None,
    };
    let mut issues = Vec::new();
    if r.remaining() > 0 {
        let flag = r.u8()?;
        terminal.flag = Some(flag);
        match flag {
            1 => {
                terminal.form = "flag-1-only".into();
                if r.remaining() > 0 {
                    let marker = r.string(true)?;
                    if marker == "NA_EXPORT" || marker == "NA_OFFLINESAVE" {
                        terminal.form = "flag-1-marker".into();
                        terminal.marker = Some(marker);
                    } else if lineage(&marker) {
                        terminal.form = "flag-1-lineage".into();
                        terminal.lineage_urn = Some(marker);
                    } else {
                        terminal.form = "flag-1-unknown-marker".into();
                        terminal.marker = Some(marker);
                        issues.push(
                            *r.error(
                                ErrorKind::Unsupported,
                                "manifest.unknown_marker",
                                "unknown flag-1 marker (encoded empty is distinct from omitted)",
                            )
                            .diagnostic,
                        );
                    }
                }
            }
            0 => {
                terminal.form = "flag-0-name".into();
                let name = r.string(true)?;
                if name.is_empty() {
                    return Err(r.error(
                        ErrorKind::Format,
                        "manifest.empty_display_name",
                        "flag-0 display name is empty",
                    ));
                }
                terminal.display_name = Some(name);
                if r.remaining() > 0 {
                    let urn = r.string(true)?;
                    if !lineage(&urn) {
                        return Err(r.error(
                            ErrorKind::Format,
                            "manifest.invalid_lineage",
                            "invalid lineage URN",
                        ));
                    }
                    terminal.lineage_urn = Some(urn);
                    terminal.form = "flag-0-name-lineage".into();
                    if r.remaining() > 0 {
                        let start = r.pos;
                        if r.take(r.remaining())? != SUFFIX {
                            return Err(r.error(
                                ErrorKind::Unsupported,
                                "manifest.unknown_tail",
                                "unknown lineage suffix",
                            ));
                        }
                        terminal.suffix =
                            Some(span(data, start, r.pos, member, source, "lineage_suffix"));
                        terminal.form.push_str("-suffix");
                    }
                }
            }
            _ => {
                return Err(r.error(
                    ErrorKind::Unsupported,
                    "manifest.unknown_tail",
                    "unknown terminal flag",
                ));
            }
        }
    }
    if r.remaining() != 0 {
        return Err(r.error(
            ErrorKind::Format,
            "manifest.trailing_bytes",
            "bytes after the terminal field",
        ));
    }
    Ok((
        ManifestTail {
            offset: at as u64,
            active_asset_guid,
            primary_asset_bases: groups.remove(0),
            secondary_asset_bases: groups.remove(0),
            terminal,
        },
        issues,
    ))
}

pub(crate) fn parse(data: &[u8], member: &str, source: &str) -> Result<DocumentManifest, Error> {
    let mut work = 0;
    let mut r = Reader::new(data, 0, member, &mut work);
    if data.len() as u64 > MAX_MANIFEST {
        return Err(r.error(
            ErrorKind::ResourceLimit,
            "limit.manifest_bytes",
            "manifest exceeds 1 MiB",
        ));
    }
    let format_version = r.string(false)?;
    if format_version != "3-2-0-0" {
        return Err(r.error(
            ErrorKind::Unsupported,
            "manifest.version",
            "unsupported document manifest version",
        ));
    }
    let document_kind = r.string(false)?;
    let extension = r.string(true)?;
    if document_kind != "FusionDocType" || extension != ".f3d" {
        return Err(r.error(
            ErrorKind::Unsupported,
            "manifest.document_kind",
            "unsupported document kind or extension",
        ));
    }
    let name = r.string(true)?;
    let description = r.string(true)?;
    let document_guid = r.guid()?;
    let document_asset_guid = r.guid()?;
    let first = r.u32()?;
    let mut generation_words = vec![first];
    for _ in 0..if first == 1234 { 3 } else { 1 } {
        generation_words.push(r.u32()?);
    }
    let schema = r.registry()?;
    let anchor = r.pos;
    let mut candidates = Vec::new();
    let mut errors = Vec::new();
    let mut attempts = 0;
    for at in anchor..data.len().saturating_sub(75) {
        if data.get(at..at + 4) != Some(&[36, 0, 0, 0]) {
            continue;
        }
        let raw = &data[at + 4..at + 76];
        if !raw.chunks_exact(2).all(|p| p[1] == 0) {
            continue;
        }
        let guid: String = raw.chunks_exact(2).map(|p| p[0] as char).collect();
        if !valid_guid(&guid) {
            continue;
        }
        attempts += 1;
        if attempts > MAX_CANDIDATES {
            return Err(Error::new(
                ErrorKind::ResourceLimit,
                "limit.manifest_candidates",
                "manifest",
                "too many tail candidates",
            )
            .member(member)
            .offset(at));
        }
        match parse_tail(data, at, member, source, &mut work) {
            Ok(value) => candidates.push(value),
            Err(error) => {
                if error.diagnostic.code == "limit.manifest_work" {
                    return Err(error);
                }
                errors.push(error);
            }
        }
    }
    if candidates.len() > 1 {
        return Err(Error::new(
            ErrorKind::Format,
            "manifest.ambiguous_tail",
            "manifest",
            "multiple complete asset-tail framings",
        )
        .member(member)
        .offset(anchor));
    }
    let (tail, generation_extension, unknown_regions, issues) =
        if let Some((tail, issues)) = candidates.pop() {
            let extension = span(
                data,
                anchor,
                tail.offset as usize,
                member,
                source,
                "generation_extension",
            );
            let mut regions = vec![extension.clone()];
            if !issues.is_empty() {
                regions.push(tail.terminal.raw.clone());
            }
            (Some(tail), Some(extension), regions, issues)
        } else {
            if let Some(error) = errors.iter().find(|e| e.kind == ErrorKind::ResourceLimit) {
                return Err(error.clone());
            }
            if !errors.is_empty() && errors.iter().all(|e| e.kind == ErrorKind::Format) {
                return Err(errors.remove(0));
            }
            let issue = Error::new(
                ErrorKind::Unsupported,
                "manifest.unknown_tail",
                "manifest",
                "no uniquely supported asset-tail framing",
            )
            .member(member)
            .offset(anchor);
            (
                None,
                None,
                vec![span(
                    data,
                    anchor,
                    data.len(),
                    member,
                    source,
                    "unresolved_manifest_tail",
                )],
                vec![*issue.diagnostic],
            )
        };
    let framing_complete = tail.is_some();
    Ok(DocumentManifest {
        format_version,
        document_kind,
        extension,
        name,
        description,
        document_guid,
        document_asset_guid,
        generation_form: if first == 1234 {
            "cookie-1234"
        } else {
            "legacy"
        },
        generation_words,
        schema,
        generation_extension,
        tail,
        framing_complete,
        unknown_regions,
        issues,
    })
}
