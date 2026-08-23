//! Forge Lua language server proxy.
//!
//! Wraps the sumneko lua-language-server. A locally installed binary wins;
//! otherwise a pinned release is downloaded. The engine needs its sibling
//! `main.lua`/`meta/` resources, so it runs from the extraction root.

use forge_lsp_proxy::{EngineSpec, PathCandidate};

/// Pinned lua-language-server release. Bump together with `version` in
/// `Cargo.toml` and `forge-extension.toml` (the asset names embed the version).
const TAG: &str = "3.19.1";

fn asset_for(platform: &str) -> Option<&'static str> {
    match platform {
        "windows-x86_64" => Some("lua-language-server-3.19.1-win32-x64.zip"),
        "macos-x86_64" => Some("lua-language-server-3.19.1-darwin-x64.tar.gz"),
        "macos-aarch64" => Some("lua-language-server-3.19.1-darwin-arm64.tar.gz"),
        "linux-x86_64" => Some("lua-language-server-3.19.1-linux-x64.tar.gz"),
        "linux-aarch64" => Some("lua-language-server-3.19.1-linux-arm64.tar.gz"),
        // Upstream ships no windows-aarch64 build; registry-gen omits the platform.
        _ => None,
    }
}

fn main() {
    let spec = EngineSpec {
        lang: "lua",
        repo: "sumneko/lua-language-server",
        tag: TAG,
        platform_asset: asset_for,
        binary_base: "lua-language-server",
        path_candidates: &[PathCandidate { program: "lua-language-server", args: &[] }],
        engine_args: &[],
        cwd_engine_root: true,
        asset_url: None,
    };
    std::process::exit(forge_lsp_proxy::run(&spec));
}
