use std::io::Cursor;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use flate2::{Decompress, FlushDecompress, Status as InflateStatus};
use serde::Serialize;

use crate::package::PackageCache;
use crate::source::allocation_error;
use crate::zip_index::{self, Entry};
use crate::{Diagnostic, DocumentInfo, Error, ErrorKind, InputSnapshot, Limits, Status};

#[derive(Clone, Copy, Debug)]
pub struct ReadOptions {
    pub limits: Limits,
    pub strict: bool,
}

impl ReadOptions {
    pub fn strict() -> Self {
        Self {
            limits: Limits::default(),
            strict: true,
        }
    }
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self::strict()
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveKind {
    F3d,
    F3z,
}

impl ArchiveKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::F3d => "f3d",
            Self::F3z => "f3z",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MemberInfo {
    pub name: String,
    pub compression_method: u16,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub crc32: u32,
    pub is_directory: bool,
    pub encrypted: bool,
    pub data_offset: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ValidationReport {
    pub schema_version: u32,
    pub source_sha256: String,
    pub status: Status,
    pub scope: &'static str,
    pub members_total: usize,
    pub members_verified: usize,
    pub bytes_decompressed: u64,
    pub nested_archives: &'static str,
    pub issues: Vec<Diagnostic>,
}

impl ValidationReport {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("report contains only JSON-safe scalar values")
    }
}

/// Immutable ZIP container with lazy document and package metadata layers.
#[derive(Debug)]
pub struct Archive {
    snapshot: InputSnapshot,
    pub(crate) limits: Limits,
    kind: ArchiveKind,
    members: Vec<MemberInfo>,
    issues: Vec<Diagnostic>,
    pub(crate) document_cache: OnceLock<Result<Arc<DocumentInfo>, Error>>,
    pub(crate) package_cache: OnceLock<Result<PackageCache, Error>>,
}

impl Archive {
    pub fn open(path: impl AsRef<Path>, options: ReadOptions) -> Result<Self, Error> {
        Self::from_snapshot(InputSnapshot::open(path, options.limits)?, options)
    }

    pub fn from_bytes(bytes: &[u8], options: ReadOptions) -> Result<Self, Error> {
        Self::from_snapshot(InputSnapshot::from_bytes(bytes, options.limits)?, options)
    }

    pub fn from_snapshot(snapshot: InputSnapshot, options: ReadOptions) -> Result<Self, Error> {
        if snapshot.data.len() as u64 > options.limits.max_input_bytes() {
            return Err(Error::new(
                ErrorKind::ResourceLimit,
                "limit.input_bytes",
                "source",
                "snapshot exceeds max_input_bytes",
            )
            .source_name(&snapshot.name));
        }
        let result = (|| {
            let entries = zip_index::parse(&snapshot.data, options.limits)?;
            // Cross-check the bounded index against the established ZIP reader.
            let mut zip =
                zip::ZipArchive::new(Cursor::new(snapshot.data.as_ref())).map_err(zip_error)?;
            if zip.len() != entries.len() {
                return Err(Error::new(
                    ErrorKind::Format,
                    "zip.index_mismatch",
                    "container",
                    "ZIP reader disagrees with bounded entry count",
                ));
            }
            let mut members = Vec::new();
            let mut issues = Vec::new();
            for (index, entry) in entries.iter().enumerate() {
                let file = zip.by_index_raw(index).map_err(zip_error)?;
                if file.name_raw() != entry.name.as_bytes()
                    || file.size() != entry.size
                    || file.compressed_size() != entry.compressed_size
                    || file.crc32() != entry.crc32
                    || file.data_start() != Some(entry.data_start as u64)
                {
                    return Err(Error::new(
                        ErrorKind::Format,
                        "zip.index_mismatch",
                        "container",
                        "ZIP reader disagrees with bounded entry metadata",
                    )
                    .member(&entry.name)
                    .offset(entry.central_start));
                }
                if let Some(error) = unsupported_entry(entry) {
                    if options.strict {
                        return Err(error);
                    }
                    issues.push(*error.source_name(&snapshot.name).diagnostic);
                }
                members.push(MemberInfo {
                    name: entry.name.clone(),
                    compression_method: entry.method,
                    compressed_size: entry.compressed_size,
                    uncompressed_size: entry.size,
                    crc32: entry.crc32,
                    is_directory: entry.name.ends_with('/'),
                    encrypted: entry.flags & 1 != 0,
                    data_offset: entry.data_start as u64,
                });
            }
            let single = members.iter().any(|m| m.name == "Manifest.dat");
            let package = members.iter().any(|m| m.name == "Manifest.json");
            let kind = match (single, package) {
                (true, false) => ArchiveKind::F3d,
                (false, true) => ArchiveKind::F3z,
                _ => {
                    return Err(Error::new(
                        ErrorKind::Format,
                        "container.not_fusion",
                        "container",
                        "expected exactly one root Manifest.dat or Manifest.json marker",
                    ));
                }
            };
            Ok((kind, members, issues))
        })();
        let (kind, members, issues) =
            result.map_err(|error: Error| error.source_name(&snapshot.name))?;
        Ok(Self {
            snapshot,
            limits: options.limits,
            kind,
            members,
            issues,
            document_cache: OnceLock::new(),
            package_cache: OnceLock::new(),
        })
    }

