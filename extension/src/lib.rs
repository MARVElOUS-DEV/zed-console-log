use zed_extension_api::{self as zed, settings::LspSettings};

struct ConsoleLogExtension;

impl zed::Extension for ConsoleLogExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let settings = LspSettings::for_worktree(language_server_id.as_ref(), worktree)?;
        let binary = settings.binary;
        let command = binary
            .as_ref()
            .and_then(|b| b.path.clone())
            .or_else(|| worktree.which("zed-console-log"))
            .ok_or_else(|| {
                "Build zed-console-log and set lsp.zed-console-log.binary.path to its absolute path"
                    .to_string()
            })?;
        let args = binary
            .as_ref()
            .and_then(|b| b.arguments.clone())
            .unwrap_or_else(|| vec!["--stdio".into()]);
        let env = binary
            .and_then(|b| b.env)
            .unwrap_or_default()
            .into_iter()
            .collect();
        Ok(zed::Command { command, args, env })
    }

    fn language_server_initialization_options(
        &mut self,
        id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<Option<zed::serde_json::Value>> {
        let settings = LspSettings::for_worktree(id.as_ref(), worktree)?;
        Ok(settings.initialization_options.or(settings.settings))
    }

    fn language_server_workspace_configuration(
        &mut self,
        id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<Option<zed::serde_json::Value>> {
        Ok(LspSettings::for_worktree(id.as_ref(), worktree)?.settings)
    }
}

zed::register_extension!(ConsoleLogExtension);
