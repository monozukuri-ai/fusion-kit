"""Schema-v1 JSON contracts for independent copies returned by to_dict().

Model properties are immutable native views. Unknown sidecar values stay JSONValue.
"""

from __future__ import annotations

from typing import Literal, TypeAlias, TypedDict

Status: TypeAlias = Literal["complete", "partial", "unsupported", "invalid"]
ExitCode: TypeAlias = Literal[0, 1, 2]
ArchiveKind: TypeAlias = Literal["f3d", "f3z"]
Severity: TypeAlias = Literal["error", "warning", "unsupported"]
OffsetBasis: TypeAlias = Literal["archive", "member"]
ValidationScope: TypeAlias = Literal["outer_archive_members", "package_archive_members"]
AssetRole: TypeAlias = Literal["design", "derived_fusion", "other", "unresolved"]
SegmentRole: TypeAlias = Literal["design", "act", "browser", "other"]
SegmentStatus: TypeAlias = Literal["complete", "unsupported", "not_requested"]
JSONValue: TypeAlias = (
    None | bool | int | float | str | list["JSONValue"] | dict[str, "JSONValue"]
)


class DiagnosticJSON(TypedDict):
    severity: Severity
    code: str
    stage: str
    source: str | None
    member: str | None
    offset: int | None
    offset_basis: OffsetBasis | None
    context: str | None
    message: str


class MemberJSON(TypedDict):
    name: str
    compression_method: int
    compressed_size: int
    uncompressed_size: int
    crc32: int
    is_directory: bool
    encrypted: bool
    data_offset: int


class ArchiveJSON(TypedDict):
    schema_version: Literal[1]
    kind: ArchiveKind
    source: str
    source_sha256: str
    source_size: int
    status: Status
    scope: Literal["outer_archive_index"]
    verification: Literal["not_run"]
    manifest: Literal["not_requested"]
    relationships: Literal["not_requested"]
    geometry: Literal["not_requested"]
    members: list[MemberJSON]
    issues: list[DiagnosticJSON]


class ValidationJSON(TypedDict):
    schema_version: Literal[1]
    source_sha256: str
    status: Status
    scope: ValidationScope
    members_total: int
    members_verified: int
    bytes_decompressed: int
    nested_archives: Literal["complete", "partial", "not_requested"]
    issues: list[DiagnosticJSON]


class ErrorJSON(TypedDict):
    schema_version: Literal[1]
    status: Status
    issues: list[DiagnosticJSON]
    report: ValidationJSON | None


class RawSpanJSON(TypedDict):
    source_sha256: str
    member: str
    offset: int
    length: int
    sha256: str
    purpose: str


class TerminalJSON(TypedDict):
    raw: RawSpanJSON
    form: str
    flag: int | None
    marker: str | None
    display_name: str | None
    lineage_urn: str | None
    suffix: RawSpanJSON | None


class ManifestTailJSON(TypedDict):
    offset: int
    active_asset_guid: str
    primary_asset_bases: list[str]
    secondary_asset_bases: list[str]
    terminal: TerminalJSON


class ManifestJSON(TypedDict):
    format_version: str
    document_kind: str
    extension: str
    name: str
    description: str
    document_guid: str
    document_asset_guid: str
    generation_form: Literal["legacy", "cookie-1234"]
    generation_words: list[int]
    schema: dict[str, int]
    generation_extension: RawSpanJSON | None
    tail: ManifestTailJSON | None
    framing_complete: bool
    unknown_regions: list[RawSpanJSON]
    issues: list[DiagnosticJSON]


class SegmentInstanceJSON(TypedDict):
    folder: str
    meta_member: str | None
    bulk_member: str | None


class SegmentJSON(TypedDict):
    slot: int
    prefix: str
    segment_type: str
    role: SegmentRole
    instances: list[SegmentInstanceJSON]


class AssetJSON(TypedDict):
    base: str
    folder: str
    id: str
    secondary_guid: str
    asset_type: str
    revision: int | None
    schema: dict[str, int] | None
    asset_kind: str | None
    subtype: str | None
    properties: dict[str, str] | None
    change_counter: int | None
    role: AssetRole
    is_active: bool
    segment_status: SegmentStatus
    segments: list[SegmentJSON] | None
    unknown_regions: list[RawSpanJSON]
    issues: list[DiagnosticJSON]


class DocumentJSON(TypedDict):
    schema_version: Literal[1]
    scope: Literal["f3d_document_and_design_registry"]
    source_sha256: str
    id: str | None
    format_version: str | None
    status: Status
    manifest_status: Status
    manifest: ManifestJSON | None
    assets: list[AssetJSON] | None
    active_asset_id: str | None
    design_asset_id: str | None
    design_asset_folder: str | None
    preview_members: list[str] | None
    metadata_bytes: int
    geometry: Literal["not_requested"]
    unknown_regions: list[RawSpanJSON]
    issues: list[DiagnosticJSON]


class ReferenceJSON(TypedDict):
    layer: Literal["graph", "redirections"]
    graph_index: int | None
    source_id: int | None
    target_id: int | None
    source: str | None
    target: str | None
    kind: str | None
    neutron_role: str | None
    neutron_data: str | None
    group_index: int
    relationship_index: int | None


class GraphNodeJSON(TypedDict):
    object_id: int
    path: str | None
    content_type: str | None


class GraphJSON(TypedDict):
    graph_index: int
    root_ids: list[int]
    nodes: list[GraphNodeJSON]
    references: list[ReferenceJSON]
    reachable_ids: list[int]
    unreachable_ids: list[int]
    cycles: list[list[int]]


class RedirectionJSON(TypedDict):
    document_id: str
    designs: list[JSONValue]
    references: list[ReferenceJSON]


class ComparisonJSON(TypedDict):
    graph_index: int
    source: str
    matches: bool


class SidecarJSON(TypedDict):
    document_id: str | None
    raw: RawSpanJSON
    value: JSONValue


class PackageDocumentJSON(TypedDict):
    id: str
    package_sha256: str
    source_sha256: str | None
    document_guid: str | None
    status: Status
    metadata: DocumentJSON | None


class PackageJSON(TypedDict):
    schema_version: Literal[1]
    scope: Literal["f3z_document_dependencies"]
    source_sha256: str
    status: Status
    declared_root: str | None
    root_document_id: str | None
    documents: list[PackageDocumentJSON]
    graphs: list[GraphJSON]
    references: list[ReferenceJSON]
    redirections: list[RedirectionJSON]
    comparisons: list[ComparisonJSON]
    undocumented_members: list[str]
    raw_sidecars: list[SidecarJSON]
    bytes_decompressed: int
    members_indexed: int
    verification: Literal["metadata_members_only"]
    occurrences: Literal["not_requested"]
    geometry: Literal["not_requested"]
    issues: list[DiagnosticJSON]
