//! User-preset persistence: JSON next to the executable in the terminal
//! build, `localStorage` in the browser build.
//!
//! This module is binary-local; `synth::presets` (the library) only contains
//! the built-in SID patch bank and has no filesystem dependencies.

use anyhow::{Context, Result};
use synthie::params::Patch;

/// Persist the user's patches (best effort).
pub fn save(patches: &[Patch]) -> Result<()> {
    let json = serde_json::to_string_pretty(patches).context("serialise patches")?;
    write(&json)
}

#[cfg(not(target_arch = "wasm32"))]
fn write(json: &str) -> Result<()> {
    let path = native::user_presets_path();
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))
}

#[cfg(target_arch = "wasm32")]
fn write(json: &str) -> Result<()> {
    let storage = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .context("localStorage unavailable")?;
    storage
        .set_item(web::STORAGE_KEY, json)
        .map_err(|_| anyhow::anyhow!("localStorage is full or blocked"))
}

#[cfg(target_arch = "wasm32")]
mod web {
    /// `localStorage` key holding the user presets as JSON.
    pub const STORAGE_KEY: &str = "synthie.user-presets";
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result};
    use synthie::params::Patch;

    /// Return the path used to persist user presets.
    /// Stored next to the executable (or current directory as fallback).
    pub fn user_presets_path() -> PathBuf {
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            return dir.join("synth_presets.json");
        }
        PathBuf::from("synth_presets.json")
    }

    /// Load patches from disk. Returns an empty Vec if the file does not exist.
    // TODO: call this on startup to merge saved user presets with the built-in bank.
    #[allow(dead_code)]
    pub fn load_patches(path: &Path) -> Result<Vec<Patch>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let json =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let patches: Vec<Patch> = serde_json::from_str(&json).context("deserialise patches")?;
        Ok(patches)
    }
}
