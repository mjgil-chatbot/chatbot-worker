import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
README = ROOT / "README.md"
CARGO = ROOT / "Cargo.toml"
LICENSE = ROOT / "LICENSE"
CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
BOOTSTRAP_SCRIPT = ROOT / "scripts" / "bootstrap-ci-siblings.sh"
VENDORED_AGENT_MEMORY = ROOT / "crates" / "agentic_memory" / "Cargo.toml"
SHELL_SCRIPTS = sorted((ROOT / "scripts").glob("*.sh"))
ARCHITECTURE_DOCS = [
    ROOT / "docs" / "architecture" / "overview.md",
    ROOT / "docs" / "architecture" / "minimax-acp-bridge.md",
]


class ReleaseReadinessTests(unittest.TestCase):
    def test_license_exists(self) -> None:
        content = LICENSE.read_text()
        self.assertIn("BSD Zero Clause License", content)

    def test_manifest_has_release_metadata(self) -> None:
        content = CARGO.read_text()
        self.assertIn('license = "0BSD"', content)
        self.assertIn('repository = "https://github.com/mjgil/chatbot-worker"', content)
        self.assertIn('agentic_memory = { path = "crates/agentic_memory" }', content)

    def test_vendored_agentic_memory_manifest_is_sanitized(self) -> None:
        content = VENDORED_AGENT_MEMORY.read_text()
        self.assertNotIn("OpenAI", content)
        self.assertNotIn("example.invalid", content)
        self.assertIn('license = "MIT OR Apache-2.0"', content)

    def test_public_docs_do_not_reference_local_paths(self) -> None:
        forbidden_refs = [
            "/home/",
            "~/git",
            "~/.rust-chatbot",
            "~/.claude",
            ".claude-desktop/",
            ".codex/",
            ".gemini/",
            ".gemini-forge/",
            ".minimax/",
            ".minimax-forge/",
            ".nvidia/",
            ".openrouter/",
            ".qwen/",
            "<checkout-root>",
            "checked out beside `chatbot-worker`",
            "the user's home directory",
            "user's local bin directory",
        ]
        for path in [README, *ARCHITECTURE_DOCS]:
            content = path.read_text()
            for forbidden in forbidden_refs:
                self.assertNotIn(forbidden, content, f"{path.name}: {forbidden}")

    def test_ci_workflow_runs_release_checks(self) -> None:
        content = CI_WORKFLOW.read_text()
        self.assertIn("scripts/bootstrap-ci-siblings.sh", content)
        self.assertIn("python3 -m unittest discover -s tests", content)
        self.assertIn("cargo test --locked", content)
        self.assertIn("cargo build --locked", content)
        self.assertIn("verify-sibling-builds.sh --check --release", content)

    def test_shell_scripts_are_executable(self) -> None:
        for path in SHELL_SCRIPTS:
            self.assertTrue(path.stat().st_mode & 0o111, path.name)

    def test_bootstrap_script_is_valid_shell(self) -> None:
        result = subprocess.run(
            ["bash", "-n", str(BOOTSTRAP_SCRIPT)],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
