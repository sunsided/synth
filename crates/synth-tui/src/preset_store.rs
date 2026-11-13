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

/// The patches saved by [`save`]; empty if nothing was saved yet.
///
/// # Errors
///
/// Fails if the saved patches exist but cannot be read or parsed.
pub fn load() -> Result<Vec<Patch>> {
    match read()? {
        Some(json) => serde_json::from_str(&json).with_context(|| format!("parse {}", location())),
        None => Ok(Vec::new()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write(json: &str) -> Result<()> {
    native::write(&native::user_presets_path(), json)
}

#[cfg(not(target_arch = "wasm32"))]
fn read() -> Result<Option<String>> {
    native::read(&native::user_presets_path())
}

/// Where the presets live, for error messages.
#[cfg(not(target_arch = "wasm32"))]
fn location() -> String {
    native::user_presets_path().display().to_string()
}

#[cfg(target_arch = "wasm32")]
fn write(json: &str) -> Result<()> {
    web::storage()?
        .set_item(web::STORAGE_KEY, json)
        .map_err(|_| anyhow::anyhow!("localStorage is full or blocked"))
}

#[cfg(target_arch = "wasm32")]
fn read() -> Result<Option<String>> {
    web::storage()?
        .get_item(web::STORAGE_KEY)
        .map_err(|_| anyhow::anyhow!("localStorage is blocked"))
}

/// Where the presets live, for error messages.
#[cfg(target_arch = "wasm32")]
fn location() -> String {
    format!("localStorage[{:?}]", web::STORAGE_KEY)
}

#[cfg(target_arch = "wasm32")]
mod web {
    use anyhow::{Context, Result};

    /// `localStorage` key holding the user presets as JSON.
    pub const STORAGE_KEY: &str = "synthie.user-presets";

    pub fn storage() -> Result<web_sys::Storage> {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .context("localStorage unavailable")
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::io::Write;
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result};

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

    /// Replace the file in one step: write a temporary file next to it and
    /// rename it over the old one, so an interrupted save leaves the previous
    /// presets intact instead of a truncated file.
    pub fn write(path: &Path, json: &str) -> Result<()> {
        let tmp = path.with_extension("json.tmp");
        (|| {
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(json.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&tmp, path)
        })()
        .with_context(|| format!("write {}", path.display()))
    }

    /// The file's contents, or `None` if it does not exist.
    pub fn read(path: &Path) -> Result<Option<String>> {
        match std::fs::read_to_string(path) {
            Ok(json) => Ok(Some(json)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use synthie::params::Patch;
        use synthie::presets::sid::default_patches;

        #[test]
        fn saved_patches_read_back_unchanged() {
            let path = std::env::temp_dir()
                .join(format!("synthie-presets-test-{}.json", std::process::id()));
            assert_eq!(read(&path).unwrap(), None, "a missing file is no error");

            let patches = vec![Patch::new("Mine", default_patches()[2].params.clone())];
            write(&path, &serde_json::to_string(&patches).unwrap()).unwrap();
            let json = read(&path).unwrap().unwrap();
            std::fs::remove_file(&path).unwrap();
            let loaded: Vec<Patch> = serde_json::from_str(&json).unwrap();
            assert_eq!(loaded, patches);

            // A second save replaces the first and leaves no temporary file.
            write(&path, "[]").unwrap();
            assert_eq!(read(&path).unwrap().as_deref(), Some("[]"));
            assert!(!path.with_extension("json.tmp").exists());
            std::fs::remove_file(&path).unwrap();
        }
    }
}
