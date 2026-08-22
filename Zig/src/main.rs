//! Forge Zig language server proxy.
//!
//! Wraps zls. A locally installed zls wins; otherwise a pinned release is
//! downloaded (zip on Windows, tar.xz via system `tar` elsewhere). zls needs a
//! matching Zig toolchain on PATH to be useful — warn when it is missing.

use forge_lsp_proxy::{EngineSpec, PathCandidate};

/// Pinned zls release. Bump together with `version` in
/// `Cargo.toml` and `forge-extension.toml`, and keep it matched to the
/// supported Zig version.
const TAG: &str = "0.16.0";

fn asset_for(platform: &str) -> Option<&'static str> {
    match platform {
        "windows-x86_64" => Some("zls-x86_64-windows.zip"),
        "windows-aarch64" => Some("zls-aarch64-windows.zip"),
        "macos-x86_64" => Some("zls-x86_64-macos.tar.xz"),
        "macos-aarch64" => Some("zls-aarch64-macos.tar.xz"),
        "linux-x86_64" => Some("zls-x86_64-linux.tar.xz"),
        "linux-aarch64" => Some("zls-aarch64-linux.tar.xz"),
        _ => None,
    }
}

fn main() {
    if forge_lsp_proxy::which("zig").is_none() {
        eprintln!("forge-lsp-zig: warning: no zig on PATH; zls will have limited functionality");
    }

    let spec = EngineSpec {
        lang: "zig",
        repo: "zigtools/zls",
        tag: TAG,
        platform_asset: asset_for,
        binary_base: "zls",
        path_candidates: &[PathCandidate { program: "zls", args: &[] }],
        engine_args: &[],
        cwd_engine_root: false,
    };
    std::process::exit(forge_lsp_proxy::run(&spec));
}
