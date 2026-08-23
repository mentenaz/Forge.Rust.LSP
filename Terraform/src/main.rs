//! Forge Terraform language server proxy.
//!
//! Wraps terraform-ls. HashiCorp distributes release archives from
//! releases.hashicorp.com rather than GitHub assets, so this spec uses the
//! direct-URL resolver; a locally installed terraform-ls still wins first.

use forge_lsp_proxy::{EngineSpec, PathCandidate};

/// Pinned terraform-ls release (releases.hashicorp.com version, no `v` prefix).
const TAG: &str = "0.39.0";

fn os_arch(platform: &str) -> Option<&'static str> {
    match platform {
        "windows-x86_64" => Some("windows_amd64"),
        "windows-aarch64" => Some("windows_arm64"),
        "macos-x86_64" => Some("darwin_amd64"),
        "macos-aarch64" => Some("darwin_arm64"),
        "linux-x86_64" => Some("linux_amd64"),
        "linux-aarch64" => Some("linux_arm64"),
        _ => None,
    }
}

fn asset_for(platform: &str) -> Option<&'static str> {
    // Only used for log messages and the raw-file fallback name; the real
    // download URL comes from `asset_url` below.
    os_arch(platform).map(|_| "terraform-ls.zip")
}

fn url_for(platform: &str, tag: &str) -> Option<String> {
    Some(format!(
        "https://releases.hashicorp.com/terraform-ls/{tag}/terraform-ls_{tag}_{}.zip",
        os_arch(platform)?
    ))
}

fn main() {
    let spec = EngineSpec {
        lang: "terraform",
        repo: "hashicorp/terraform-ls",
        tag: TAG,
        platform_asset: asset_for,
        binary_base: "terraform-ls",
        path_candidates: &[PathCandidate { program: "terraform-ls", args: &["serve"] }],
        engine_args: &["serve"],
        cwd_engine_root: false,
        asset_url: Some(url_for),
    };
    std::process::exit(forge_lsp_proxy::run(&spec));
}
