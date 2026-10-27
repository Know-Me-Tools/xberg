"""Exercise local governance task dispatch with controlled CLI versions."""

import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest


class GovernanceRuntimeTests(unittest.TestCase):
    """Use actual Taskfiles and a PATH-local executable without running generators."""

    def setUp(self) -> None:
        """Copy task definitions into a disposable project and record CLI calls."""
        root = Path(__file__).resolve().parents[2]
        self.directory = tempfile.TemporaryDirectory(prefix="governance-runtime-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        shutil.copy(root / "Cargo.toml", self.root / "Cargo.toml")
        shutil.copy(root / "Taskfile.yml", self.root / "Taskfile.yml")
        shutil.copytree(root / ".task", self.root / ".task")
        executable = self.root / "ai-rulez"
        executable.write_text(
            '#!/bin/sh\nprintf "%s\\n" "$*" >> "$RUNTIME_CALLS"\n'
            'if [ "$1" = "--version" ]; then printf "ai-rulez version %s\\n" "$RUNTIME_VERSION"; fi\n'
        )
        executable.chmod(0o755)
        self.calls = self.root / "calls.log"
        self.environment = os.environ | {
            "PATH": str(self.root) + os.pathsep + os.environ["PATH"],
            "RUNTIME_CALLS": str(self.calls),
            "RUNTIME_VERSION": "5.6.3",
        }

    def run_task(self, target: str) -> subprocess.CompletedProcess[str]:
        """Run real task dependency dispatch with a bounded timeout."""
        return subprocess.run(
            ["task", target],
            cwd=self.root,
            env=self.environment,
            capture_output=True,
            text=True,
            timeout=20,
            check=False,
        )

    def test_should_reject_old_version_before_any_governance_command(self) -> None:
        """All seven entry points reject an executable below the minimum before doing work."""
        targets = (
            "governance:validate",
            "governance:generate",
            "governance:verify",
            "governance:check",
            "governance:hooks:install",
            "plugin:generate",
            "plugin:verify",
        )
        assert len(targets) == 7
        for target in targets:
            with self.subTest(target=target):
                self.calls.unlink(missing_ok=True)
                result = self.run_task(target)
                assert result.returncode != 0, result.stdout + result.stderr
                assert "requires ai-rulez 5.7.0 or newer; found ai-rulez version 5.6.3" in result.stderr
                assert self.calls.read_text().splitlines() == ["--version"]

    def test_should_install_latest_in_ci_and_pin_release_installations(self) -> None:
        """CI installers take the latest release; the six release-path installers pin 5.7.0."""
        root = Path(__file__).resolve().parents[2]
        taskfile = (root / "Taskfile.yml").read_text()
        assert re.search(r'AI_RULEZ_MIN_VERSION: "5\.7\.0"', taskfile) is not None
        assert "AI_RULEZ_VERSION:" not in taskfile
        release_paths = {"publish.yaml", "plugin-artifacts.yaml"}
        versions: dict[str, list[str]] = {"ci": [], "release": []}
        for path in (root / ".github/workflows").glob("*"):
            source = path.read_text()
            if "xberg-io/actions/setup-ai-rulez@" in source:
                assert "source-revision:" not in source, f"{path} overrides the released AI-Rulez runtime"
            kind = "release" if path.name in release_paths else "ci"
            versions[kind].extend(re.findall(r'ai-rulez-version: "([^"\n]+)"', source))
        assert versions["ci"] == ["latest"] * 3, versions
        assert versions["release"] == ["5.7.0"] * 6, versions

    def test_should_validate_with_minimum_or_newer_version(self) -> None:
        """The minimum and newer releases, compared numerically, reach the validation command."""
        for version in ("5.7.0", "5.7.1", "5.10.0", "6.0.0"):
            with self.subTest(version=version):
                self.calls.unlink(missing_ok=True)
                self.environment["RUNTIME_VERSION"] = version
                result = self.run_task("governance:validate")
                assert result.returncode == 0, result.stdout + result.stderr
                assert self.calls.read_text().splitlines() == ["--version", "validate"]

    def test_should_reject_versions_below_minimum_compared_numerically(self) -> None:
        """A patch release below the floor is rejected even when it sorts after it as text."""
        for version in ("5.6.10", "4.99.0"):
            with self.subTest(version=version):
                self.calls.unlink(missing_ok=True)
                self.environment["RUNTIME_VERSION"] = version
                result = self.run_task("governance:validate")
                assert result.returncode != 0, result.stdout + result.stderr
                assert f"requires ai-rulez 5.7.0 or newer; found ai-rulez version {version}" in result.stderr
                assert self.calls.read_text().splitlines() == ["--version"]


if __name__ == "__main__":
    unittest.main()
