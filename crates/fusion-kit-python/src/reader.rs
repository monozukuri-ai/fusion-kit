use std::path::PathBuf;
use std::sync::Arc;

use fusion_kit_core::{
    Archive, Diagnostic, Error, ErrorKind, InputSnapshot, MemberInfo, ReadOptions, ValidationReport,
};
use pyo3::exceptions::{PyException, PyKeyError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyFloat, PyInt, PyList, PyString};
use serde_json::Value;

use super::PyLimits;
use super::document::{PyDocumentInfo, PyRawSpan};
use super::package::{PyPackageInfo, PyReferenceInfo};

pyo3::create_exception!(fusion_kit, FusionError, PyException);
pyo3::create_exception!(fusion_kit, FormatError, FusionError);
pyo3::create_exception!(fusion_kit, UnsupportedFeatureError, FusionError);
pyo3::create_exception!(fusion_kit, ResourceLimitError, FusionError);
pyo3::create_exception!(fusion_kit, ReferenceResolutionError, FusionError);

#[pyclass(name = "Diagnostic", module = "fusion_kit", frozen, get_all)]
pub struct PyDiagnostic {
    severity: String,
    code: String,
    stage: String,
    source: Option<String>,
    member: Option<String>,
    offset: Option<u64>,
    offset_basis: Option<String>,
    context: Option<String>,
    message: String,
}

impl From<&Diagnostic> for PyDiagnostic {
    fn from(d: &Diagnostic) -> Self {
        Self {
            severity: d.severity.into(),
            code: d.code.into(),
            stage: d.stage.into(),
            source: d.source.clone(),
            member: d.member.clone(),
            offset: d.offset,
            offset_basis: d.offset_basis.map(str::to_owned),
            context: d.context.clone(),
            message: d.message.clone(),
        }
    }
}

#[pyclass(name = "MemberInfo", module = "fusion_kit", frozen, get_all)]
pub struct PyMemberInfo {
    name: String,
    compression_method: u16,
    compressed_size: u64,
    uncompressed_size: u64,
    crc32: u32,
    is_directory: bool,
    encrypted: bool,
    data_offset: u64,
}

impl From<&MemberInfo> for PyMemberInfo {
    fn from(m: &MemberInfo) -> Self {
        Self {
            name: m.name.clone(),
            compression_method: m.compression_method,
            compressed_size: m.compressed_size,
            uncompressed_size: m.uncompressed_size,
            crc32: m.crc32,
            is_directory: m.is_directory,
            encrypted: m.encrypted,
            data_offset: m.data_offset,
        }
    }
}

#[pyclass(name = "Archive", module = "fusion_kit", frozen)]
pub struct PyArchive {
    inner: Arc<Archive>,
    strict: bool,
}

