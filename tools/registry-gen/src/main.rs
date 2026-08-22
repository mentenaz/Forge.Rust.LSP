//! Generates `forge-registry.json` — the canonical index Forge consumes to
//! discover and download language server packages from this repo's releases.
//!
//! It scans every sibling directory of this tool that carries a
//! `forge-extension.toml`, reads the package metadata, and emits one registry
//! entry per package with per-platform release assets. Run it from the repo
//! root (or set the working dir) so it can find the package folders.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Every platform we publish server binaries for. Keys are Forge platform ids;
/// the values drive asset naming.
const PLATFORMS: &[&str] = &[
    "windows-x86_64",
    "windows-aarch64",
    "macos-x86_64",
    "macos-aarch64",
    "linux-x86_64",
    "linux-aarch64",
];

#[derive(Deserialize)]
struct PackageToml {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    version: String,
    language: PackageLanguage,
    language_server: PackageServer,
}

#[derive(Deserialize)]
struct PackageLanguage {
    language_id: String,
    #[serde(default)]
    file_extensions: Vec<String>,
}

#[derive(Deserialize)]
struct PackageServer {
    binary: String,
    #[serde(default)]
    args: Vec<String>,
    /// Platforms this package actually supports. Empty means all of
    /// [`PLATFORMS`]. Packages whose upstream engine has no prebuilt for a
    /// platform should omit it so the registry stays truthful.
    #[serde(default)]
    platforms: Vec<String>,
}

#[derive(Serialize)]
struct RegistryEntry {
    name: String,
    description: String,
    version: String,
    repo: String,
    language_id: String,
    file_extensions: Vec<String>,
    args: Vec<String>,
    assets: BTreeMap<String, PlatformAsset>,
}

#[derive(Serialize)]
struct PlatformAsset {
    archive: String,
    binary: String,
}

fn main() {
    let root = env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .expect("CARGO_MANIFEST_DIR")
        .parent() // tools/
        .expect("parent")
        .parent() // repo root
        .expect("repo root")
        .to_path_buf();

    let repo = env::var("FORGE_LSP_REPO").unwrap_or_else(|_| "mentenaz/Forge.Rust.LSP".into());

    let mut registry: BTreeMap<String, RegistryEntry> = BTreeMap::new();
    for pkg_dir in package_dirs(&root) {
        let toml_path = pkg_dir.join("forge-extension.toml");
        if !toml_path.exists() {
            continue;
        }
        let raw = fs::read_to_string(&toml_path).expect("read forge-extension.toml");
        let pkg: PackageToml = toml::from_str(&raw).expect("parse forge-extension.toml");

        let mut assets = BTreeMap::new();
        for platform in PLATFORMS {
            if !pkg.language_server.platforms.is_empty()
                && !pkg.language_server.platforms.iter().any(|p| p == platform)
            {
                continue;
            }
            let exe_suffix = if platform.starts_with("windows-") { ".exe" } else { "" };
            assets.insert(
                platform.to_string(),
                PlatformAsset {
                    archive: format!("{}-{platform}.zip", pkg.language_server.binary),
                    binary: format!("{}{exe_suffix}", pkg.language_server.binary),
                },
            );
        }

        registry.insert(
            pkg.id.clone(),
            RegistryEntry {
                name: pkg.name,
                description: pkg.description,
                version: pkg.version,
                repo: repo.clone(),
                language_id: pkg.language.language_id,
                file_extensions: pkg.language.file_extensions,
                args: pkg.language_server.args,
                assets,
            },
        );
    }

    let out = root.join("forge-registry.json");
    let json = serde_json::to_string_pretty(&registry).expect("serialize registry");
    fs::write(&out, json).expect("write forge-registry.json");
    println!("wrote {} packages -> {}", registry.len(), out.display());
}

/// Direct child directories of `root` that contain a package manifest.
fn package_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(root).expect("read repo root") {
        let entry = entry.expect("dir entry");
        if entry.path().is_dir()
            && entry.path().join("forge-extension.toml").exists()
            && !is_hidden(&entry.file_name())
        {
            dirs.push(entry.path());
        }
    }
    dirs
}

fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}