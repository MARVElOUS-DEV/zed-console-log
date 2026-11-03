use std::{fs, path::Path, path::PathBuf};
use zed_extension_api::{Architecture, Os, Result};

pub fn release_target(os: Os, arch: Architecture) -> Result<&'static str> {
    match (os, arch) {
        (Os::Mac, Architecture::Aarch64) => Ok("aarch64-apple-darwin"),
        (Os::Mac, Architecture::X8664) => Ok("x86_64-apple-darwin"),
        (Os::Linux, Architecture::Aarch64) => Ok("aarch64-unknown-linux-musl"),
        (Os::Linux, Architecture::X8664) => Ok("x86_64-unknown-linux-musl"),
        (Os::Windows, Architecture::Aarch64) => Ok("aarch64-pc-windows-msvc"),
        (Os::Windows, Architecture::X8664) => Ok("x86_64-pc-windows-msvc"),
        (_, Architecture::X86) => Err("Automatic downloads require a 64-bit platform".into()),
    }
}

fn valid_binary(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}

/// Only promote a complete installation to the versioned cache. All paths are inside
/// the extension work directory; callbacks use Zed's download and permissions APIs.
pub fn install_binary(
    directory: &Path,
    executable: &str,
    download: impl FnOnce(&Path) -> Result<()>,
    make_executable: impl FnOnce(&Path) -> Result<()>,
) -> Result<PathBuf> {
    let binary = directory.join(executable);
    if valid_binary(&binary) {
        return Ok(binary);
    }

    let staging = directory.with_file_name(format!(
        "{}.download",
        directory.file_name().unwrap().to_string_lossy()
    ));
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|error| format!("Cannot clear interrupted download: {error}"))?;
    }
    fs::create_dir_all(&staging)
        .map_err(|error| format!("Cannot create download directory: {error}"))?;
    download(&staging)?;
    let staged_binary = staging.join(executable);
    if !valid_binary(&staged_binary) {
        return Err(format!(
            "Downloaded archive has no nonempty {executable} executable"
        ));
    }
    make_executable(&staged_binary)?;
    if directory.exists() {
        fs::remove_dir_all(directory)
            .map_err(|error| format!("Cannot remove incomplete installation: {error}"))?;
    }
    fs::rename(&staging, directory)
        .map_err(|error| format!("Cannot finish server installation: {error}"))?;
    Ok(binary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Cache(PathBuf);

    impl Cache {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "console-log-installer-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn directory(&self) -> PathBuf {
            self.0.join("server-v0.1.0-target")
        }
    }

    impl Drop for Cache {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn cached_installation_works_without_network() {
        let cache = Cache::new();
        let directory = cache.directory();
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("server"), b"executable").unwrap();
        let binary = install_binary(
            &directory,
            "server",
            |_| panic!("cached binaries must work offline"),
            |_| panic!("cached binaries are already executable"),
        )
        .unwrap();
        assert_eq!(fs::read(binary).unwrap(), b"executable");
    }

    #[test]
    fn interrupted_download_is_retried_without_caching_partial_binary() {
        let cache = Cache::new();
        let directory = cache.directory();
        assert!(
            install_binary(
                &directory,
                "server",
                |staging| {
                    fs::write(staging.join("server"), b"partial").unwrap();
                    fs::write(staging.join("leftover"), b"old").unwrap();
                    Err("network interrupted".into())
                },
                |_| panic!("failed downloads must not be promoted"),
            )
            .is_err()
        );
        assert!(!directory.exists());
        let binary = install_binary(
            &directory,
            "server",
            |staging| {
                assert!(!staging.join("leftover").exists());
                fs::write(staging.join("server"), b"complete").unwrap();
                Ok(())
            },
            |path| {
                assert_eq!(fs::read(path).unwrap(), b"complete");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(fs::read(binary).unwrap(), b"complete");
    }

    #[test]
    fn malformed_archive_and_permission_failure_are_not_cached() {
        let cache = Cache::new();
        let directory = cache.directory();
        for contents in [None, Some(b"".as_slice()), Some(b"executable".as_slice())] {
            assert!(
                install_binary(
                    &directory,
                    "server.exe",
                    |staging| {
                        if let Some(contents) = contents {
                            fs::write(staging.join("server.exe"), contents).unwrap();
                        }
                        Ok(())
                    },
                    |_| Err("permission denied".into()),
                )
                .is_err()
            );
            assert!(!directory.exists());
        }
    }

    #[test]
    fn different_versions_have_separate_caches_and_windows_executables() {
        let cache = Cache::new();
        let first = cache.directory();
        let second = cache.0.join("server-v0.2.0-target");
        for directory in [&first, &second] {
            let binary = install_binary(
                directory,
                "server.exe",
                |staging| {
                    fs::write(staging.join("server.exe"), b"executable").unwrap();
                    Ok(())
                },
                |_| Ok(()),
            )
            .unwrap();
            assert_eq!(binary, directory.join("server.exe"));
        }
        assert!(first.join("server.exe").is_file());
        assert!(second.join("server.exe").is_file());
    }

    #[test]
    fn unsupported_platform_has_no_download_target() {
        for os in [Os::Mac, Os::Linux, Os::Windows] {
            assert!(release_target(os, Architecture::X86).is_err());
        }
    }
}
