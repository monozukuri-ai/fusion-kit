use std::fmt;
use std::io;
use std::sync::Arc;

use serde::Serialize;

use crate::ValidationReport;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Complete,
    Partial,
    Unsupported,
    Invalid,
}

impl Status {
    pub const fn exit_code(self) -> i32 {
        match self {
            Self::Complete => 0,
            Self::Partial | Self::Unsupported => 2,
            Self::Invalid => 1,
        }
    }
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Unsupported => "unsupported",
            Self::Invalid => "invalid",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub severity: &'static str,
    pub code: &'static str,
    pub stage: &'static str,
    pub source: Option<String>,
    pub member: Option<String>,
    pub offset: Option<u64>,
    pub offset_basis: Option<&'static str>,
    pub context: Option<String>,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Format,
    Unsupported,
    Reference,
    ResourceLimit,
    MemberNotFound,
    Io,
}

#[derive(Clone, Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub diagnostic: Box<Diagnostic>,
    pub report: Option<Box<ValidationReport>>,
    pub io_error: Option<Arc<io::Error>>,
    pub diagnostics: Vec<Diagnostic>,
    pub result_status: Option<Status>,
}

impl Error {
    pub fn new(
        kind: ErrorKind,
        code: &'static str,
        stage: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            diagnostic: Box::new(Diagnostic {
                severity: "error",
                code,
                stage,
                source: None,
                member: None,
                offset: None,
                offset_basis: None,
                context: None,
                message: message.into(),
            }),
            report: None,
            io_error: None,
            diagnostics: Vec::new(),
            result_status: None,
        }
    }

    pub(crate) fn member(mut self, member: &str) -> Self {
        self.diagnostic.member = Some(member.to_owned());
        self
    }

    pub(crate) fn offset(mut self, offset: usize) -> Self {
        self.diagnostic.offset = Some(offset as u64);
        self.diagnostic.offset_basis = Some(
            if matches!(self.diagnostic.stage, "manifest" | "asset_manifest") {
                "member"
            } else {
                "archive"
            },
        );
        self
    }

    pub(crate) fn source_name(mut self, source: &str) -> Self {
        self.diagnostic.source = Some(source.to_owned());
        self
    }

    pub(crate) fn source_if_missing(mut self, source: &str) -> Self {
        if self.diagnostic.source.is_none() {
            self.diagnostic.source = Some(source.into());
        }
        for d in &mut self.diagnostics {
            if d.source.is_none() {
                d.source = Some(source.into());
            }
        }
        self
    }

    pub fn status(&self) -> Status {
        self.result_status
            .or_else(|| self.report.as_ref().map(|r| r.status))
            .unwrap_or(if self.kind == ErrorKind::Unsupported {
                Status::Unsupported
            } else {
                Status::Invalid
            })
    }

    pub fn issues(&self) -> Vec<Diagnostic> {
        if !self.diagnostics.is_empty() {
            self.diagnostics.clone()
        } else if let Some(report) = &self.report {
            report.issues.clone()
        } else {
            vec![(*self.diagnostic).clone()]
        }
    }

    /// Same schema-v1 failure envelope used by the Python CLI.
    pub fn to_json(&self) -> String {
        #[derive(Serialize)]
        struct Failure<'a> {
            schema_version: u32,
            status: Status,
            issues: Vec<Diagnostic>,
            report: Option<&'a ValidationReport>,
        }
        serde_json::to_string(&Failure {
            schema_version: 1,
            status: self.status(),
            issues: self.issues(),
            report: self.report.as_deref(),
        })
        .expect("diagnostics contain JSON-safe values")
    }

    pub(crate) fn io(error: io::Error, source: &str) -> Self {
        let mut result =
            Self::new(ErrorKind::Io, "source.io", "source", error.to_string()).source_name(source);
        result.io_error = Some(Arc::new(error));
        result
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.diagnostic.code, self.diagnostic.message)
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_exit_codes_and_partial_error_envelope_are_consistent() {
        for (status, code) in [
            (Status::Complete, 0),
            (Status::Partial, 2),
            (Status::Unsupported, 2),
            (Status::Invalid, 1),
        ] {
            assert_eq!(status.exit_code(), code);
        }
        let mut e = Error::new(
            ErrorKind::Reference,
            "test.partial",
            "relationships",
            "not complete",
        );
        e.result_status = Some(Status::Partial);
        e.diagnostics = vec![(*e.diagnostic).clone(), (*e.diagnostic).clone()];
        assert_eq!(e.status().exit_code(), 2);
        let json: serde_json::Value = serde_json::from_str(&e.to_json()).unwrap();
        assert_eq!(json["status"], "partial");
        assert_eq!(json["issues"].as_array().unwrap().len(), 2);
        assert!(json["report"].is_null());
    }
    #[test]
    fn offsets_identify_their_coordinate_space() {
        for (stage, basis) in [
            ("container", "archive"),
            ("verification", "archive"),
            ("manifest", "member"),
            ("asset_manifest", "member"),
        ] {
            let e = Error::new(ErrorKind::Format, "test.offset", stage, "test").offset(12);
            assert_eq!(e.diagnostic.offset, Some(12));
            assert_eq!(e.diagnostic.offset_basis, Some(basis));
        }
    }
}