#[pymethods]
impl PyArchive {
    #[pyo3(signature=(*,strict=None))]
    fn read_package(&self, py: Python<'_>, strict: Option<bool>) -> PyResult<PyPackageInfo> {
        let inner = self.inner.clone();
        let strict = strict.unwrap_or(self.strict);
        let inner = py
            .detach(move || inner.read_package(strict))
            .map_err(|e| convert_error(py, e))?;
        Ok(PyPackageInfo { inner })
    }
    #[pyo3(signature=(document_id, *, strict=None))]
    fn open_document(
        &self,
        py: Python<'_>,
        document_id: String,
        strict: Option<bool>,
    ) -> PyResult<PyArchive> {
        let inner = self.inner.clone();
        let strict = strict.unwrap_or(self.strict);
        let inner = py
            .detach(move || inner.open_document(&document_id, strict))
            .map_err(|e| convert_error(py, e))?;
        Ok(PyArchive { inner, strict })
    }
    #[getter]
    fn references(&self, py: Python<'_>) -> PyResult<Vec<PyReferenceInfo>> {
        Ok(self
            .read_package(py, None)?
            .inner
            .references
            .iter()
            .map(Into::into)
            .collect())
    }
    #[pyo3(signature=(*,strict=None))]
    fn read_document(&self, py: Python<'_>, strict: Option<bool>) -> PyResult<PyDocumentInfo> {
        let inner = self.inner.clone();
        let strict = strict.unwrap_or(self.strict);
        let inner = py
            .detach(move || inner.read_document(strict))
            .map_err(|e| convert_error(py, e))?;
        Ok(PyDocumentInfo { inner })
    }
    #[getter]
    fn documents(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        if matches!(self.inner.kind(), fusion_kit_core::ArchiveKind::F3z) {
            Ok(self
                .read_package(py, None)?
                .documents()
                .into_pyobject(py)?
                .into_any()
                .unbind())
        } else {
            Ok(vec![self.read_document(py, None)?]
                .into_pyobject(py)?
                .into_any()
                .unbind())
        }
    }
    #[getter]
    fn root_document_id(&self, py: Python<'_>) -> PyResult<Option<String>> {
        if matches!(self.inner.kind(), fusion_kit_core::ArchiveKind::F3z) {
            return Ok(self.read_package(py, None)?.inner.root_document_id.clone());
        }
        Ok(self.read_document(py, None)?.inner.id.clone())
    }
    fn read_span<'py>(
        &self,
        py: Python<'py>,
        region: PyRef<'_, PyRawSpan>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let inner = self.inner.clone();
        let region = region.inner.clone();
        let bytes = py
            .detach(move || inner.read_span(&region))
            .map_err(|e| convert_error(py, e))?;
        Ok(PyBytes::new(py, &bytes))
    }
    #[pyo3(signature=(*,document_id=None,strict=None))]
    fn get_preview<'py>(
        &self,
        py: Python<'py>,
        document_id: Option<String>,
        strict: Option<bool>,
    ) -> PyResult<Option<Bound<'py, PyBytes>>> {
        let inner = self.inner.clone();
        let strict = strict.unwrap_or(self.strict);
        let bytes = py
            .detach(move || inner.get_preview(document_id.as_deref(), strict))
            .map_err(|e| convert_error(py, e))?;
        Ok(bytes.as_ref().map(|b| PyBytes::new(py, b)))
    }
    #[getter]
    fn kind(&self) -> &'static str {
        self.inner.kind().as_str()
    }
    #[getter]
    fn status(&self) -> &'static str {
        self.inner.status().as_str()
    }
    #[getter]
    fn exit_code(&self) -> i32 {
        self.inner.status().exit_code()
    }
    #[getter]
    fn source_sha256(&self) -> &str {
        self.inner.source_sha256()
    }
    #[getter]
    fn source_size(&self) -> u64 {
        self.inner.source_size()
    }
    #[getter]
    fn source(&self) -> &str {
        self.inner.source_name()
    }
    #[getter]
    fn members(&self) -> Vec<PyMemberInfo> {
        self.inner.members().iter().map(Into::into).collect()
    }
    #[getter]
    fn issues(&self) -> Vec<PyDiagnostic> {
        self.inner.issues().iter().map(Into::into).collect()
    }

    fn read_member<'py>(&self, py: Python<'py>, name: String) -> PyResult<Bound<'py, PyBytes>> {
        let inner = self.inner.clone();
        let bytes = py
            .detach(move || inner.read_member(&name))
            .map_err(|e| convert_error(py, e))?;
        Ok(PyBytes::new(py, &bytes))
    }

    #[pyo3(signature=(*, strict=true, recursive=true))]
    fn verify(
        &self,
        py: Python<'_>,
        strict: bool,
        recursive: bool,
    ) -> PyResult<PyValidationReport> {
        let inner = self.inner.clone();
        let report = py
            .detach(move || {
                if recursive {
                    inner.verify(strict)
                } else {
                    inner.verify_outer(strict)
                }
            })
            .map_err(|e| convert_error(py, e))?;
        Ok(PyValidationReport { inner: report })
    }

    fn to_json(&self, py: Python<'_>) -> String {
        let inner = self.inner.clone();
        py.detach(move || inner.to_json())
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        json_to_py(py, &self.to_json(py))
    }

    fn __repr__(&self) -> String {
        format!(
            "Archive(kind='{}', members={}, status='{}')",
            self.kind(),
            self.inner.members().len(),
            self.status()
        )
    }
}

#[pyclass(name = "ValidationReport", module = "fusion_kit", frozen)]
pub struct PyValidationReport {
    inner: ValidationReport,
}

#[pymethods]
impl PyValidationReport {
    #[getter]
    fn exit_code(&self) -> i32 {
        self.inner.status.exit_code()
    }
    #[getter]
    fn nested_archives(&self) -> &str {
        self.inner.nested_archives
    }
    #[getter]
    fn status(&self) -> &'static str {
        self.inner.status.as_str()
    }
    #[getter]
    fn scope(&self) -> &'static str {
        self.inner.scope
    }
    #[getter]
    fn source_sha256(&self) -> &str {
        &self.inner.source_sha256
    }
    #[getter]
    fn members_total(&self) -> usize {
        self.inner.members_total
    }
    #[getter]
    fn members_verified(&self) -> usize {
        self.inner.members_verified
    }
    #[getter]
    fn bytes_decompressed(&self) -> u64 {
        self.inner.bytes_decompressed
    }
    #[getter]
    fn issues(&self) -> Vec<PyDiagnostic> {
        self.inner.issues.iter().map(Into::into).collect()
    }
    fn to_json(&self, py: Python<'_>) -> String {
        py.detach(|| self.inner.to_json())
    }
    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        json_to_py(py, &self.to_json(py))
    }
}

#[pyfunction]
#[pyo3(signature=(path, *, limits=None, strict=true))]
pub fn readfile(
    py: Python<'_>,
    path: PathBuf,
    limits: Option<PyRef<'_, PyLimits>>,
    strict: bool,
) -> PyResult<PyArchive> {
    let options = ReadOptions {
        limits: limits.map_or_else(Default::default, |l| l.inner),
        strict,
    };
    let inner = py
        .detach(move || Archive::open(path, options))
        .map_err(|e| convert_error(py, e))?;
    Ok(PyArchive {
        inner: Arc::new(inner),
        strict,
    })
}

