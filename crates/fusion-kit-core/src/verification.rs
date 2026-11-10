//! Iterative recursive ZIP verification with one shared byte/member/depth budget.
use crate::{Archive, Error, ErrorKind, InputSnapshot, ReadOptions, Status, ValidationReport};

impl Archive {
    pub(crate) fn verify_recursive(&self, strict: bool) -> Result<ValidationReport, Error> {
        let mut report = ValidationReport {
            schema_version: 1,
            source_sha256: self.source_sha256().into(),
            status: Status::Complete,
            scope: "package_archive_members",
            members_total: self.members().len(),
            members_verified: 0,
            bytes_decompressed: 0,
            nested_archives: "complete",
            issues: Vec::new(),
        };
        // None denotes the borrowed root; child snapshots are owned until visited.
        let mut pending = vec![(None, 1_u64)];
        let mut first = None;
        while let Some((owned, depth)) = pending.pop() {
            let archive = owned.as_ref().unwrap_or(self);
            for member in archive.members() {
                let lower = member.name.to_ascii_lowercase();
                let nested =
                    !member.is_directory && (lower.ends_with(".f3d") || lower.ends_with(".f3z"));
                let result = (|| {
                    if nested && depth >= self.limits.max_nesting_depth() {
                        return Err(Error::new(
                            ErrorKind::ResourceLimit,
                            "limit.nesting_depth",
                            "verification",
                            "nested archive exceeds max_nesting_depth",
                        )
                        .source_name(archive.source_name())
                        .member(&member.name));
                    }
                    let bytes = archive.decode(member, &mut report.bytes_decompressed, nested)?;
                    report.members_verified += 1;
                    if nested {
                        let mut snapshot = InputSnapshot::from_vec(bytes, self.limits)?;
                        snapshot.name = format!("{}!{}", archive.source_name(), member.name);
                        let child = Archive::from_snapshot(
                            snapshot,
                            ReadOptions {
                                limits: self
                                    .limits
                                    .remaining_entries(report.members_total as u64)?,
                                strict: false,
                            },
                        )?;
                        report.members_total = report
                            .members_total
                            .checked_add(child.members().len())
                            .ok_or_else(|| {
                                Error::new(
                                    ErrorKind::ResourceLimit,
                                    "limit.entries",
                                    "verification",
                                    "entry count overflow",
                                )
                            })?;
                        if report.members_total as u64 > self.limits.max_entries() {
                            return Err(Error::new(
                                ErrorKind::ResourceLimit,
                                "limit.entries",
                                "verification",
                                "cumulative nested member count exceeds max_entries",
                            )
                            .source_name(child.source_name()));
                        }
                        pending.push((Some(child), depth + 1));
                    }
                    Ok(())
                })();
                if let Err(mut error) = result {
                    if error.diagnostic.source.is_none() {
                        error = error
                            .source_name(archive.source_name())
                            .member(&member.name);
                    }
                    report.issues.push((*error.diagnostic).clone());
                    if error.kind == ErrorKind::ResourceLimit {
                        report.status = Status::Invalid;
                        report.nested_archives = "partial";
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
        if report.status != Status::Complete {
            report.nested_archives = "partial";
        }
        if strict && let Some(mut error) = first {
            error.report = Some(Box::new(report));
            return Err(error);
        }
        Ok(report)
    }
}
