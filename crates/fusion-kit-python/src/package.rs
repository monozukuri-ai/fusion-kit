use super::document::{PyDocumentInfo, PyRawSpan};
use super::reader::{PyDiagnostic, json_to_py};
use fusion_kit_core::{
    GraphInfo, GraphNode, JsonSidecar, LayerComparison, PackageDocument, PackageInfo,
    RedirectionInfo, ReferenceInfo,
};
use pyo3::prelude::*;
use std::sync::Arc;

#[pyclass(name = "ReferenceInfo", module = "fusion_kit", frozen, get_all)]
pub struct PyReferenceInfo {
    layer: String,
    graph_index: Option<usize>,
    source_id: Option<i64>,
    target_id: Option<i64>,
    source: Option<String>,
    target: Option<String>,
    kind: Option<String>,
    neutron_role: Option<String>,
    neutron_data: Option<String>,
    group_index: usize,
    relationship_index: Option<usize>,
}
impl From<&ReferenceInfo> for PyReferenceInfo {
    fn from(r: &ReferenceInfo) -> Self {
        Self {
            layer: r.layer.into(),
            graph_index: r.graph_index,
            source_id: r.source_id,
            target_id: r.target_id,
            source: r.source.clone(),
            target: r.target.clone(),
            kind: r.kind.clone(),
            neutron_role: r.neutron_role.clone(),
            neutron_data: r.neutron_data.clone(),
            group_index: r.group_index,
            relationship_index: r.relationship_index,
        }
    }
}

#[pyclass(name = "GraphNode", module = "fusion_kit", frozen, get_all)]
pub struct PyGraphNode {
    object_id: i64,
    path: Option<String>,
    content_type: Option<String>,
}
impl From<&GraphNode> for PyGraphNode {
    fn from(n: &GraphNode) -> Self {
        Self {
            object_id: n.object_id,
            path: n.path.clone(),
            content_type: n.content_type.clone(),
        }
    }
}

#[pyclass(name = "GraphInfo", module = "fusion_kit", frozen)]
pub struct PyGraphInfo {
    inner: GraphInfo,
}
#[pymethods]
impl PyGraphInfo {
    #[getter]
    fn graph_index(&self) -> usize {
        self.inner.graph_index
    }
    #[getter]
    fn root_ids(&self) -> Vec<i64> {
        self.inner.root_ids.clone()
    }
    #[getter]
    fn nodes(&self) -> Vec<PyGraphNode> {
        self.inner.nodes.iter().map(Into::into).collect()
    }
    #[getter]
    fn references(&self) -> Vec<PyReferenceInfo> {
        self.inner.references.iter().map(Into::into).collect()
    }
    #[getter]
    fn reachable_ids(&self) -> Vec<i64> {
        self.inner.reachable_ids.clone()
    }
    #[getter]
    fn unreachable_ids(&self) -> Vec<i64> {
        self.inner.unreachable_ids.clone()
    }
    #[getter]
    fn cycles(&self) -> Vec<Vec<i64>> {
        self.inner.cycles.clone()
    }
}

#[pyclass(name = "PackageDocument", module = "fusion_kit", frozen)]
pub struct PyPackageDocument {
    pub(crate) inner: PackageDocument,
}
#[pymethods]
impl PyPackageDocument {
    #[getter]
    fn id(&self) -> &str {
        &self.inner.id
    }
    #[getter]
    fn package_sha256(&self) -> &str {
        &self.inner.package_sha256
    }
    #[getter]
    fn source_sha256(&self) -> Option<&str> {
        self.inner.source_sha256.as_deref()
    }
    #[getter]
    fn document_guid(&self) -> Option<&str> {
        self.inner.document_guid.as_deref()
    }
    #[getter]
    fn status(&self) -> &str {
        self.inner.status.as_str()
    }
    #[getter]
    fn metadata(&self) -> Option<PyDocumentInfo> {
        self.inner
            .metadata
            .clone()
            .map(|inner| PyDocumentInfo { inner })
    }
}