#[pyfunction]
#[pyo3(signature=(data, *, limits=None, strict=true))]
pub fn read(
    py: Python<'_>,
    data: &Bound<'_, PyBytes>,
    limits: Option<PyRef<'_, PyLimits>>,
    strict: bool,
) -> PyResult<PyArchive> {
    let options = ReadOptions {
        limits: limits.map_or_else(Default::default, |l| l.inner),
        strict,
    };
    let bytes = data.as_bytes();
    if bytes.len() as u64 > options.limits.max_input_bytes() {
        return Err(convert_error(
            py,
            Error::new(
                ErrorKind::ResourceLimit,
                "limit.input_bytes",
                "source",
                "input exceeds max_input_bytes",
            ),
        ));
    }
    let mut owned = Vec::new();
    owned.try_reserve_exact(bytes.len()).map_err(|_| {
        convert_error(
            py,
            Error::new(
                ErrorKind::ResourceLimit,
                "limit.allocation",
                "source",
                "unable to copy input bytes",
            ),
        )
    })?;
    owned.extend_from_slice(bytes);
    let inner = py
        .detach(move || {
            Archive::from_snapshot(InputSnapshot::from_vec(owned, options.limits)?, options)
        })
        .map_err(|e| convert_error(py, e))?;
    Ok(PyArchive {
        inner: Arc::new(inner),
        strict,
    })
}

fn convert_error(py: Python<'_>, error: Error) -> PyErr {
    let result = match error.kind {
        ErrorKind::Format => FormatError::new_err(error.to_string()),
        ErrorKind::Unsupported => UnsupportedFeatureError::new_err(error.to_string()),
        ErrorKind::Reference => ReferenceResolutionError::new_err(error.to_string()),
        ErrorKind::ResourceLimit => ResourceLimitError::new_err(error.to_string()),
        ErrorKind::MemberNotFound => PyKeyError::new_err(error.to_string()),
        ErrorKind::Io => {
            let original = error
                .io_error
                .as_ref()
                .expect("I/O errors retain their source");
            let copy = original.raw_os_error().map_or_else(
                || std::io::Error::new(original.kind(), original.to_string()),
                std::io::Error::from_raw_os_error,
            );
            PyErr::from(copy)
        }
    };
    let value = result.value(py);
    let d = &error.diagnostic;
    for (name, item) in [
        ("code", d.code),
        ("stage", d.stage),
        ("message", d.message.as_str()),
    ] {
        if let Err(error) = value.setattr(name, item) {
            return error;
        }
    }
    if let Err(error) = value
        .setattr("member", d.member.as_deref())
        .and_then(|_| value.setattr("offset", d.offset))
        .and_then(|_| value.setattr("source", d.source.as_deref()))
        .and_then(|_| value.setattr("context", d.context.as_deref()))
        .and_then(|_| value.setattr("offset_basis", d.offset_basis))
        .and_then(|_| value.setattr("status", error.status().as_str()))
        .and_then(|_| value.setattr("exit_code", error.status().exit_code()))
    {
        return error;
    }
    let issues: Vec<PyDiagnostic> = error.issues().iter().map(Into::into).collect();
    if let Err(error) = value.setattr("issues", issues) {
        return error;
    }
    let failure = py.detach(|| error.to_json());
    match json_to_py(py, &failure).and_then(|result| value.setattr("result", result)) {
        Ok(()) => {}
        Err(error) => return error,
    }
    match serde_json::to_string(d)
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
        .and_then(|s| json_to_py(py, &s))
    {
        Ok(diagnostic) => {
            if let Err(error) = value.setattr("diagnostic", diagnostic) {
                return error;
            }
        }
        Err(error) => return error,
    }
    let report = error.report.map(|r| PyValidationReport { inner: *r });
    if let Err(error) = value.setattr("report", report) {
        return error;
    }
    result
}

pub(crate) fn json_to_py(py: Python<'_>, text: &str) -> PyResult<Py<PyAny>> {
    let value: Value = serde_json::from_str(text)
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
    value_to_py(py, value)
}

fn value_to_py(py: Python<'_>, value: Value) -> PyResult<Py<PyAny>> {
    Ok(match value {
        Value::Null => py.None(),
        Value::Bool(v) => PyBool::new(py, v).to_owned().into_any().unbind(),
        Value::String(v) => PyString::new(py, &v).into_any().unbind(),
        Value::Number(v) => {
            if let Some(n) = v.as_u64() {
                PyInt::new(py, n).into_any().unbind()
            } else if let Some(n) = v.as_i64() {
                PyInt::new(py, n).into_any().unbind()
            } else {
                PyFloat::new(py, v.as_f64().unwrap_or(f64::NAN))
                    .into_any()
                    .unbind()
            }
        }
        Value::Array(values) => {
            let list = PyList::empty(py);
            for v in values {
                list.append(value_to_py(py, v)?)?;
            }
            list.into_any().unbind()
        }
        Value::Object(values) => {
            let dict = PyDict::new(py);
            for (k, v) in values {
                dict.set_item(k, value_to_py(py, v)?)?;
            }
            dict.into_any().unbind()
        }
    })
}
