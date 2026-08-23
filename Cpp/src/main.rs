//! Forge C/C++ language server proxy.
//!
//! Wraps clangd. A locally installed clangd wins; otherwise a pinned release
//! from the official `clangd/clangd` repo is downloaded and extracted.

use forge_lsp_proxy::{EngineSpec, PathCandidate};

/// Pinned clangd release. Bump together with `version` in
/// `Cargo.toml` and `forge-extension.toml`.
const TAG: &str = "22.1.6";

fn asset_for(platform: &str) -> Option<&'static str> {
    match platform {
        "windows-x86_64" => Some("clangd-windows-22.1.6.zip"),
        "macos-x86_64" | "macos-aarch64" => Some("clangd-mac-22.1.6.zip"),
        "linux-x86_64" => Some("clangd-linux-22.1.6.zip"),
        // Upstream publishes no ARM builds; registry-gen omits these platforms.
        _ => None,
    }
}

fn main() {
    let spec = EngineSpec {
        lang: "cpp",
        repo: "clangd/clangd",
        tag: TAG,
        platform_asset: asset_for,
        binary_base: "clangd",
        path_candidates: &[PathCandidate { program: "clangd", args: &[] }],
        engine_args: &["--background-index"],
        cwd_engine_root: true,
        asset_url: None,
    };
    std::process::exit(forge_lsp_proxy::run(&spec));
}
