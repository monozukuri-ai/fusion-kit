use std::sync::Arc;

use fusion_kit_core::{
    AssetInfo, DocumentInfo, DocumentManifest, ManifestTail, RawSpan, SegmentInfo, SegmentInstance,
    TerminalInfo,
};
use pyo3::prelude::*;

use super::reader::{PyDiagnostic, json_to_py};

#[pyclass(name = "RawSpan", module = "fusion_kit", frozen)]
#[derive(Clone)]
pub struct PyRawSpan {
    pub(crate) inner: RawSpan,
}

#[pymethods]
impl PyRawSpan {
    #[getter]
    fn member(&self) -> &str {
        &self.inner.member
    }
    #[getter]
    fn source_sha256(&self) -> &str {
        &self.inner.source_sha256
    }
    #[getter]
    fn offset(&self) -> u64 {
        self.inner.offset
    }
    #[getter]
    fn length(&self) -> u64 {
        self.inner.length
    }
    #[getter]
    fn sha256(&self) -> &str {
        &self.inner.sha256
    }
    #[getter]
    fn purpose(&self) -> &str {
        &self.inner.purpose
    }
}

fn spans(values: &[RawSpan]) -> Vec<PyRawSpan> {
    values
        .iter()
        .map(|s| PyRawSpan { inner: s.clone() })
        .collect()
}

#[pyclass(name = "TerminalInfo", module = "fusion_kit", frozen)]
pub struct PyTerminalInfo {
    inner: TerminalInfo,
}

#[pymethods]
impl PyTerminalInfo {
    #[getter]
    fn form(&self) -> &str {
        &self.inner.form
    }
    #[getter]
    fn flag(&self) -> Option<u8> {
        self.inner.flag
    }
    #[getter]
    fn marker(&self) -> Option<&str> {
        self.inner.marker.as_deref()
    }
    #[getter]
    fn display_name(&self) -> Option<&str> {
        self.inner.display_name.as_deref()
    }
    #[getter]
    fn lineage_urn(&self) -> Option<&str> {
        self.inner.lineage_urn.as_deref()
    }
    #[getter]
    fn raw(&self) -> PyRawSpan {
        PyRawSpan {
            inner: self.inner.raw.clone(),
        }
    }
    #[getter]
    fn suffix(&self) -> Option<PyRawSpan> {
        self.inner.suffix.clone().map(|inner| PyRawSpan { inner })
    }
}

#[pyclass(name = "ManifestTail", module = "fusion_kit", frozen)]
pub struct PyManifestTail {
    inner: ManifestTail,
}

#[pymethods]
impl PyManifestTail {
    #[getter]
    fn offset(&self) -> u64 {
        self.inner.offset
    }
    #[getter]
    fn active_asset_guid(&self) -> &str {
        &self.inner.active_asset_guid
    }
    #[getter]
    fn primary_asset_bases(&self) -> Vec<String> {
        self.inner.primary_asset_bases.clone()
    }
    #[getter]
    fn secondary_asset_bases(&self) -> Vec<String> {
        self.inner.secondary_asset_bases.clone()
    }
    #[getter]
    fn terminal(&self) -> PyTerminalInfo {
        PyTerminalInfo {
            inner: self.inner.terminal.clone(),
        }
    }
}

#[pyclass(name = "DocumentManifest", module = "fusion_kit", frozen)]
pub struct PyDocumentManifest {
    inner: DocumentManifest,
}

#[pymethods]
impl PyDocumentManifest {
    #[getter]
    fn issues(&self) -> Vec<PyDiagnostic> {
        self.inner.issues.iter().map(Into::into).collect()
    }
    #[getter]
    fn format_version(&self) -> &str {
        &self.inner.format_version
    }
    #[getter]
    fn document_kind(&self) -> &str {
        &self.inner.document_kind
    }
    #[getter]
    fn extension(&self) -> &str {
        &self.inner.extension
    }
    #[getter]
    fn name(&self) -> &str {
        &self.inner.name
    }
    #[getter]
    fn description(&self) -> &str {
        &self.inner.description
    }
    #[getter]
    fn document_guid(&self) -> &str {
        &self.inner.document_guid
    }
    #[getter]
    fn document_asset_guid(&self) -> &str {
        &self.inner.document_asset_guid
    }
    #[getter]
    fn generation_form(&self) -> &str {
        self.inner.generation_form
    }
    #[getter]
    fn generation_words(&self) -> Vec<u32> {
        self.inner.generation_words.clone()
    }
    #[getter]
    fn schema(&self) -> std::collections::BTreeMap<String, u32> {
        self.inner.schema.clone()
    }
    #[getter]
    fn generation_extension(&self) -> Option<PyRawSpan> {
        self.inner
            .generation_extension
            .clone()
            .map(|inner| PyRawSpan { inner })
    }
    #[getter]
    fn tail(&self) -> Option<PyManifestTail> {
        self.inner
            .tail
            .clone()
            .map(|inner| PyManifestTail { inner })
    }
    #[getter]
    fn framing_complete(&self) -> bool {
        self.inner.framing_complete
    }
    #[getter]
    fn unknown_regions(&self) -> Vec<PyRawSpan> {
        spans(&self.inner.unknown_regions)
    }
}

#[pyclass(name = "SegmentInstance", module = "fusion_kit", frozen, get_all)]
pub struct PySegmentInstance {
    folder: String,
    meta_member: Option<String>,
    bulk_member: Option<String>,
}

impl From<&SegmentInstance> for PySegmentInstance {
    fn from(s: &SegmentInstance) -> Self {
        Self {
            folder: s.folder.clone(),
            meta_member: s.meta_member.clone(),
            bulk_member: s.bulk_member.clone(),
        }
    }
}