    pub fn kind(&self) -> ArchiveKind {
        self.kind
    }
    pub fn source_sha256(&self) -> &str {
        &self.snapshot.sha256
    }
    pub fn source_size(&self) -> u64 {
        self.snapshot.data.len() as u64
    }
    pub fn source_name(&self) -> &str {
        &self.snapshot.name
    }
    pub fn members(&self) -> &[MemberInfo] {
        &self.members
    }
    pub fn issues(&self) -> &[Diagnostic] {
        &self.issues
    }
    pub fn status(&self) -> Status {
        if self.issues.is_empty() {
            Status::Complete
        } else {
            Status::Partial
        }
    }

    /// Read and verify one member with a fresh per-operation byte budget.
    pub fn read_member(&self, name: &str) -> Result<Vec<u8>, Error> {
        self.read_member_budgeted(name, &mut 0)
    }

    pub(crate) fn read_member_budgeted(
        &self,
        name: &str,
        total: &mut u64,
    ) -> Result<Vec<u8>, Error> {
        let member = self
            .members
            .iter()
            .find(|m| m.name == name)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::MemberNotFound,
                    "zip.member_not_found",
                    "container",
                    "member does not exist",
                )
                .member(name)
                .source_name(&self.snapshot.name)
            })?;
        self.decode(member, total, true)
    }

    /// Validate all ZIP members, including nested archives in F3Z.
    pub fn verify(&self, strict: bool) -> Result<ValidationReport, Error> {
        if matches!(self.kind, ArchiveKind::F3z) {
            return self.verify_recursive(strict);
        }
        self.verify_outer(strict)
    }

    pub fn verify_outer(&self, strict: bool) -> Result<ValidationReport, Error> {
        let mut report = ValidationReport {
            schema_version: 1,
            source_sha256: self.snapshot.sha256.clone(),
            status: Status::Complete,
            scope: "outer_archive_members",
            members_total: self.members.len(),
            members_verified: 0,
            bytes_decompressed: 0,
            nested_archives: "not_requested",
            issues: self.issues.clone(),
        };
        let mut first = None;
        for member in &self.members {
            match self.decode(member, &mut report.bytes_decompressed, false) {
                Ok(_) => report.members_verified += 1,
                Err(mut error) => {
                    if !report.issues.iter().any(|issue| {
                        issue.code == error.diagnostic.code
                            && issue.member == error.diagnostic.member
                    }) {
                        report.issues.push((*error.diagnostic).clone());
                    }
                    if error.kind == ErrorKind::ResourceLimit {
                        report.status = Status::Invalid;
                        error.report = Some(Box::new(report));
                        return Err(error);
                    }
                    let status = if error.kind == ErrorKind::Unsupported {
                        Status::Partial
                    } else {
                        Status::Invalid
                    };
                    if report.status != Status::Invalid {
                        report.status = status;
                    }
                    if first.is_none() || error.kind != ErrorKind::Unsupported {
                        first = Some(error);
                    }
                }
            }
        }
        if strict && let Some(mut error) = first {
            error.report = Some(Box::new(report));
            return Err(error);
        }
        Ok(report)
    }

    pub(crate) fn decode(
        &self,
        member: &MemberInfo,
        total: &mut u64,
        retain: bool,
    ) -> Result<Vec<u8>, Error> {
        let result = (|| {
            if member.encrypted {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    "zip.encrypted",
                    "decompression",
                    "encrypted members are unsupported",
                ));
            }
            if ![0, 8, 93].contains(&member.compression_method) {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    "zip.compression_unsupported",
                    "decompression",
                    format!(
                        "unsupported compression method {}",
                        member.compression_method
                    ),
                ));
            }
            if member.uncompressed_size > self.limits.max_member_bytes() {
                return Err(Error::new(
                    ErrorKind::ResourceLimit,
                    "limit.member_bytes",
                    "decompression",
                    "declared member size exceeds max_member_bytes",
                ));
            }
            if member.uncompressed_size
                > self
                    .limits
                    .max_total_uncompressed_bytes()
                    .saturating_sub(*total)
            {
                return Err(Error::new(
                    ErrorKind::ResourceLimit,
                    "limit.total_bytes",
                    "decompression",
                    "declared member size exceeds remaining cumulative budget",
                ));
            }
            let start = member.data_offset as usize;
            let end = start + member.compressed_size as usize;
            let raw = &self.snapshot.data[start..end]; // validated before snapshot publication
            let mut output = Output {
                data: Vec::new(),
                crc: crc32fast::Hasher::new(),
                count: 0,
                total,
                limits: self.limits,
                expected: member.uncompressed_size,
                retain,
            };
            match member.compression_method {
                0 => {
                    output.accept(raw)?;
                }
                8 => {
                    let mut decoder = Decompress::new(false);
                    let mut buffer = [0; 8192];
                    loop {
                        let before_in = decoder.total_in();
                        let before_out = decoder.total_out();
                        let request = output.request_size(buffer.len());
                        let decoded = decoder.decompress(
                            &raw[before_in as usize..],
                            &mut buffer[..request],
                            FlushDecompress::Finish,
                        );
                        let produced = (decoder.total_out() - before_out) as usize;
                        output.accept(&buffer[..produced])?;
                        let status = decoded.map_err(|error| {
                            Error::new(
                                ErrorKind::Format,
                                "zip.decompression_failed",
                                "decompression",
                                error.to_string(),
                            )
                        })?;
                        if status == InflateStatus::StreamEnd {
                            if decoder.total_in() != raw.len() as u64 {
                                return Err(Error::new(
                                    ErrorKind::Format,
                                    "zip.trailing_compressed_data",
                                    "decompression",
                                    "trailing bytes after Deflate stream",
                                ));
                            }
                            break;
                        }
                        if decoder.total_in() == before_in && produced == 0 {
                            return Err(Error::new(
                                ErrorKind::Format,
                                "zip.truncated_stream",
                                "decompression",
                                "Deflate stream did not reach its end marker",
                            ));
                        }
                    }
                }
                93 => {
                    use zstd::stream::raw::{DParameter, Decoder, InBuffer, Operation, OutBuffer};
                    let mut decoder = Decoder::new().map_err(decode_io)?;
                    // libzstd's minimum window is 1 KiB. This cap applies to all frames.
                    let window_log = self
                        .limits
                        .max_member_bytes()
                        .min(self.limits.max_total_uncompressed_bytes())
                        .ilog2()
                        .clamp(10, 31);
                    decoder
                        .set_parameter(DParameter::WindowLogMax(window_log))
                        .map_err(decode_io)?;
                    let mut buffer = [0; 8192];
                    let mut input = InBuffer::around(raw);
                    loop {
                        let before = input.pos();
                        let request = output.request_size(buffer.len());
                        let (decoded, n) = {
                            let mut target = OutBuffer::around(&mut buffer[..request]);
                            let decoded = decoder.run(&mut input, &mut target);
                            (decoded, target.pos())
                        };
                        // Charge emitted bytes even when a damaged frame returns an error.
                        output.accept(&buffer[..n])?;
                        let remaining = decoded.map_err(decode_io)?;
                        if remaining == 0 && input.pos() == raw.len() {
                            break;
                        }
                        if before == input.pos() && n == 0 {
                            return Err(Error::new(
                                ErrorKind::Format,
                                "zip.truncated_stream",
                                "decompression",
                                "Zstandard stream did not reach a frame end",
                            ));
                        }
                    }
                }
                _ => unreachable!("compression was checked above"),
            }
            if output.count != member.uncompressed_size {
                return Err(Error::new(
                    ErrorKind::Format,
                    "zip.size_mismatch",
                    "verification",
                    "expanded size does not match the central directory",
                ));
            }
            if output.crc.finalize() != member.crc32 {
                return Err(Error::new(
                    ErrorKind::Format,
                    "zip.crc_mismatch",
                    "verification",
                    "expanded member CRC32 does not match",
                ));
            }
            Ok(output.data)
        })();
        result.map_err(|error: Error| {
            error
                .member(&member.name)
                .offset(member.data_offset as usize)
                .source_name(&self.snapshot.name)
        })
    }

    pub fn to_json(&self) -> String {
        #[derive(Serialize)]
        struct Summary<'a> {
            schema_version: u32,
            kind: ArchiveKind,
            source: &'a str,
            source_sha256: &'a str,
            source_size: u64,
            status: Status,
            scope: &'static str,
            verification: &'static str,
            manifest: &'static str,
            relationships: &'static str,
            geometry: &'static str,
            members: &'a [MemberInfo],
            issues: &'a [Diagnostic],
        }
        serde_json::to_string(&Summary {
            schema_version: 1,
            kind: self.kind,
            source: &self.snapshot.name,
            source_sha256: &self.snapshot.sha256,
            source_size: self.source_size(),
            status: self.status(),
            scope: "outer_archive_index",
            verification: "not_run",
            manifest: "not_requested",
            relationships: "not_requested",
            geometry: "not_requested",
            members: &self.members,
            issues: &self.issues,
        })
        .expect("summary contains only JSON-safe scalar values")
    }
}

