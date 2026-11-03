import contextlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.1.0"\n')
        for name in release.DOCUMENTS:
            (self.root / name).write_text(f"{name}\n")
        self.output = self.root / "dist"

    def package_all(self):
        with patch.object(release.subprocess, "check_output", return_value="zed-console-log 0.1.0\n"):
            for target in release.TARGETS:
                binary = self.root / "target" / target / "release" / release.executable(target)
                binary.parent.mkdir(parents=True)
                binary.write_bytes(b"native executable fixture")
                binary.chmod(0o755)
                release.package(target, self.root)

    def test_archives_match_the_installer_and_include_license(self):
        self.package_all()
        release.verify(self.output, "0.1.0")
        self.assertEqual(len((self.output / "SHA256SUMS").read_text().splitlines()), 6)

    def test_incomplete_matrix_cannot_be_published(self):
        self.package_all()
        (self.output / release.asset_name(release.TARGETS[-1], "0.1.0")).unlink()
        with self.assertRaises(FileNotFoundError):
            release.verify(self.output, "0.1.0")
        self.assertFalse((self.output / "SHA256SUMS").exists())

    def test_tampered_archive_cannot_be_published(self):
        self.package_all()
        archive = self.output / release.asset_name(release.TARGETS[0], "0.1.0")
        archive.write_bytes(b"corrupted download")
        with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
            release.verify(self.output, "0.1.0")

    def test_unexpected_binary_version_cannot_be_packaged(self):
        with patch.object(release.subprocess, "check_output", return_value="zed-console-log 0.2.0\n"):
            with self.assertRaisesRegex(ValueError, "wrong version"):
                release.package(release.TARGETS[0], self.root)
        self.assertFalse(self.output.exists())

    def test_release_tag_must_match_the_extension(self):
        with self.assertRaisesRegex(ValueError, f"Release tag must be v{release.version()}"):
            release.validate(ref="refs/tags/v999.0.0")


if __name__ == "__main__":
    with contextlib.redirect_stdout(io.StringIO()):
        unittest.main()