#[pyclass(name = "RedirectionInfo", module = "fusion_kit", frozen)]
pub struct PyRedirectionInfo {
    inner: RedirectionInfo,
}
#[pymethods]
impl PyRedirectionInfo {
    #[getter]
    fn document_id(&self) -> &str {
        &self.inner.document_id
    }
    #[getter]
    fn references(&self) -> Vec<PyReferenceInfo> {
        self.inner.references.iter().map(Into::into).collect()
    }
    #[getter]
    fn designs(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        json_to_py(
            py,
            &serde_json::to_string(&self.inner.designs).expect("JSON-safe values"),
        )
    }
}

#[pyclass(name = "JsonSidecar", module = "fusion_kit", frozen)]
pub struct PyJsonSidecar {
    inner: JsonSidecar,
}
#[pymethods]
impl PyJsonSidecar {
    #[getter]
    fn document_id(&self) -> Option<&str> {
        self.inner.document_id.as_deref()
    }
    #[getter]
    fn raw(&self) -> PyRawSpan {
        PyRawSpan {
            inner: self.inner.raw.clone(),
        }
    }
    #[getter]
    fn value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        json_to_py(py, &self.inner.value.to_string())
    }
}

#[pyclass(name = "LayerComparison", module = "fusion_kit", frozen, get_all)]
pub struct PyLayerComparison {
    graph_index: usize,
    source: String,
    matches: bool,
}
impl From<&LayerComparison> for PyLayerComparison {
    fn from(c: &LayerComparison) -> Self {
        Self {
            graph_index: c.graph_index,
            source: c.source.clone(),
            matches: c.matches,
        }
    }
}

#[pyclass(name = "PackageInfo", module = "fusion_kit", frozen)]
pub struct PyPackageInfo {
    pub(crate) inner: Arc<PackageInfo>,
}
#[pymethods]
impl PyPackageInfo {
    #[getter]
    fn exit_code(&self) -> i32 {
        self.inner.status.exit_code()
    }
    #[getter]
    fn scope(&self) -> &str {
        self.inner.scope
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
    fn declared_root(&self) -> Option<&str> {
        self.inner.declared_root.as_deref()
    }
    #[getter]
    fn root_document_id(&self) -> Option<&str> {
        self.inner.root_document_id.as_deref()
    }
    #[getter]
    pub(crate) fn documents(&self) -> Vec<PyPackageDocument> {
        self.inner
            .documents
            .iter()
            .map(|d| PyPackageDocument { inner: d.clone() })
            .collect()
    }
    #[getter]
    fn graphs(&self) -> Vec<PyGraphInfo> {
        self.inner
            .graphs
            .iter()
            .map(|g| PyGraphInfo { inner: g.clone() })
            .collect()
    }
    #[getter]
    fn references(&self) -> Vec<PyReferenceInfo> {
        self.inner.references.iter().map(Into::into).collect()
    }
    #[getter]
    fn redirections(&self) -> Vec<PyRedirectionInfo> {
        self.inner
            .redirections
            .iter()
            .map(|r| PyRedirectionInfo { inner: r.clone() })
            .collect()
    }
    #[getter]
    fn comparisons(&self) -> Vec<PyLayerComparison> {
        self.inner.comparisons.iter().map(Into::into).collect()
    }
    #[getter]
    fn undocumented_members(&self) -> Vec<String> {
        self.inner.undocumented_members.clone()
    }
    #[getter]
    fn raw_sidecars(&self) -> Vec<PyJsonSidecar> {
        self.inner
            .raw_sidecars
            .iter()
            .map(|r| PyJsonSidecar { inner: r.clone() })
            .collect()
    }
    #[getter]
    fn bytes_decompressed(&self) -> u64 {
        self.inner.bytes_decompressed
    }
    #[getter]
    fn members_indexed(&self) -> u64 {
        self.inner.members_indexed
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