fn zip_error(error: zip::result::ZipError) -> Error {
    let kind = if matches!(error, zip::result::ZipError::UnsupportedArchive(_)) {
        ErrorKind::Unsupported
    } else {
        ErrorKind::Format
    };
    Error::new(kind, "zip.library_rejected", "container", error.to_string())
}

fn unsupported_entry(entry: &Entry) -> Option<Error> {
    let result = if entry.flags & 1 != 0 {
        Some(Error::new(
            ErrorKind::Unsupported,
            "zip.encrypted",
            "decompression",
            "encrypted members are unsupported",
        ))
    } else if ![0, 8, 93].contains(&entry.method) {
        Some(Error::new(
            ErrorKind::Unsupported,
            "zip.compression_unsupported",
            "decompression",
            format!("unsupported compression method {}", entry.method),
        ))
    } else {
        None
    };
    result.map(|e| e.member(&entry.name).offset(entry.data_start))
}

fn decode_io(error: std::io::Error) -> Error {
    let message = error.to_string();
    let window = message.contains("too much memory") || message.contains("Window size too large");
    Error::new(
        if window {
            ErrorKind::ResourceLimit
        } else {
            ErrorKind::Format
        },
        if window {
            "limit.zstd_window"
        } else {
            "zip.decompression_failed"
        },
        "decompression",
        message,
    )
}

