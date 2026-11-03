"""Build and verify the native archives consumed by the Zed launcher (Python 3.11+)."""

import argparse
import hashlib
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parent.parent
TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-musl",
    "aarch64-pc-windows-msvc",
    "x86_64-pc-windows-msvc",
)
DOCUMENTS = ("README.md", "LICENSE", "CHANGELOG.md")


def version(root=ROOT):
    return tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]


def validate(root=ROOT, ref=None):
    manifest = tomllib.loads((root / "extension/extension.toml").read_text())
    expected_version = version(root)
    if not re.fullmatch(r"\d+\.\d+\.\d+", expected_version):
        raise ValueError("Release version must be a stable major.minor.patch version")
    if manifest["version"] != expected_version:
        raise ValueError("Workspace and extension versions must match")
    if manifest["id"] != "debug-log-lsp":
        raise ValueError("Registry ID must be debug-log-lsp")
    if manifest.get("repository") != "https://github.com/MARVElOUS-DEV/zed-console-log":
        raise ValueError("Manifest must identify the public release repository")
    if not manifest.get("authors"):
        raise ValueError("Manifest must identify its authors")
    if (root / "extension/LICENSE").read_bytes() != (root / "LICENSE").read_bytes():
        raise ValueError("extension/LICENSE must match the root MIT license")
    ref = os.environ.get("GITHUB_REF", "") if ref is None else ref
    if ref.startswith("refs/tags/") and ref != f"refs/tags/v{expected_version}":
        raise ValueError(f"Release tag must be v{expected_version}, got {ref}")
    installer = (root / "extension/src/install.rs").read_text()
    mapped = set(re.findall(r'Ok\("([a-z0-9_]+-(?:apple|unknown|pc)-[a-z0-9-]+)"\)', installer))
    workflow = (root / ".github/workflows/release.yml").read_text()
    built = set(re.findall(r"target: ([a-z0-9_-]+)", workflow))
    if mapped != set(TARGETS) or built != set(TARGETS):
        raise ValueError("Launcher targets and release workflow must match the packaged targets")
    print(f"Validated debug-log-lsp v{expected_version} ({len(TARGETS)} native targets)")


def executable(target):
    return "zed-console-log.exe" if "windows" in target else "zed-console-log"


def asset_name(target, release_version):
    extension = "zip" if "windows" in target else "tar.gz"
    return f"zed-console-log-v{release_version}-{target}.{extension}"


def checksum(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def package(target, root=ROOT, output=None):
    if target not in TARGETS:
        raise ValueError(f"Unsupported target: {target}")
    release_version = version(root)
    binary = root / "target" / target / "release" / executable(target)
    reported = subprocess.check_output([str(binary), "--version"], text=True).strip()
    if reported != f"zed-console-log {release_version}":
        raise ValueError(f"Built binary has the wrong version: {reported}")
    output = output or root / "dist"
    output.mkdir(parents=True, exist_ok=True)
    archive = output / asset_name(target, release_version)
    files = [binary, *(root / name for name in DOCUMENTS)]
    if "windows" in target:
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
            for path in files:
                bundle.write(path, arcname=path.name)
    else:
        with tarfile.open(archive, "w:gz") as bundle:
            for path in files:
                bundle.add(path, arcname=path.name)
    (output / f"{archive.name}.sha256").write_text(
        f"{checksum(archive)}  {archive.name}\n", encoding="utf-8"
    )
    print(f"Packaged {archive.name}")


def verify(output, release_version):
    lines = []
    for target in TARGETS:
        archive = output / asset_name(target, release_version)
        expected = (output / f"{archive.name}.sha256").read_text().strip()
        line = f"{checksum(archive)}  {archive.name}"
        if expected != line:
            raise ValueError(f"Checksum mismatch for {archive.name}")
        required = {executable(target), *DOCUMENTS}
        if "windows" in target:
            with zipfile.ZipFile(archive) as bundle:
                entries = bundle.infolist()
                if (
                    {entry.filename for entry in entries} != required
                    or len(entries) != len(required)
                    or any(entry.is_dir() or entry.file_size == 0 for entry in entries)
                    or bundle.testzip() is not None
                ):
                    raise ValueError(f"Invalid ZIP contents in {archive.name}")
        else:
            with tarfile.open(archive) as bundle:
                entries = bundle.getmembers()
                if (
                    {entry.name for entry in entries} != required
                    or len(entries) != len(required)
                    or any(not entry.isfile() or entry.size == 0 for entry in entries)
                    or not bundle.getmember(executable(target)).mode & 0o111
                ):
                    raise ValueError(f"Invalid tar contents or executable permissions in {archive.name}")
        lines.append(line)
    (output / "SHA256SUMS").write_text("\n".join(sorted(lines)) + "\n", encoding="utf-8")
    print(f"Verified all {len(TARGETS)} release archives and wrote SHA256SUMS")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("validate")
    build = commands.add_parser("package")
    build.add_argument("--target", required=True, choices=TARGETS)
    check = commands.add_parser("verify")
    check.add_argument("--directory", type=Path, default=ROOT / "dist")
    args = parser.parse_args()
    if args.command == "validate":
        validate()
    elif args.command == "package":
        validate()
        package(args.target)
    else:
        validate()
        verify(args.directory, version())


if __name__ == "__main__":
    main()
