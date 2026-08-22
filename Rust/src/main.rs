//! Forge Rust language server proxy.
//!
//! Wraps rust-analyzer. A locally installed rust-analyzer (rustup component)
//! always wins — it is version-matched to the project's toolchain; otherwise a
//! pinned upstream release is downloaded into the per-user engine cache.

use forge_lsp_proxy::{EngineSpec, PathCandidate};

/// Pinned rust-analyzer release. Bump together with `version` in
/// `Cargo.toml` and `forge-extension.toml`.
const TAG: &str = "2026-08-17.4";

fn asset_for(platform: &str) -> Option<&'static str> {
    match platform {
        "windows-x86_64" => Some("rust-analyzer-x86_64-pc-windows-msvc.zip"),
        "windows-aarch64" => Some("rust-analyzer-aarch64-pc-windows-msvc.zip"),
        "macos-x86_64" => Some("rust-analyzer-x86_64-apple-darwin.gz"),
        "macos-aarch64" => Some("rust-analyzer-aarch64-apple-darwin.gz"),
        "linux-x86_64" => Some("rust-analyzer-x86_64-unknown-linux-gnu.gz"),
        "linux-aarch64" => Some("rust-analyzer-aarch64-unknown-linux-gnu.gz"),
        _ => None,
    }
}

fn main() {
    let spec = EngineSpec {
        lang: "rust",
        repo: "rust-lang/rust-analyzer",
        tag: TAG,
        platform_asset: asset_for,
        binary_base: "rust-analyzer",
        path_candidates: &[PathCandidate { program: "rust-analyzer", args: &[] }],
        engine_args: &[],
        cwd_engine_root: false,
    };
    std::process::exit(forge_lsp_proxy::run(&spec));
}
