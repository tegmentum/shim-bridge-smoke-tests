//! sha256 hashing of on-disk wasm artifacts for stamping into
//! `test_runs` (provider + bridge trio traceability, design §5, §8).

use std::path::Path;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// sha256 of the file at `path`, hex-encoded (lowercase).
pub fn sha256_hex(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("read {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}
