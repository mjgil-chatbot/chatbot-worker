import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
HOOK = ROOT / ".githooks" / "pre-commit"
SCRIPT = ROOT / "scripts" / "verify-engineering-gate.sh"
BOOTSTRAP_SCRIPT = ROOT / "scripts" / "bootstrap-ci-siblings.sh"
PY_SCRIPT = ROOT / "scripts" / "verify_engineering_gate.py"
REGISTRY = ROOT / "contracts" / "engineering-gate.toml"
WORKFLOW = ROOT / ".github" / "workflows" / "engineering-gate.yml"


class EngineeringGateTests(unittest.TestCase):
    def test_shell_scripts_parse(self) -> None:
        result = subprocess.run(
            ["bash", "-n", str(HOOK), str(SCRIPT), str(BOOTSTRAP_SCRIPT)],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_registry_declares_repo_local_contracts(self) -> None:
        content = REGISTRY.read_text()
        self.assertIn('repo_id = "chatbot-worker"', content)
        self.assertIn('id = "engineering_docs"', content)
        self.assertIn('id = "worker_spool_contract"', content)
        self.assertIn('id = "sibling_repo_boundary"', content)
        self.assertIn("--repo chatbot-worker --repo __DESKTOP_REPO__", content)
        sibling_script = (ROOT / "scripts" / "verify-sibling-builds.sh").read_text()
        self.assertIn("CARGO_TARGET_DIR", sibling_script)
        self.assertIn("--repo <name>", sibling_script)

    def test_hook_runs_engineering_gate_script(self) -> None:
        content = HOOK.read_text()
        self.assertIn('echo "pre-commit: running engineering gate"', content)
        self.assertIn('"$repo_root/scripts/verify-engineering-gate.sh"', content)

    def test_ci_workflow_runs_engineering_gate_with_diff_base(self) -> None:
        content = WORKFLOW.read_text()
        self.assertIn("name: Engineering Gate", content)
        self.assertIn("fetch-depth: 0", content)
        self.assertIn("scripts/bootstrap-ci-siblings.sh", content)
        self.assertIn("scripts/verify-engineering-gate.sh", content)
        self.assertIn("--diff-base", content)
        self.assertIn("ENGINEERING_GATE_TARGET_SUFFIX: ci", content)

    def test_gate_fails_when_worker_contract_changes_without_doc(self) -> None:
        result = subprocess.run(
            [
                "python3",
                str(PY_SCRIPT),
                "--repo-root",
                str(ROOT),
                "--files",
                "src/worker.rs",
                "--dry-run",
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("worker_spool_contract", result.stderr)
        self.assertIn("required documentation", result.stderr)

    def test_gate_uses_real_staged_file_discovery(self) -> None:
        with tempfile.TemporaryDirectory(prefix="chatbot-worker-stage-") as tmpdir:
            repo_root = Path(tmpdir) / "chatbot-worker"
            (repo_root / "contracts").mkdir(parents=True)
            (repo_root / "src").mkdir()
            (repo_root / "docs" / "architecture").mkdir(parents=True)
            (repo_root / "src" / "worker.rs").write_text("pub fn demo() {}\n")
            (repo_root / "docs" / "architecture" / "overview.md").write_text("# overview\n")
            (repo_root / "contracts" / "engineering-gate.toml").write_text(
                textwrap.dedent(
                    """
                    version = 1
                    repo_id = "chatbot-worker"
                    repo_root_names = ["chatbot-worker"]

                    [[contract]]
                    id = "staged_contract"
                    description = "staged file discovery coverage"
                    trigger_globs = ["src/*.rs", "src/**/*.rs"]
                    required_any_staged = ["docs/architecture/overview.md"]
                    commands = ['python3 -c "print(\"ok\")"']
                    """
                ).strip()
                + "\n",
                encoding="utf-8",
            )

            subprocess.run(["git", "init"], cwd=repo_root, check=True, capture_output=True, text=True)
            subprocess.run(
                [
                    "git",
                    "add",
                    "src/worker.rs",
                    "docs/architecture/overview.md",
                    "contracts/engineering-gate.toml",
                ],
                cwd=repo_root,
                check=True,
                capture_output=True,
                text=True,
            )

            result = subprocess.run(
                [
                    "python3",
                    str(PY_SCRIPT),
                    "--repo-root",
                    str(repo_root),
                    "--registry",
                    str(repo_root / "contracts" / "engineering-gate.toml"),
                    "--dry-run",
                ],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("engineering-gate[chatbot-worker]: active contracts", result.stdout)
            self.assertIn("staged_contract", result.stdout)

    def test_gate_can_run_named_command_group(self) -> None:
        with tempfile.TemporaryDirectory(prefix="chatbot-worker-group-") as tmpdir:
            repo_root = Path(tmpdir) / "chatbot-worker"
            (repo_root / "contracts").mkdir(parents=True)
            (repo_root / "contracts" / "engineering-gate.toml").write_text(
                textwrap.dedent(
                    """
                    version = 1
                    repo_id = "chatbot-worker"
                    repo_root_names = ["chatbot-worker"]

                    [[command_group]]
                    id = "demo_group"
                    commands = ['python3 -c "print(\\"ok\\")"']
                    """
                ).strip()
                + "\n",
                encoding="utf-8",
            )

            result = subprocess.run(
                [
                    "python3",
                    str(PY_SCRIPT),
                    "--repo-root",
                    str(repo_root),
                    "--registry",
                    str(repo_root / "contracts" / "engineering-gate.toml"),
                    "--run-group",
                    "demo_group",
                    "--dry-run",
                ],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("active command groups", result.stdout)
            self.assertIn("demo_group", result.stdout)
            self.assertIn('python3 -c "print(\\"ok\\")"', result.stdout)

    def test_gate_uses_diff_range_file_discovery(self) -> None:
        with tempfile.TemporaryDirectory(prefix="chatbot-worker-diff-") as tmpdir:
            repo_root = Path(tmpdir) / "chatbot-worker"
            (repo_root / "contracts").mkdir(parents=True)
            (repo_root / "src").mkdir()
            (repo_root / "docs" / "architecture").mkdir(parents=True)
            (repo_root / "src" / "worker.rs").write_text("pub fn demo() {}\n")
            (repo_root / "docs" / "architecture" / "overview.md").write_text("# overview\n")
            (repo_root / "contracts" / "engineering-gate.toml").write_text(
                textwrap.dedent(
                    """
                    version = 1
                    repo_id = "chatbot-worker"
                    repo_root_names = ["chatbot-worker"]

                    [[contract]]
                    id = "diff_contract"
                    description = "diff discovery coverage"
                    trigger_globs = ["src/*.rs", "src/**/*.rs"]
                    required_any_staged = ["docs/architecture/overview.md"]
                    commands = ['python3 -c "print(\\"ok\\")"']
                    """
                ).strip()
                + "\n",
                encoding="utf-8",
            )

            subprocess.run(["git", "init"], cwd=repo_root, check=True, capture_output=True, text=True)
            subprocess.run(
                ["git", "config", "user.name", "Chatbot Worker Test"],
                cwd=repo_root,
                check=True,
                capture_output=True,
                text=True,
            )
            subprocess.run(
                ["git", "config", "user.email", "chatbot-worker-test@example.com"],
                cwd=repo_root,
                check=True,
                capture_output=True,
                text=True,
            )
            subprocess.run(["git", "add", "."], cwd=repo_root, check=True, capture_output=True, text=True)
            subprocess.run(
                ["git", "commit", "-m", "base"],
                cwd=repo_root,
                check=True,
                capture_output=True,
                text=True,
            )

            (repo_root / "src" / "worker.rs").write_text("pub fn demo() { println!(\"changed\"); }\n")
            (repo_root / "docs" / "architecture" / "overview.md").write_text("# overview\nchanged\n")
            subprocess.run(
                ["git", "add", "src/worker.rs", "docs/architecture/overview.md"],
                cwd=repo_root,
                check=True,
                capture_output=True,
                text=True,
            )
            subprocess.run(
                ["git", "commit", "-m", "head"],
                cwd=repo_root,
                check=True,
                capture_output=True,
                text=True,
            )

            result = subprocess.run(
                [
                    "python3",
                    str(PY_SCRIPT),
                    "--repo-root",
                    str(repo_root),
                    "--registry",
                    str(repo_root / "contracts" / "engineering-gate.toml"),
                    "--diff-base",
                    "HEAD~1",
                    "--diff-head",
                    "HEAD",
                    "--dry-run",
                ],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("engineering-gate[chatbot-worker]: active contracts", result.stdout)
            self.assertIn("diff_contract", result.stdout)

    def test_gate_rejects_registry_for_different_repo_root(self) -> None:
        with tempfile.TemporaryDirectory(prefix="worker-other-repo-") as tmpdir:
            repo_root = Path(tmpdir) / "other-repo"
            repo_root.mkdir()
            result = subprocess.run(
                [
                    "python3",
                    str(PY_SCRIPT),
                    "--repo-root",
                    str(repo_root),
                    "--registry",
                    str(REGISTRY),
                    "--dry-run",
                ],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "registry for chatbot-worker cannot run from repo root 'other-repo'",
                result.stderr,
            )


if __name__ == "__main__":
    unittest.main()
