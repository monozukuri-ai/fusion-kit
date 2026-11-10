//! F3Z document dependencies. No component placements or geometry are inferred.
mod graph;

use crate::{
    Archive, ArchiveKind, Diagnostic, DocumentInfo, Error, ErrorKind, InputSnapshot, RawSpan,
    ReadOptions, Status,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize)]
pub struct ReferenceInfo {
    pub layer: &'static str,
    pub graph_index: Option<usize>,
    pub source_id: Option<i64>,
    pub target_id: Option<i64>,
    pub source: Option<String>,
    pub target: Option<String>,
    pub kind: Option<String>,
    pub neutron_role: Option<String>,
    pub neutron_data: Option<String>,
    pub group_index: usize,
    pub relationship_index: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphNode {
    pub object_id: i64,
    pub path: Option<String>,
    pub content_type: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphInfo {
    pub graph_index: usize,
    pub root_ids: Vec<i64>,
    pub nodes: Vec<GraphNode>,
    pub references: Vec<ReferenceInfo>,
    pub reachable_ids: Vec<i64>,
    pub unreachable_ids: Vec<i64>,
    pub cycles: Vec<Vec<i64>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RedirectionInfo {
    pub document_id: String,
    pub designs: Vec<Value>,
    pub references: Vec<ReferenceInfo>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LayerComparison {
    pub graph_index: usize,
    pub source: String,
    pub matches: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct JsonSidecar {
    pub document_id: Option<String>,
    pub raw: RawSpan,
    pub value: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct PackageDocument {
    /// Member path, scoped by package_sha256; not a graph object ID or Fusion GUID.
    pub id: String,
    pub package_sha256: String,
    pub source_sha256: Option<String>,
    pub document_guid: Option<String>,
    pub status: Status,
    pub metadata: Option<Arc<DocumentInfo>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PackageInfo {
    pub schema_version: u32,
    pub scope: &'static str,
    pub source_sha256: String,
    pub status: Status,
    pub declared_root: Option<String>,
    pub root_document_id: Option<String>,
    pub documents: Vec<PackageDocument>,
    pub graphs: Vec<GraphInfo>,
    pub references: Vec<ReferenceInfo>,
    pub redirections: Vec<RedirectionInfo>,
    pub comparisons: Vec<LayerComparison>,
    pub undocumented_members: Vec<String>,
    pub raw_sidecars: Vec<JsonSidecar>,
    pub bytes_decompressed: u64,
    pub members_indexed: u64,
    pub verification: &'static str,
    pub occurrences: &'static str,
    pub geometry: &'static str,
    pub issues: Vec<Diagnostic>,
}

impl PackageInfo {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("package metadata contains JSON-safe values")
    }
}

#[derive(Debug)]
pub(crate) struct PackageCache {
    info: Arc<PackageInfo>,
    children: BTreeMap<String, Arc<Archive>>,
}

pub(crate) fn path_valid(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 65535
        && !path.ends_with('/')
        && !path.contains(['\\', ':', '\0'])
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

fn error(kind: ErrorKind, code: &'static str, message: impl Into<String>, member: &str) -> Error {
    Error::new(kind, code, "package", message).member(member)
}

// Keep a real member locator separate from a graph/JSON context. Inner sources
// are temporarily member IDs here; inspect() binds them to the parent source.
fn locate(d: &mut Diagnostic) {
    if d.stage != "relationships" || d.context.is_some() {
        return;
    }
    let Some(context) = d.member.clone() else {
        return;
    };
    d.context = Some(context.clone());
    if context.starts_with("DesignDescription.json") {
        d.member = Some("DesignDescription.json".into());
    } else if let Some(path) = context.strip_suffix("!RedirectionsStream.dat") {
        d.source = Some(path.into());
        d.member = Some("RedirectionsStream.dat".into());
    } else if context == "archive" {
        d.member = None;
    }
}

fn read_sidecar(
    archive: &Archive,
    member: &str,
    total: &mut u64,
    document_id: Option<&str>,
) -> Result<JsonSidecar, Error> {
    let entry = archive
        .members()
        .iter()
        .find(|m| m.name == member && !m.is_directory)
        .ok_or_else(|| {
            error(
                ErrorKind::Format,
                "package.sidecar_missing",
                "required JSON sidecar is absent",
                member,
            )
        })?;
    if entry.uncompressed_size > 16 * 1024 * 1024 {
        return Err(error(
            ErrorKind::ResourceLimit,
            "limit.json_bytes",
            "JSON sidecar exceeds 16 MiB",
            member,
        ));
    }
    let raw = archive.read_member_budgeted(member, total)?;
    let value = crate::json::parse(&raw, member)?;
    Ok(JsonSidecar {
        document_id: document_id.map(str::to_owned),
        raw: crate::manifest::span(
            &raw,
            0,
            raw.len(),
            member,
            archive.source_sha256(),
            "json_sidecar",
        ),
        value,
    })
}

fn inspect(archive: &Archive) -> Result<PackageCache, Error> {
    let mut total = 0;
    let manifest = read_sidecar(archive, "Manifest.json", &mut total, None)?;
    let description = read_sidecar(archive, "DesignDescription.json", &mut total, None)?;
    let mut sidecars = vec![manifest, description];
    let mut children = BTreeMap::new();
    let mut documents = Vec::new();
    let mut entries = archive.members().len() as u64;
    let mut tables = BTreeMap::new();
    // Every F3D member is inspected, including members absent from the graph.
    for member in archive
        .members()
        .iter()
        .filter(|m| !m.is_directory && m.name.to_ascii_lowercase().ends_with(".f3d"))
    {
        if archive.limits.max_nesting_depth() < 2 {
            return Err(error(
                ErrorKind::ResourceLimit,
                "limit.nesting_depth",
                "F3D child requires archive depth 2",
                &member.name,
            ));
        }
        let bytes = archive.read_member_budgeted(&member.name, &mut total)?;
        let mut snapshot = InputSnapshot::from_vec(bytes, archive.limits)?;
        snapshot.name = format!("{}!{}", archive.source_name(), member.name);
        let child = Arc::new(Archive::from_snapshot(
            snapshot,
            ReadOptions {
                limits: archive.limits.remaining_entries(entries)?,
                strict: false,
            },
        )?);
        if !matches!(child.kind(), ArchiveKind::F3d) {
            return Err(error(
                ErrorKind::Format,
                "package.child_kind",
                ".f3d member is not an F3D container",
                &member.name,
            ));
        }
        entries = entries
            .checked_add(child.members().len() as u64)
            .ok_or_else(|| {
                error(
                    ErrorKind::ResourceLimit,
                    "limit.entries",
                    "cumulative entry count overflow",
                    &member.name,
                )
            })?;
        if entries > archive.limits.max_entries() {
            return Err(error(
                ErrorKind::ResourceLimit,
                "limit.entries",
                "cumulative member count exceeds max_entries",
                &member.name,
            ));
        }
        let metadata = Arc::new(
            crate::document::inspect_budgeted(&child, &mut total)
                .map_err(|e| e.source_name(child.source_name()))?,
        );
        let _ = child.document_cache.set(Ok(metadata.clone()));
        if child
            .members()
            .iter()
            .any(|m| m.name == "RedirectionsStream.dat" && !m.is_directory)
        {
            let table = read_sidecar(
                &child,
                "RedirectionsStream.dat",
                &mut total,
                Some(&member.name),
            )
            .map_err(|e| e.source_name(child.source_name()))?;
            tables.insert(member.name.clone(), table.value.clone());
            sidecars.push(table);
        }
        documents.push(PackageDocument {
            id: member.name.clone(),
            package_sha256: archive.source_sha256().into(),
            source_sha256: Some(child.source_sha256().into()),
            document_guid: metadata.id.clone(),
            status: metadata.status,
            metadata: Some(metadata),
        });
        children.insert(member.name.clone(), child);
    }
    let members: BTreeSet<String> = archive
        .members()
        .iter()
        .filter(|m| !m.is_directory)
        .map(|m| m.name.clone())
        .collect();
    let analysis = graph::analyze(&sidecars[0].value, &sidecars[1].value, &members, &tables)?;
    let non_f3d: BTreeSet<String> = analysis
        .graphs
        .iter()
        .flat_map(|g| &g.nodes)
        .filter_map(|n| n.path.as_ref())
        .filter(|p| members.contains(*p) && !p.to_ascii_lowercase().ends_with(".f3d"))
        .cloned()
        .collect();
    for path in non_f3d {
        documents.push(PackageDocument {
            id: path,
            package_sha256: archive.source_sha256().into(),
            source_sha256: None,
            document_guid: None,
            status: Status::Unsupported,
            metadata: None,
        });
    }
    let mut issues = analysis.issues;
    for doc in &documents {
        if doc.metadata.is_none() {
            let mut d = *error(
                ErrorKind::Unsupported,
                "unsupported_document",
                "document needs another decoder; member retained",
                &doc.id,
            )
            .diagnostic;
            d.severity = "unsupported";
            issues.push(d);
        }
    }
    for doc in &documents {
        if let Some(metadata) = &doc.metadata {
            issues.extend(metadata.issues.iter().map(|d| {
                let mut d = d.clone();
                d.severity = if metadata.status == Status::Unsupported {
                    "unsupported"
                } else {
                    "warning"
                };
                d.source = Some(format!("{}!{}", archive.source_name(), doc.id));
                d
            }));
        }
    }
    for d in &mut issues {
        locate(d);
        if let Some(child) = d.source.as_ref().and_then(|s| children.get(s)) {
            d.source = Some(child.source_name().into());
        }
        if d.source.is_none() {
            d.source = Some(archive.source_name().into());
        }
    }
    let status = graph::status(&issues);
    let info = PackageInfo {
        schema_version: 1,
        scope: "f3z_document_dependencies",
        source_sha256: archive.source_sha256().into(),
        status,
        declared_root: analysis.declared_root,
        root_document_id: analysis.root_document_id,
        documents,
        graphs: analysis.graphs,
        references: analysis.references,
        redirections: analysis.redirections,
        comparisons: analysis.comparisons,
        undocumented_members: analysis.undocumented_members,
        raw_sidecars: sidecars,
        bytes_decompressed: total,
        members_indexed: entries,
        verification: "metadata_members_only",
        occurrences: "not_requested",
        geometry: "not_requested",
        issues,
    };
    Ok(PackageCache {
        info: Arc::new(info),
        children,
    })
}

impl Archive {
    fn package_data(&self) -> Result<&PackageCache, Error> {
        if !matches!(self.kind(), ArchiveKind::F3z) {
            return Err(error(
                ErrorKind::Unsupported,
                "package.not_f3z",
                "package layer requires F3Z",
                "Manifest.json",
            )
            .source_name(self.source_name()));
        }
        self.package_cache
            .get_or_init(|| {
                inspect(self).map_err(|e| {
                    let mut e = e;
                    locate(&mut e.diagnostic);
                    if let Some(path) = &e.diagnostic.source
                        && self.members().iter().any(|m| &m.name == path)
                    {
                        e.diagnostic.source = Some(format!("{}!{}", self.source_name(), path));
                    }
                    e.source_if_missing(self.source_name())
                })
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    pub fn read_package(&self, strict: bool) -> Result<Arc<PackageInfo>, Error> {
        let info = self.package_data()?.info.clone();
        if strict && info.status != Status::Complete {
            let kind = if info.status == Status::Invalid {
                ErrorKind::Reference
            } else if info.status == Status::Unsupported {
                ErrorKind::Unsupported
            } else {
                ErrorKind::Reference
            };
            let mut e = error(
                kind,
                "package.partial",
                "package dependencies are not complete",
                "DesignDescription.json",
            )
            .source_name(self.source_name());
            let severity = if info.status == Status::Invalid {
                "error"
            } else if info.status == Status::Unsupported {
                "unsupported"
            } else {
                "warning"
            };
            if let Some(d) = info
                .issues
                .iter()
                .find(|d| d.severity == severity)
                .or(info.issues.first())
            {
                e.diagnostic = Box::new(d.clone());
                if info.status != Status::Invalid && d.stage != "relationships" {
                    e.kind = ErrorKind::Unsupported;
                }
            }
            e.diagnostics = info.issues.clone();
            e.result_status = Some(info.status);
            return Err(e);
        }
        Ok(info)
    }

    /// Open a child by its package-scoped member path. The returned archive owns its snapshot.
    pub fn open_document(&self, document_id: &str, strict: bool) -> Result<Arc<Archive>, Error> {
        let info = self.read_package(strict)?;
        if info
            .documents
            .iter()
            .any(|d| d.id == document_id && d.metadata.is_none())
        {
            return Err(error(
                ErrorKind::Unsupported,
                "document.decoder_unsupported",
                "document requires another decoder",
                document_id,
            )
            .source_name(self.source_name()));
        }
        self.package_data()?
            .children
            .get(document_id)
            .cloned()
            .ok_or_else(|| {
                error(
                    ErrorKind::MemberNotFound,
                    "document.not_found",
                    "unknown F3D document member ID",
                    document_id,
                )
                .source_name(self.source_name())
            })
    }

    pub(crate) fn package_preview(
        &self,
        document_id: Option<&str>,
        strict: bool,
    ) -> Result<Option<Vec<u8>>, Error> {
        let info = self.read_package(strict)?;
        let id = document_id
            .or(info.root_document_id.as_deref())
            .ok_or_else(|| {
                error(
                    ErrorKind::Reference,
                    "package.root_unresolved",
                    "package has no uniquely resolved F3D root",
                    "Manifest.json",
                )
            })?;
        let child = self.open_document(id, false)?;
        let metadata = child.read_document(strict)?;
        let Some(previews) = &metadata.preview_members else {
            return Err(error(
                ErrorKind::Unsupported,
                "preview.design_unresolved",
                "Design owner is unresolved",
                id,
            ));
        };
        let default = format!(
            "{}/Previews/small.png",
            metadata.design_asset_folder.as_deref().unwrap_or("")
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
            error(
                ErrorKind::Format,
                "preview.ambiguous",
                "multiple previews without a known default",
                id,
            )
        })?;
        let mut total = info.bytes_decompressed;
        child.read_member_budgeted(selected, &mut total).map(Some)
    }
}
