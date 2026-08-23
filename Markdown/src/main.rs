//! Forge Markdown language server proxy.
//!
//! Wraps marksman. A locally installed binary wins; otherwise a pinned release
//! is downloaded (raw single-binary assets, no archive).

use forge_lsp_proxy::{EngineSpec, PathCandidate};

/// Pinned marksman release. Bump together with `version` in
/// `Cargo.toml` and `forge-extension.toml`.
const TAG: &str = "2026-02-08";

fn asset_for(platform: &str) -> Option<&'static str> {
    match platform {
        "windows-x86_64" => Some("marksman.exe"),
        // Single universal macOS asset.
        "macos-x86_64" | "macos-aarch64" => Some("marksman-macos"),
        "linux-x86_64" => Some("marksman-linux-x64"),
        "linux-aarch64" => Some("marksman-linux-arm64"),
        // Upstream ships no windows-aarch64 build; registry-gen omits the platform.
        _ => None,
    }
}

fn main() {
    let spec = EngineSpec {
        lang: "markdown",
        repo: "artempyanykh/marksman",
        tag: TAG,
        platform_asset: asset_for,
        binary_base: "marksman",
        path_candidates: &[PathCandidate { program: "marksman", args: &[] }],
        engine_args: &[],
        cwd_engine_root: false,
        asset_url: None,
    };
    std::process::exit(forge_lsp_proxy::run(&spec));
}
