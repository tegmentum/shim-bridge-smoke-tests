//! Per-function bridge cache (design §4 Option B).
//!
//! MVP: we don't yet have per-function codegen wired end-to-end
//! (the prerequisite `sqlink-shim-codegen --function` change is
//! still pending). So the "cache" collapses to a single monolith
//! bridge whose (signature, implementation) key is the pair
//! recorded on the function row.
//!
//! Once codegen lands the flow expands: `ensure` will invoke
//! `sqlink-shim-codegen --function <name>`, run `cargo build
//! --release --target wasm32-wasip2`, `wac plug` against the
//! provider, and write `MANIFEST.json` under a
//! `<sig8>_<impl8>` subdir. The CLI shape doesn't change.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::db::FunctionRow;
use crate::hashing;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub extension: String,
    pub function: String,
    pub signature_hash: String,
    pub implementation_hash: String,
    pub upstream_version: Option<String>,
    pub built_at: String,
    pub bridge_wasm_hash: String,
    pub provider_wasm_hash: String,
}

pub struct Resolved {
    pub bridge_path: PathBuf,
    pub bridge_wasm_hash: String,
    pub provider_path: PathBuf,
    pub provider_wasm_hash: String,
    pub manifest: Manifest,
}

pub fn ensure(
    row: &FunctionRow,
    bridge: &Path,
    provider: &Path,
    cache_root: &Path,
) -> Result<Resolved> {
    if !bridge.exists() {
        bail!("bridge wasm not found: {}", bridge.display());
    }
    if !provider.exists() {
        bail!("provider wasm not found: {}", provider.display());
    }
    let bridge_wasm_hash = hashing::sha256_hex(bridge)?;
    let provider_wasm_hash = hashing::sha256_hex(provider)?;
    let sig8 = row
        .signature_hash
        .as_deref()
        .map(|h| &h[..h.len().min(8)])
        .unwrap_or("nosig");
    let impl8 = row
        .implementation_hash
        .as_deref()
        .map(|h| &h[..h.len().min(8)])
        .unwrap_or("noimpl");
    let dir = cache_root
        .join(&row.extension)
        .join(&row.name)
        .join(format!("{sig8}_{impl8}"));
    std::fs::create_dir_all(&dir).with_context(|| dir.display().to_string())?;
    let manifest = Manifest {
        extension: row.extension.clone(),
        function: row.name.clone(),
        signature_hash: row
            .signature_hash
            .clone()
            .unwrap_or_default(),
        implementation_hash: row
            .implementation_hash
            .clone()
            .unwrap_or_default(),
        upstream_version: row.last_seen_upstream_version.clone(),
        built_at: chrono::Utc::now().to_rfc3339(),
        bridge_wasm_hash: bridge_wasm_hash.clone(),
        provider_wasm_hash: provider_wasm_hash.clone(),
    };
    let manifest_path = dir.join("MANIFEST.json");
    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    // Atomic overwrite through a sibling temp file (design §4
    // atomic-rename move).
    let tmp = dir.join("MANIFEST.json.tmp");
    std::fs::write(&tmp, manifest_json.as_bytes())?;
    std::fs::rename(&tmp, &manifest_path)?;
    Ok(Resolved {
        bridge_path: bridge.to_path_buf(),
        bridge_wasm_hash,
        provider_path: provider.to_path_buf(),
        provider_wasm_hash,
        manifest,
    })
}
