//! Bounded, Python-independent Fusion container, document and package reader.

#![forbid(unsafe_code)]

mod archive;
mod diagnostic;
mod document;
mod json;
mod limits;
mod manifest;
mod package;
mod source;
mod verification;
mod zip_index;

pub use archive::{Archive, ArchiveKind, MemberInfo, ReadOptions, ValidationReport};
pub use diagnostic::{Diagnostic, Error, ErrorKind, Status};
pub use document::{AssetInfo, DocumentInfo, SegmentInfo, SegmentInstance};
pub use limits::{InvalidLimits, Limits};
pub use manifest::{DocumentManifest, ManifestTail, RawSpan, TerminalInfo};
pub use package::{
    GraphInfo, GraphNode, JsonSidecar, LayerComparison, PackageDocument, PackageInfo,
    RedirectionInfo, ReferenceInfo,
};
pub use source::InputSnapshot;

/// Package version, inherited from the Cargo workspace.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
