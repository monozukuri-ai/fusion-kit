use fusion_kit_core::Limits;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

mod document;
mod package;
mod reader;

/// Immutable resource policy. Byte limits apply to individual archive operations;
/// constructing this object does not allocate those amounts of memory.
#[pyclass(name = "Limits", module = "fusion_kit", frozen)]
struct PyLimits {
    inner: Limits,
}

#[pymethods]
impl PyLimits {
    #[new]
    #[pyo3(signature = (*, max_input_bytes=None, max_member_bytes=None, max_total_uncompressed_bytes=None, max_entries=None, max_nesting_depth=None))]
    fn new(
        max_input_bytes: Option<u64>,
        max_member_bytes: Option<u64>,
        max_total_uncompressed_bytes: Option<u64>,
        max_entries: Option<u64>,
        max_nesting_depth: Option<u64>,
    ) -> PyResult<Self> {
        let defaults = Limits::default();
        let inner = Limits::new(
            max_input_bytes.unwrap_or(defaults.max_input_bytes()),
            max_member_bytes.unwrap_or(defaults.max_member_bytes()),
            max_total_uncompressed_bytes.unwrap_or(defaults.max_total_uncompressed_bytes()),
            max_entries.unwrap_or(defaults.max_entries()),
            max_nesting_depth.unwrap_or(defaults.max_nesting_depth()),
        )
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
        Ok(Self { inner })
    }

    #[getter]
    fn max_input_bytes(&self) -> u64 {
        self.inner.max_input_bytes()
    }

    #[getter]
    fn max_member_bytes(&self) -> u64 {
        self.inner.max_member_bytes()
    }

    #[getter]
    fn max_total_uncompressed_bytes(&self) -> u64 {
        self.inner.max_total_uncompressed_bytes()
    }

    #[getter]
    fn max_entries(&self) -> u64 {
        self.inner.max_entries()
    }

    #[getter]
    fn max_nesting_depth(&self) -> u64 {
        self.inner.max_nesting_depth()
    }

    fn __repr__(&self) -> String {
        format!(
            "Limits(max_input_bytes={}, max_member_bytes={}, max_total_uncompressed_bytes={}, max_entries={}, max_nesting_depth={})",
            self.inner.max_input_bytes(),
            self.inner.max_member_bytes(),
            self.inner.max_total_uncompressed_bytes(),
            self.inner.max_entries(),
            self.inner.max_nesting_depth(),
        )
    }
}

#[pymodule]
mod _native {
    use pyo3::prelude::*;

    #[pymodule_export]
    use super::PyLimits;

    #[pymodule_export]
    use super::package::{
        PyGraphInfo, PyGraphNode, PyJsonSidecar, PyLayerComparison, PyPackageDocument,
        PyPackageInfo, PyRedirectionInfo, PyReferenceInfo,
    };

    #[pymodule_export]
    use super::document::{
        PyAssetInfo, PyDocumentInfo, PyDocumentManifest, PyManifestTail, PyRawSpan, PySegmentInfo,
        PySegmentInstance, PyTerminalInfo,
    };

    #[pymodule_export]
    use super::reader::{
        FormatError, FusionError, PyArchive, PyDiagnostic, PyMemberInfo, PyValidationReport,
        ReferenceResolutionError, ResourceLimitError, UnsupportedFeatureError, read, readfile,
    };

    #[pymodule_init]
    fn init(module: &Bound<'_, PyModule>) -> PyResult<()> {
        module.add("__version__", env!("CARGO_PKG_VERSION"))?;
        module.add("__core_version__", fusion_kit_core::VERSION)?;
        Ok(())
    }
}
