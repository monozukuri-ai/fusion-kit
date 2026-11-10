use std::error::Error;
use std::fmt;

/// Resource policy for one archive-reading or verification operation.
///
/// Constructing this policy validates its configuration; it does not read an
/// archive or reserve the configured amounts of memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    max_input_bytes: u64,
    max_member_bytes: u64,
    max_total_uncompressed_bytes: u64,
    max_entries: u64,
    max_nesting_depth: u64,
}

impl Limits {
    pub(crate) fn remaining_entries(self, used: u64) -> Result<Self, crate::Error> {
        let remaining = self.max_entries.saturating_sub(used);
        if remaining == 0 {
            return Err(crate::Error::new(
                crate::ErrorKind::ResourceLimit,
                "limit.entries",
                "container",
                "cumulative member count exhausted before child index",
            ));
        }
        Ok(Self {
            max_entries: remaining,
            ..self
        })
    }
    /// Construct a policy with positive limits and a member cap no larger than
    /// the cumulative uncompressed-byte cap.
    pub fn new(
        max_input_bytes: u64,
        max_member_bytes: u64,
        max_total_uncompressed_bytes: u64,
        max_entries: u64,
        max_nesting_depth: u64,
    ) -> Result<Self, InvalidLimits> {
        for (field, value) in [
            ("max_input_bytes", max_input_bytes),
            ("max_member_bytes", max_member_bytes),
            ("max_total_uncompressed_bytes", max_total_uncompressed_bytes),
            ("max_entries", max_entries),
            ("max_nesting_depth", max_nesting_depth),
        ] {
            if value == 0 {
                return Err(InvalidLimits::Zero { field });
            }
        }
        if max_member_bytes > max_total_uncompressed_bytes {
            return Err(InvalidLimits::MemberExceedsTotal);
        }
        Ok(Self {
            max_input_bytes,
            max_member_bytes,
            max_total_uncompressed_bytes,
            max_entries,
            max_nesting_depth,
        })
    }

    pub const fn max_input_bytes(&self) -> u64 {
        self.max_input_bytes
    }

    pub const fn max_member_bytes(&self) -> u64 {
        self.max_member_bytes
    }

    pub const fn max_total_uncompressed_bytes(&self) -> u64 {
        self.max_total_uncompressed_bytes
    }

    pub const fn max_entries(&self) -> u64 {
        self.max_entries
    }

    /// Maximum archive depth; the outer archive is depth one.
    pub const fn max_nesting_depth(&self) -> u64 {
        self.max_nesting_depth
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_bytes: 512 * 1024 * 1024,
            max_member_bytes: 256 * 1024 * 1024,
            max_total_uncompressed_bytes: 1024 * 1024 * 1024,
            max_entries: 100_000,
            max_nesting_depth: 2,
        }
    }
}

/// A configuration error, distinct from a resource limit exceeded by a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidLimits {
    Zero { field: &'static str },
    MemberExceedsTotal,
}

impl fmt::Display for InvalidLimits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero { field } => write!(f, "{field} must be greater than zero"),
            Self::MemberExceedsTotal => write!(
                f,
                "max_member_bytes must not exceed max_total_uncompressed_bytes"
            ),
        }
    }
}

impl Error for InvalidLimits {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_zero_limit_is_rejected_with_its_field() {
        let fields = [
            "max_input_bytes",
            "max_member_bytes",
            "max_total_uncompressed_bytes",
            "max_entries",
            "max_nesting_depth",
        ];
        for (index, field) in fields.into_iter().enumerate() {
            let mut values = [1; 5];
            values[index] = 0;
            assert_eq!(
                Limits::new(values[0], values[1], values[2], values[3], values[4]),
                Err(InvalidLimits::Zero { field })
            );
        }
    }

    #[test]
    fn member_budget_cannot_exceed_total_but_equal_is_valid() {
        assert_eq!(
            Limits::new(1, 2, 1, 1, 1),
            Err(InvalidLimits::MemberExceedsTotal)
        );
        assert!(Limits::new(1, 2, 2, 1, 1).is_ok());
    }

    #[test]
    fn compressed_input_cap_does_not_cap_uncompressed_member_size() {
        let limits = Limits::new(1, 1_u64 << 40, 1_u64 << 41, 5, 2).unwrap();
        assert_eq!(limits.max_input_bytes(), 1);
        assert_eq!(limits.max_member_bytes(), 1_u64 << 40);
        assert_eq!(limits.max_total_uncompressed_bytes(), 1_u64 << 41);
    }

    #[test]
    fn documented_default_policy_is_valid() {
        let limits = Limits::default();
        assert_eq!(limits.max_input_bytes(), 536_870_912);
        assert_eq!(limits.max_member_bytes(), 268_435_456);
        assert_eq!(limits.max_total_uncompressed_bytes(), 1_073_741_824);
        assert_eq!(limits.max_entries(), 100_000);
        assert_eq!(limits.max_nesting_depth(), 2);
        assert_eq!(
            Limits::new(
                limits.max_input_bytes(),
                limits.max_member_bytes(),
                limits.max_total_uncompressed_bytes(),
                limits.max_entries(),
                limits.max_nesting_depth(),
            ),
            Ok(limits)
        );
    }
}
