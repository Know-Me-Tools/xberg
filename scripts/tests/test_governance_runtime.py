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

    def test_should_reject_wrong_version_before_any_governance_command(self) -> None:
        """All seven entry points reject the mismatched executable before doing work."""
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
                assert "requires ai-rulez 5.7.0" in result.stderr
                assert self.calls.read_text().splitlines() == ["--version"]

    def test_should_match_all_nine_workflow_installations(self) -> None:
        """Local enforcement and every CI installer use one released version."""
        root = Path(__file__).resolve().parents[2]
        taskfile = (root / "Taskfile.yml").read_text()
        expected = re.search(r'AI_RULEZ_VERSION: "([^"\n]+)"', taskfile)
        assert expected is not None
        versions = []
        for path in (root / ".github/workflows").glob("*"):
            source = path.read_text()
            if "xberg-io/actions/setup-ai-rulez@" in source:
                assert "source-revision:" not in source, f"{path} overrides the released AI-Rulez runtime"
            versions.extend(re.findall(r'ai-rulez-version: "([^"\n]+)"', source))
        assert len(versions) == 9
        assert set(versions) == {expected[1]}

    def test_should_validate_with_exact_reviewed_version(self) -> None:
        """A matching executable reaches the original validation command."""
        self.environment["RUNTIME_VERSION"] = "5.7.0"
        result = self.run_task("governance:validate")
        assert result.returncode == 0, result.stdout + result.stderr
        assert self.calls.read_text().splitlines() == ["--version", "validate"]


if __name__ == "__main__":
    unittest.main()