#[pyclass(name = "SegmentInfo", module = "fusion_kit", frozen)]
pub struct PySegmentInfo {
    inner: SegmentInfo,
}

#[pymethods]
impl PySegmentInfo {
    #[getter]
    fn slot(&self) -> u32 {
        self.inner.slot
    }
    #[getter]
    fn prefix(&self) -> &str {
        &self.inner.prefix
    }
    #[getter]
    fn segment_type(&self) -> &str {
        &self.inner.segment_type
    }
    #[getter]
    fn role(&self) -> &str {
        self.inner.role
    }
    #[getter]
    fn instances(&self) -> Vec<PySegmentInstance> {
        self.inner.instances.iter().map(Into::into).collect()
    }
}

#[pyclass(name = "AssetInfo", module = "fusion_kit", frozen)]
pub struct PyAssetInfo {
    inner: AssetInfo,
}

#[pymethods]
impl PyAssetInfo {
    #[getter]
    fn issues(&self) -> Vec<PyDiagnostic> {
        self.inner.issues.iter().map(Into::into).collect()
    }
    #[getter]
    fn base(&self) -> &str {
        &self.inner.base
    }
    #[getter]
    fn folder(&self) -> &str {
        &self.inner.folder
    }
    #[getter]
    fn id(&self) -> &str {
        &self.inner.id
    }
    #[getter]
    fn secondary_guid(&self) -> &str {
        &self.inner.secondary_guid
    }
    #[getter]
    fn asset_type(&self) -> &str {
        &self.inner.asset_type
    }
    #[getter]
    fn revision(&self) -> Option<u32> {
        self.inner.revision
    }
    #[getter]
    fn schema(&self) -> Option<std::collections::BTreeMap<String, u32>> {
        self.inner.schema.clone()
    }
    #[getter]
    fn asset_kind(&self) -> Option<&str> {
        self.inner.asset_kind.as_deref()
    }
    #[getter]
    fn subtype(&self) -> Option<&str> {
        self.inner.subtype.as_deref()
    }
    #[getter]
    fn properties(&self) -> Option<std::collections::BTreeMap<String, String>> {
        self.inner.properties.clone()
    }
    #[getter]
    fn change_counter(&self) -> Option<u32> {
        self.inner.change_counter
    }
    #[getter]
    fn role(&self) -> &str {
        self.inner.role
    }
    #[getter]
    fn is_active(&self) -> bool {
        self.inner.is_active
    }
    #[getter]
    fn segment_status(&self) -> &str {
        self.inner.segment_status
    }
    #[getter]
    fn segments(&self) -> Option<Vec<PySegmentInfo>> {
        self.inner.segments.as_ref().map(|s| {
            s.iter()
                .map(|s| PySegmentInfo { inner: s.clone() })
                .collect()
        })
    }
    #[getter]
    fn unknown_regions(&self) -> Vec<PyRawSpan> {
        spans(&self.inner.unknown_regions)
    }
}

#[pyclass(name = "DocumentInfo", module = "fusion_kit", frozen)]
pub struct PyDocumentInfo {
    pub(crate) inner: Arc<DocumentInfo>,
}

#[pymethods]
impl PyDocumentInfo {
    #[getter]
    fn exit_code(&self) -> i32 {
        self.inner.status.exit_code()
    }
    #[getter]
    fn id(&self) -> Option<&str> {
        self.inner.id.as_deref()
    }
    #[getter]
    fn format_version(&self) -> Option<&str> {
        self.inner.format_version.as_deref()
    }
    #[getter]
    fn source_sha256(&self) -> &str {
        &self.inner.source_sha256
    }
    #[getter]
    fn status(&self) -> &str {
        self.inner.status.as_str()
    }
    #[getter]
    fn scope(&self) -> &str {
        self.inner.scope
    }
    #[getter]
    fn manifest_status(&self) -> &str {
        self.inner.manifest_status.as_str()
    }
    #[getter]
    fn manifest(&self) -> Option<PyDocumentManifest> {
        self.inner
            .manifest
            .clone()
            .map(|inner| PyDocumentManifest { inner })
    }
    #[getter]
    fn assets(&self) -> Option<Vec<PyAssetInfo>> {
        self.inner
            .assets
            .as_ref()
            .map(|a| a.iter().map(|a| PyAssetInfo { inner: a.clone() }).collect())
    }
    #[getter]
    fn active_asset_id(&self) -> Option<&str> {
        self.inner.active_asset_id.as_deref()
    }
    #[getter]
    fn design_asset_id(&self) -> Option<&str> {
        self.inner.design_asset_id.as_deref()
    }
    #[getter]
    fn design_asset_folder(&self) -> Option<&str> {
        self.inner.design_asset_folder.as_deref()
    }
    #[getter]
    fn preview_members(&self) -> Option<Vec<String>> {
        self.inner.preview_members.clone()
    }
    #[getter]
    fn metadata_bytes(&self) -> u64 {
        self.inner.metadata_bytes
    }
    #[getter]
    fn unknown_regions(&self) -> Vec<PyRawSpan> {
        spans(&self.inner.unknown_regions)
    }
    #[getter]
    fn issues(&self) -> Vec<PyDiagnostic> {
        self.inner.issues.iter().map(Into::into).collect()
    }
    fn to_json(&self, py: Python<'_>) -> String {
        let inner = self.inner.clone();
        py.detach(move || inner.to_json())
    }
    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        json_to_py(py, &self.to_json(py))
    }
}