struct Output<'a> {
    data: Vec<u8>,
    crc: crc32fast::Hasher,
    count: u64,
    total: &'a mut u64,
    limits: Limits,
    expected: u64,
    retain: bool,
}

impl Output<'_> {
    fn request_size(&self, capacity: usize) -> usize {
        self.expected
            .saturating_sub(self.count)
            .min(self.limits.max_member_bytes().saturating_sub(self.count))
            .min(
                self.limits
                    .max_total_uncompressed_bytes()
                    .saturating_sub(*self.total),
            )
            .saturating_add(1)
            .min(capacity as u64) as usize
    }

    fn accept(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let n = bytes.len() as u64;
        let next = self.count.checked_add(n).ok_or_else(allocation_error)?;
        let total = self.total.checked_add(n).ok_or_else(allocation_error)?;
        if next > self.limits.max_member_bytes() {
            return Err(Error::new(
                ErrorKind::ResourceLimit,
                "limit.member_bytes",
                "decompression",
                "actual member size exceeds max_member_bytes",
            ));
        }
        if total > self.limits.max_total_uncompressed_bytes() {
            return Err(Error::new(
                ErrorKind::ResourceLimit,
                "limit.total_bytes",
                "decompression",
                "actual cumulative expansion exceeds max_total_uncompressed_bytes",
            ));
        }
        self.count = next;
        *self.total = total;
        if next > self.expected {
            return Err(Error::new(
                ErrorKind::Format,
                "zip.size_mismatch",
                "verification",
                "member expands beyond declared size",
            ));
        }
        if self.retain {
            if self.data.capacity() < next as usize {
                let maximum = usize::try_from(self.expected).map_err(|_| allocation_error())?;
                let capacity = self
                    .data
                    .capacity()
                    .saturating_mul(2)
                    .max(8192)
                    .min(maximum)
                    .max(next as usize);
                self.data
                    .try_reserve_exact(capacity - self.data.len())
                    .map_err(|_| allocation_error())?;
            }
            self.data.extend_from_slice(bytes);
        }
        self.crc.update(bytes);
        Ok(())
    }
}
