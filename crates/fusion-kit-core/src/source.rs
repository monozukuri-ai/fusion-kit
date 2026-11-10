use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::{Error, ErrorKind, Limits};

/// An owned, immutable input. Its hash always describes the bytes parsed.
#[derive(Clone)]
pub struct InputSnapshot {
    pub(crate) data: Arc<[u8]>,
    pub(crate) name: String,
    pub(crate) sha256: String,
}

impl std::fmt::Debug for InputSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputSnapshot")
            .field("name", &self.name)
            .field("bytes", &self.data.len())
            .field("sha256", &self.sha256)
            .finish()
    }
}

impl InputSnapshot {
    pub fn from_bytes(data: &[u8], limits: Limits) -> Result<Self, Error> {
        check_size(data.len() as u64, limits)?;
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(data.len())
            .map_err(|_| allocation_error())?;
        owned.extend_from_slice(data);
        Ok(Self::owned(owned, "<bytes>".to_owned()))
    }

    /// Adopt an already owned Python-independent buffer, checking its size before hashing.
    pub fn from_vec(data: Vec<u8>, limits: Limits) -> Result<Self, Error> {
        check_size(data.len() as u64, limits)?;
        Ok(Self::owned(data, "<bytes>".to_owned()))
    }

    pub fn open(path: impl AsRef<Path>, limits: Limits) -> Result<Self, Error> {
        let path = path.as_ref();
        let name = path.to_string_lossy().into_owned();
        let result = (|| {
            let mut file = File::open(path).map_err(|error| Error::io(error, &name))?;
            let metadata = file.metadata().map_err(|error| Error::io(error, &name))?;
            if !metadata.is_file() {
                return Err(Error::io(
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "input must be a regular file",
                    ),
                    &name,
                ));
            }
            check_size(metadata.len(), limits)?;
            let mut data = Vec::new();
            let mut chunk = [0; 65536];
            loop {
                let remaining = limits.max_input_bytes().saturating_sub(data.len() as u64);
                let request = remaining.saturating_add(1).min(chunk.len() as u64) as usize;
                let count = file
                    .read(&mut chunk[..request])
                    .map_err(|error| Error::io(error, &name))?;
                if count == 0 {
                    break;
                }
                check_size(data.len() as u64 + count as u64, limits)?;
                data.try_reserve_exact(count)
                    .map_err(|_| allocation_error())?;
                data.extend_from_slice(&chunk[..count]);
            }
            Ok(Self::owned(data, name.clone()))
        })();
        result.map_err(|error: Error| error.source_name(&name))
    }

    fn owned(data: Vec<u8>, name: String) -> Self {
        let sha256 = Sha256::digest(&data)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Self {
            data: data.into(),
            name,
            sha256,
        }
    }
}

fn check_size(size: u64, limits: Limits) -> Result<(), Error> {
    if size > limits.max_input_bytes() {
        return Err(Error::new(
            ErrorKind::ResourceLimit,
            "limit.input_bytes",
            "source",
            "input exceeds max_input_bytes",
        ));
    }
    Ok(())
}

pub(crate) fn allocation_error() -> Error {
    Error::new(
        ErrorKind::ResourceLimit,
        "limit.allocation",
        "source",
        "unable to allocate a bounded input/output buffer",
    )
}
