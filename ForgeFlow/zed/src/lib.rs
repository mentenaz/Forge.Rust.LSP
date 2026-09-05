use zed_extension_api as zed;

struct ForgeFlowExtension;

impl zed::Extension for ForgeFlowExtension {
    fn new() -> Self {
        ForgeFlowExtension
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let binary = worktree.which("forge-lsp-forgeflow").ok_or_else(|| {
            "forge-lsp-forgeflow not found on PATH. Build it with \
             `cargo build --release -p forge-lsp-forgeflow` and add the \
             resulting binary to your PATH."
                .to_string()
        })?;
        Ok(zed::Command {
            command: binary,
            args: vec![],
            env: vec![],
        })
    }
}

zed::register_extension!(ForgeFlowExtension);
