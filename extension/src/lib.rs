mod install;

use std::path::Path;
use zed_extension_api::{self as zed, settings::LspSettings};

const REPOSITORY: &str = "MARVElOUS-DEV/zed-console-log";

fn downloaded_binary(id: &zed::LanguageServerId) -> zed::Result<String> {
    let result = (|| {
        let (os, arch) = zed::current_platform();
        let target = install::release_target(os, arch)?;
        let tag = concat!("v", env!("CARGO_PKG_VERSION"));
        let directory = format!("zed-console-log-{tag}-{target}");
        let executable = if os == zed::Os::Windows {
            "zed-console-log.exe"
        } else {
            "zed-console-log"
        };
        install::install_binary(
            Path::new(&directory),
            executable,
            |staging| {
                zed::set_language_server_installation_status(
                    id,
                    &zed::LanguageServerInstallationStatus::CheckingForUpdate,
                );
                let release = zed::github_release_by_tag_name(REPOSITORY, tag)?;
                let archive = if os == zed::Os::Windows {
                    "zip"
                } else {
                    "tar.gz"
                };
                let asset_name = format!("zed-console-log-{tag}-{target}.{archive}");
                let asset = release
                    .assets
                    .iter()
                    .find(|asset| asset.name == asset_name)
                    .ok_or_else(|| format!("Release {tag} has no asset {asset_name}"))?;
                zed::set_language_server_installation_status(
                    id,
                    &zed::LanguageServerInstallationStatus::Downloading,
                );
                zed::download_file(
                    &asset.download_url,
                    &staging.to_string_lossy(),
                    if os == zed::Os::Windows {
                        zed::DownloadedFileType::Zip
                    } else {
                        zed::DownloadedFileType::GzipTar
                    },
                )
            },
            |binary| zed::make_file_executable(&binary.to_string_lossy()),
        )
        .map(|path| path.to_string_lossy().into_owned())
    })();
    match &result {
        Ok(_) => zed::set_language_server_installation_status(
            id,
            &zed::LanguageServerInstallationStatus::None,
        ),
        Err(error) => zed::set_language_server_installation_status(
            id,
            &zed::LanguageServerInstallationStatus::Failed(error.clone()),
        ),
    }
    result.map_err(|error| {
        format!(
            "Could not install zed-console-log: {error}. You can install the server yourself and set \
             lsp.zed-console-log.binary.path to its absolute path. Releases: \
             https://github.com/{REPOSITORY}/releases"
        )
    })
}

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
        let command = match binary
            .as_ref()
            .and_then(|b| b.path.clone())
            .or_else(|| worktree.which("zed-console-log"))
        {
            Some(path) => path,
            None => downloaded_binary(language_server_id)?,
        };
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
