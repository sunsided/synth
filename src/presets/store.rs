//! User-preset persistence: file path resolution, JSON serialisation, and loading.

use crate::params::Patch;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Return the path used to persist user presets.
/// Stored next to the executable (or current directory as fallback).
pub fn user_presets_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("synth_presets.json");
        }
    }
    PathBuf::from("synth_presets.json")
}

/// Persist a list of patches to disk.
pub fn save_patches(patches: &[Patch], path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(patches).context("serialise patches")?;
    std::fs::write(path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Load patches from disk. Returns an empty Vec if the file does not exist.
#[allow(dead_code)]
pub fn load_patches(path: &Path) -> Result<Vec<Patch>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let json = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let patches: Vec<Patch> = serde_json::from_str(&json).context("deserialise patches")?;
    Ok(patches)
}
