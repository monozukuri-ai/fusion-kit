use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::manifest::{self, MAX_MANIFEST, RawSpan, Reader};
use crate::{Archive, ArchiveKind, Diagnostic, DocumentManifest, Error, ErrorKind, Status};

#[derive(Clone, Debug, Serialize)]
pub struct SegmentInstance {
    pub folder: String,
    pub meta_member: Option<String>,
    pub bulk_member: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SegmentInfo {
    pub slot: u32,
    pub prefix: String,
    pub segment_type: String,
    pub role: &'static str,
    pub instances: Vec<SegmentInstance>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AssetInfo {
    pub base: String,
    pub folder: String,
    pub id: String,
    pub secondary_guid: String,
    pub asset_type: String,
    pub revision: Option<u32>,
    pub schema: Option<BTreeMap<String, u32>>,
    pub asset_kind: Option<String>,
    pub subtype: Option<String>,
    pub properties: Option<BTreeMap<String, String>>,
    pub change_counter: Option<u32>,
    pub role: &'static str,
    pub is_active: bool,
    pub segment_status: &'static str,
    pub segments: Option<Vec<SegmentInfo>>,
    pub unknown_regions: Vec<RawSpan>,
    pub issues: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DocumentInfo {
    pub schema_version: u32,
    pub scope: &'static str,
    pub source_sha256: String,
    pub id: Option<String>,
    pub format_version: Option<String>,
    pub status: Status,
    pub manifest_status: Status,
    pub manifest: Option<DocumentManifest>,
    pub assets: Option<Vec<AssetInfo>>,
    pub active_asset_id: Option<String>,
    pub design_asset_id: Option<String>,
    pub design_asset_folder: Option<String>,
    pub preview_members: Option<Vec<String>>,
    pub metadata_bytes: u64,
    pub geometry: &'static str,
    pub unknown_regions: Vec<RawSpan>,
    pub issues: Vec<Diagnostic>,
}

impl DocumentInfo {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("document metadata contains only JSON-safe values")
    }
}

fn issue(code: &'static str, message: impl Into<String>, member: &str) -> Error {
    Error::new(ErrorKind::Format, code, "document", message).member(member)
}

pub(crate) fn inspect(archive: &Archive) -> Result<DocumentInfo, Error> {
    inspect_budgeted(archive, &mut 0)
}

pub(crate) fn inspect_budgeted(archive: &Archive, total: &mut u64) -> Result<DocumentInfo, Error> {
    if !matches!(archive.kind(), ArchiveKind::F3d) {
        return Err(Error::new(
            ErrorKind::Unsupported,
            "document.package_not_decoded",
            "document",
            "F3Z root and document resolution is a later layer; explicitly open a child F3D",
        ));
    }
    let before = *total;
    let raw = archive.read_metadata_member("Manifest.dat", total)?;
    let mut first_work = 0;
    let format_version = Some(Reader::new(&raw, 0, "Manifest.dat", &mut first_work).string(false)?);
    let mut result = DocumentInfo {
        schema_version: 1,
        scope: "f3d_document_and_design_registry",
        source_sha256: archive.source_sha256().into(),
        id: None,
        format_version,
        status: Status::Complete,
        manifest_status: Status::Complete,
        manifest: None,
        assets: None,
        active_asset_id: None,
        design_asset_id: None,
        design_asset_folder: None,
        preview_members: None,
        metadata_bytes: 0,
        geometry: "not_requested",
        unknown_regions: Vec::new(),
        issues: Vec::new(),
    };
    let mut parsed = match manifest::parse(&raw, "Manifest.dat", archive.source_sha256()) {
        Ok(manifest) => manifest,
        Err(error) if error.kind == ErrorKind::Unsupported => {
            result.status = Status::Unsupported;
            result.manifest_status = Status::Unsupported;
            result.unknown_regions.push(manifest::span(
                &raw,
                0,
                raw.len(),
                "Manifest.dat",
                archive.source_sha256(),
                "unsupported_document_manifest",
            ));
            result
                .issues
                .push(*error.source_name(archive.source_name()).diagnostic);
            result.metadata_bytes = *total - before;
            return Ok(result);
        }
        Err(error) => return Err(error),
    };
    for d in &mut parsed.issues {
        d.source = Some(archive.source_name().into());
    }
    result.id = Some(parsed.document_guid.clone());
    result.issues.extend(parsed.issues.iter().cloned());
    result.unknown_regions = parsed.unknown_regions.clone();
    let Some(tail) = &parsed.tail else {
        result.status = Status::Unsupported;
        result.manifest_status = Status::Unsupported;
        result.manifest = Some(parsed);
        result.metadata_bytes = *total - before;
        for d in &mut result.issues {
            d.source = Some(archive.source_name().into());
        }
        return Ok(result);
    };
    if !parsed.issues.is_empty() {
        result.manifest_status = Status::Unsupported;
        result.status = Status::Partial;
    }
    let mut assets = Vec::new();
    for base in tail
        .primary_asset_bases
        .iter()
        .chain(&tail.secondary_asset_bases)
    {
        let folders: Vec<String> = [base.clone(), format!("{base}[Active]")]
            .into_iter()
            .filter(|folder| {
                archive.members().iter().any(|m| {
                    m.name
                        .strip_prefix(folder)
                        .is_some_and(|s| s.starts_with('/'))
                })
            })
            .collect();
        if folders.len() != 1 {
            return Err(issue(
                "document.asset_folder_ambiguous",
                format!("asset base {base:?} has {} matching folders", folders.len()),
                "Manifest.dat",
            ));
        }
        let folder = &folders[0];
        let member = format!("{folder}/Manifest.dat");
        if !archive
            .members()
            .iter()
            .any(|m| m.name == member && !m.is_directory)
        {
            return Err(issue(
                "document.asset_manifest_missing",
                "listed asset has no Manifest.dat",
                &member,
            ));
        }
        let bytes = archive.read_metadata_member(&member, total)?;
        let mut asset = parse_asset(&bytes, base, folder, archive)?;
        for d in &mut asset.issues {
            d.source = Some(archive.source_name().into());
        }
        if assets
            .iter()
            .any(|a: &AssetInfo| a.id.eq_ignore_ascii_case(&asset.id))
        {
            return Err(issue(
                "document.duplicate_asset_guid",
                "multiple assets declare the same primary GUID",
                &member,
            ));
        }
        result.issues.extend(asset.issues.iter().cloned());
        assets.push(asset);
    }
    let active: Vec<usize> = assets
        .iter()
        .enumerate()
        .filter(|(_, a)| a.id.eq_ignore_ascii_case(&tail.active_asset_guid))
        .map(|(i, _)| i)
        .collect();
    if active.len() != 1 {
        return Err(issue(
            "document.active_asset_unresolved",
            "active GUID does not identify one listed asset",
            "Manifest.dat",
        ));
    }
    assets[active[0]].is_active = true;
    result.active_asset_id = Some(assets[active[0]].id.clone());
    let designs: Vec<&AssetInfo> = assets.iter().filter(|a| a.role == "design").collect();
    if designs.len() > 1 {
        return Err(issue(
            "document.multiple_design_assets",
            "more than one listed asset declares root Design",
            "Manifest.dat",
        ));
    }
    if designs.len() == 1 && !assets.iter().any(|a| a.role == "unresolved") {
        let design = designs[0];
        result.design_asset_id = Some(design.id.clone());
        result.design_asset_folder = Some(design.folder.clone());
        let prefix = format!("{}/Previews/", design.folder);
        result.preview_members = Some(
            archive
                .members()
                .iter()
                .filter(|m| {
                    !m.is_directory
                        && m.name.starts_with(&prefix)
                        && m.name.rsplit('.').next().is_some_and(|ext| {
                            ["png", "jpg", "jpeg"]
                                .iter()
                                .any(|known| ext.eq_ignore_ascii_case(known))
                        })
                })
                .map(|m| m.name.clone())
                .collect(),
        );
    } else {
        result.status = Status::Partial;
        result.issues.push(
            *Error::new(
                ErrorKind::Unsupported,
                "document.design_unresolved",
                "document",
                "root Design cannot be uniquely established from typed asset headers",
            )
            .member("Manifest.dat")
            .diagnostic,
        );
    }
    if result.issues.iter().any(|d| d.severity == "error") {
        result.status = Status::Partial;
    }
    result.assets = Some(assets);
    result.manifest = Some(parsed);
    result.metadata_bytes = *total - before;
    for d in &mut result.issues {
        d.source = Some(archive.source_name().into());
    }
    Ok(result)
}

fn parse_asset(
    data: &[u8],
    base: &str,
    folder: &str,
    archive: &Archive,
) -> Result<AssetInfo, Error> {
    let member = format!("{folder}/Manifest.dat");
    let mut work = 0;
    let mut r = Reader::new(data, 0, &member, &mut work);
    let declared = r.string(true)?;
    if declared != base {
        return Err(r.error(
            ErrorKind::Format,
            "asset.base_mismatch",
            "asset header base differs from document registry",
        ));
    }
    let id = r.guid()?;
    let secondary_guid = r.guid()?;
    let asset_type = r.string(false)?;
    if !asset_type.ends_with("AssetType") {
        return Err(r.error(
            ErrorKind::Format,
            "asset.invalid_type",
            "invalid asset type token",
        ));
    }
    let mut asset = AssetInfo {
        base: base.into(),
        folder: folder.into(),
        id,
        secondary_guid,
        asset_type,
        revision: None,
        schema: None,
        asset_kind: None,
        subtype: None,
        properties: None,
        change_counter: None,
        role: "other",
        is_active: false,
        segment_status: "not_requested",
        segments: None,
        unknown_regions: Vec::new(),
        issues: Vec::new(),
    };
    if asset.asset_type != "FusionAssetType" {
        asset.unknown_regions.push(manifest::span(
            data,
            r.pos,
            data.len(),
            &member,
            archive.source_sha256(),
            "non_design_asset_payload",
        ));
        return Ok(asset);
    }
    let revision = r.u32()?;
    asset.revision = Some(revision);
    if ![14, 19, 20].contains(&revision) {
        asset.role = "unresolved";
        asset.segment_status = "unsupported";
        asset.issues.push(
            *r.error(
                ErrorKind::Unsupported,
                "asset.revision",
                "unsupported Fusion asset manifest revision",
            )
            .diagnostic,
        );
        asset.unknown_regions.push(manifest::span(
            data,
            r.pos,
            data.len(),
            &member,
            archive.source_sha256(),
            "unsupported_asset_revision",
        ));
        return Ok(asset);
    }
    asset.schema = Some(r.registry()?);
    let kind = r.string(false)?;
    asset.asset_kind = Some(kind.clone());
    if kind != "Neutron3DAssetType" {
        asset.role = "unresolved";
        asset.segment_status = "unsupported";
        asset.issues.push(
            *r.error(
                ErrorKind::Unsupported,
                "asset.kind",
                "unsupported Fusion asset kind",
            )
            .diagnostic,
        );
        asset.unknown_regions.push(manifest::span(
            data,
            r.pos,
            data.len(),
            &member,
            archive.source_sha256(),
            "unsupported_asset_kind",
        ));
        return Ok(asset);
    }
    let mode_at = r.pos;
    if r.u8()? != 0 {
        asset.role = "unresolved";
        asset.segment_status = "unsupported";
        asset.issues.push(
            *r.error(
                ErrorKind::Unsupported,
                "asset.subtype_mode",
                "unknown asset subtype mode",
            )
            .diagnostic,
        );
        asset.unknown_regions.push(manifest::span(
            data,
            mode_at,
            data.len(),
            &member,
            archive.source_sha256(),
            "unsupported_asset_subtype_mode",
        ));
        return Ok(asset);
    }
    let subtype = r.string(false)?;
    asset.subtype = Some(subtype.clone());
    if !subtype.is_empty() {
        asset.role = "derived_fusion";
        asset.unknown_regions.push(manifest::span(
            data,
            r.pos,
            data.len(),
            &member,
            archive.source_sha256(),
            "derived_asset_payload",
        ));
        return Ok(asset);
    }
    asset.role = "design";
    let count = r.count()?;
    let mut properties = BTreeMap::new();
    for _ in 0..count {
        let key = r.string(false)?;
        if key.is_empty() || properties.insert(key, r.string(true)?).is_some() {
            return Err(r.error(
                ErrorKind::Format,
                "asset.duplicate_property",
                "empty or duplicate asset property",
            ));
        }
    }
    let refs_at = r.pos;
    asset.properties = Some(properties);
    let first = r.u32()?;
    let compact = first == 0;
    let (mut segments, table_at, table_end) = if compact {
        asset.change_counter = Some(r.u32()?);
        let table_at = r.pos;
        let (rows, end) = segment_table(data, table_at, &member, &mut work)?;
        (rows, table_at, end)
    } else {
        // The observed legacy/reference preambles differ. Admit only one
        // counted, typed segment registry ending exactly at EOF; retain all
        // preceding bytes without interpreting them as document references.
        let mut candidates = Vec::new();
        let mut attempts = 0;
        for at in refs_at..data.len().saturating_sub(3) {
            let count = u32::from_le_bytes(data[at..at + 4].try_into().expect("four-byte slice"));
            if !(1..=256).contains(&count) {
                continue;
            }
            attempts += 1;
            if attempts > 4096 {
                return Err(Error::new(
                    ErrorKind::ResourceLimit,
                    "limit.asset_candidates",
                    "asset_manifest",
                    "too many segment-registry candidates",
                )
                .member(&member)
                .offset(at));
            }
            match segment_table(data, at, &member, &mut work) {
                Ok((rows, end)) if end == data.len() => candidates.push((rows, at, end)),
                Err(error) if error.diagnostic.code == "limit.manifest_work" => return Err(error),
                _ => {}
            }
        }
        if candidates.len() > 1 {
            return Err(issue(
                "asset.ambiguous_segment_table",
                "multiple complete segment-registry suffixes",
                &member,
            ));
        }
        if let Some(candidate) = candidates.pop() {
            candidate
        } else {
            asset.segment_status = "unsupported";
            asset.issues.push(
                *Error::new(
                    ErrorKind::Unsupported,
                    "asset.segment_table",
                    "asset_manifest",
                    "no unique typed segment-registry suffix",
                )
                .member(&member)
                .offset(refs_at)
                .diagnostic,
            );
            asset.unknown_regions.push(manifest::span(
                data,
                refs_at,
                data.len(),
                &member,
                archive.source_sha256(),
                "asset_unresolved_registry_tail",
            ));
            return Ok(asset);
        }
    };
    asset.unknown_regions.push(manifest::span(
        data,
        refs_at,
        table_at,
        &member,
        archive.source_sha256(),
        "asset_reference_preamble",
    ));
    let mut seen = BTreeSet::new();
    for segment in &mut segments {
        let asset_prefix = format!("{folder}/");
        let folders: BTreeSet<String> = archive
            .members()
            .iter()
            .filter_map(|m| m.name.strip_prefix(&asset_prefix))
            .filter_map(|s| s.split_once('/').map(|(f, _)| f.to_owned()))
            .filter(|f| {
                f == &segment.prefix
                    || f.strip_prefix(&segment.prefix)
                        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            })
            .collect();
        for f in folders {
            let directory = format!("{folder}/{f}");
            if !seen.insert(directory.clone()) {
                return Err(issue(
                    "asset.ambiguous_segment",
                    "multiple declarations resolve the same segment folder",
                    &member,
                ));
            }
            let meta = format!("{directory}/MetaStream.dat");
            let bulk = format!("{directory}/BulkStream.dat");
            segment.instances.push(SegmentInstance {
                folder: directory,
                meta_member: archive
                    .members()
                    .iter()
                    .any(|m| m.name == meta && !m.is_directory)
                    .then_some(meta),
                bulk_member: archive
                    .members()
                    .iter()
                    .any(|m| m.name == bulk && !m.is_directory)
                    .then_some(bulk),
            });
        }
    }
    if table_end != data.len() {
        asset.segment_status = "unsupported";
        asset.issues.push(
            *Error::new(
                ErrorKind::Unsupported,
                "asset.trailing_bytes",
                "asset_manifest",
                "unknown bytes after segment registry",
            )
            .member(&member)
            .offset(table_end)
            .diagnostic,
        );
        asset.unknown_regions.push(manifest::span(
            data,
            table_end,
            data.len(),
            &member,
            archive.source_sha256(),
            "asset_unknown_tail",
        ));
    } else {
        asset.segment_status = "complete";
    }
    asset.segments = Some(segments);
    Ok(asset)
}

fn segment_table(
    data: &[u8],
    at: usize,
    member: &str,
    work: &mut usize,
) -> Result<(Vec<SegmentInfo>, usize), Error> {
    let mut r = Reader::new(data, at, member, work);
    let count = r.count()?;
    let mut rows = Vec::new();
    let mut slots = BTreeSet::new();
    let mut prefixes = BTreeSet::new();
    for _ in 0..count {
        let slot = r.u32()?;
        let prefix = r.string(false)?;
        let segment_type = r.string(false)?;
        if !manifest::valid_base(&prefix)
            || !manifest::valid_base(&segment_type)
            || !slots.insert(slot)
            || !prefixes.insert(prefix.clone())
        {
            return Err(r.error(
                ErrorKind::Format,
                "asset.invalid_segment",
                "unsafe or duplicate segment declaration",
            ));
        }
        let role = match segment_type.as_str() {
            "Design" | "FusionDesignSegmentType" => "design",
            "FusionACTSegmentType" => "act",
            "FusionBrowserSegmentType" => "browser",
            _ => "other",
        };
        rows.push(SegmentInfo {
            slot,
            prefix,
            segment_type,
            role,
            instances: Vec::new(),
        });
    }
    Ok((rows, r.pos))
}

impl Archive {
    /// Lazily parse the standalone F3D document. Strictness is a view policy;
    /// unknown data remains in the cached partial result for later inspection.
    pub fn read_document(&self, strict: bool) -> Result<Arc<DocumentInfo>, Error> {
        let info = self
            .document_cache
            .get_or_init(|| {
                inspect(self)
                    .map(Arc::new)
                    .map_err(|e| e.source_name(self.source_name()))
            })
            .clone()?;
        if strict && info.status != Status::Complete {
            let mut error = Error::new(
                ErrorKind::Unsupported,
                "document.partial",
                "document",
                "document metadata is only partially supported",
            );
            if let Some(d) = info.issues.first() {
                error.diagnostic = Box::new(d.clone());
            }
            error.diagnostics = info.issues.clone();
            error.result_status = Some(info.status);
            return Err(error);
        }
        Ok(info)
    }

    pub fn read_span(&self, region: &RawSpan) -> Result<Vec<u8>, Error> {
        self.read_span_inner(region)
            .map_err(|e| e.source_if_missing(self.source_name()))
    }

    fn read_span_inner(&self, region: &RawSpan) -> Result<Vec<u8>, Error> {
        if region.source_sha256 != self.source_sha256() {
            return Err(issue(
                "span.source_mismatch",
                "raw span belongs to a different input snapshot",
                &region.member,
            ));
        }
        let bytes = self.read_member(&region.member)?;
        let start = usize::try_from(region.offset)
            .map_err(|_| issue("span.bounds", "span offset overflow", &region.member))?;
        let length = usize::try_from(region.length)
            .map_err(|_| issue("span.bounds", "span length overflow", &region.member))?;
        let end = start
            .checked_add(length)
            .ok_or_else(|| issue("span.bounds", "span range overflow", &region.member))?;
        let selected = bytes
            .get(start..end)
            .ok_or_else(|| issue("span.bounds", "span is outside the member", &region.member))?
            .to_vec();
        let actual: String = Sha256::digest(&selected)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if actual != region.sha256 {
            return Err(issue(
                "span.hash_mismatch",
                "raw span hash does not describe its source bytes",
                &region.member,
            ));
        }
        Ok(selected)
    }

    pub fn get_preview(
        &self,
        document_id: Option<&str>,
        strict: bool,
    ) -> Result<Option<Vec<u8>>, Error> {
        self.preview_inner(document_id, strict)
            .map_err(|e| e.source_if_missing(self.source_name()))
    }

    fn preview_inner(
        &self,
        document_id: Option<&str>,
        strict: bool,
    ) -> Result<Option<Vec<u8>>, Error> {
        if matches!(self.kind(), ArchiveKind::F3z) {
            return self.package_preview(document_id, strict);
        }
        let info = self.read_document(strict)?;
        if let Some(id) = document_id
            && !info
                .id
                .as_deref()
                .is_some_and(|actual| actual.eq_ignore_ascii_case(id))
        {
            return Err(Error::new(
                ErrorKind::MemberNotFound,
                "document.not_found",
                "preview",
                "unknown document id",
            ));
        }
        let previews = info.preview_members.as_ref().ok_or_else(|| {
            Error::new(
                ErrorKind::Unsupported,
                "preview.design_unresolved",
                "preview",
                "preview owner is unresolved",
            )
        })?;
        let default = format!(
            "{}/Previews/small.png",
            info.design_asset_folder.as_deref().unwrap_or("")
        );
        let selected = previews.iter().find(|p| *p == &default).or_else(|| {
            if previews.len() == 1 {
                previews.first()
            } else {
                None
            }
        });
        if previews.is_empty() {
            return Ok(None);
        }
        let selected = selected.ok_or_else(|| {
            Error::new(
                ErrorKind::Format,
                "preview.ambiguous",
                "preview",
                "multiple previews without one known default",
            )
        })?;
        let mut total = info.metadata_bytes;
        Ok(Some(self.read_member_budgeted(selected, &mut total)?))
    }

    pub(crate) fn read_metadata_member(
        &self,
        name: &str,
        total: &mut u64,
    ) -> Result<Vec<u8>, Error> {
        if let Some(member) = self.members().iter().find(|m| m.name == name)
            && member.uncompressed_size > MAX_MANIFEST
        {
            return Err(Error::new(
                ErrorKind::ResourceLimit,
                "limit.manifest_bytes",
                "manifest",
                "manifest exceeds 1 MiB",
            )
            .member(name));
        }
        self.read_member_budgeted(name, total)
    }
}
