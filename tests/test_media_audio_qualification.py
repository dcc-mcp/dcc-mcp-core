"""Tiny host-free qualification supervisor regressions; no real child process."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

if sys.version_info < (3, 12):
    raise unittest.SkipTest("Qualification supervisor targets hosted Python 3.12")

SPEC = importlib.util.spec_from_file_location(
    "audio_qualification", Path(__file__).parents[1] / "scripts/ci/qualify_media_audio.py"
)
qualification = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(qualification)


class QualificationLogTests(unittest.TestCase):
    def setUp(self):
        # The Linux supervisor is fully mocked: Windows has no SIGKILL.
        signal_patch = mock.patch.object(qualification.signal, "SIGKILL", 9, create=True)
        signal_patch.start()
        self.addCleanup(signal_patch.stop)

    def test_workflow_resolves_runner_temp_inside_a_step(self):
        import yaml

        workflow = Path(__file__).parents[1] / ".github/workflows/ci.yml"
        job = yaml.safe_load(workflow.read_text())["jobs"]["media-audio-native"]
        self.assertNotIn("MEDIA_AUDIO_EVIDENCE", job["env"])
        self.assertFalse(any("runner." in str(value) for value in job["env"].values()))
        first = job["steps"][0]
        self.assertEqual(first["shell"], "bash")
        self.assertEqual(
            first["run"],
            'echo "MEDIA_AUDIO_EVIDENCE=$RUNNER_TEMP/media-audio-evidence" >> "$GITHUB_ENV"',
        )
        upload = next(step for step in job["steps"] if step.get("uses", "").startswith("actions/upload-artifact@"))
        self.assertIn("${{ runner.temp }}/media-audio-evidence/", upload["with"]["path"])

    def test_child_environment_does_not_inherit_runner_secrets(self):
        sentinels = {
            "GITHUB_TOKEN": "synthetic-github-sentinel",
            "ACTIONS_RUNTIME_TOKEN": "synthetic-actions-sentinel",
            "AWS_SECRET_ACCESS_KEY": "synthetic-aws-sentinel",
            "UNEXPECTED_PRIVATE_KEY": "synthetic-arbitrary-sentinel",
            "PATH": "/synthetic/untrusted/path",
        }
        with mock.patch.dict(qualification.os.environ, sentinels, clear=True):
            env = qualification.child_environment(Path("/isolated/home"), Path("/isolated/bin"))
        self.assertEqual(
            set(env),
            {
                "HOME",
                "VX_HOME",
                "XDG_CACHE_HOME",
                "TMPDIR",
                "PATH",
                "LANG",
                "DCC_MCP_MEDIA_AUTO_INSTALL_VX",
            },
        )
        self.assertEqual(env["PATH"], str(Path("/isolated/bin")))
        self.assertFalse(set(sentinels.values()) & set(env.values()))

    def test_native_environment_retains_only_explicit_isolation_controls(self):
        receipt = {
            "vx": {"path": "/verified/bin/vx"},
            "executables": {
                "ffmpeg": {"path": "/verified/ffmpeg"},
                "ffprobe": {"path": "/verified/ffprobe"},
            },
        }
        with mock.patch.dict(qualification.os.environ, {"ARBITRARY_TOKEN": "synthetic-only"}, clear=True):
            env = qualification.native_environment(Path("/evidence"), receipt)
        self.assertNotIn("ARBITRARY_TOKEN", env)
        self.assertEqual(env["DCC_MCP_GATEWAY_PORT"], "0")
        self.assertEqual(env["DCC_MCP_DISABLE_DEFAULT_SKILL_PATHS"], "1")
        self.assertEqual(env["DCC_MCP_REGISTRY_DIR"], str(Path("/evidence") / "registry"))
        self.assertEqual(env["DCC_MCP_CHECKPOINT_DIR"], str(Path("/evidence") / "checkpoints"))
        self.assertEqual(env["DCC_MCP_MEDIA_VX_BIN"], str(Path("/verified/bin/vx")))
        self.assertEqual(env["DCC_MCP_MEDIA_FFMPEG_BIN"], "/verified/ffmpeg")
        self.assertEqual(env["DCC_MCP_MEDIA_FFPROBE_BIN"], "/verified/ffprobe")

    def test_native_launcher_executes_only_hosted_module_with_closed_environment(self):
        receipt = {
            "vx": {"path": "/verified/bin/vx"},
            "executables": {
                "ffmpeg": {"path": "/verified/ffmpeg"},
                "ffprobe": {"path": "/verified/ffprobe"},
            },
        }
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "qualification.json").write_text(json.dumps(receipt))
            with mock.patch.dict(
                qualification.os.environ,
                {
                    "GITHUB_ACTIONS": "true",
                    "MEDIA_AUDIO_EVIDENCE": tmp,
                    "ARBITRARY_TOKEN": "synthetic-only",
                },
                clear=True,
            ), mock.patch.object(qualification.os, "execve") as execute:
                qualification.native_tests()
            executable, argv, env = execute.call_args.args
            self.assertEqual(executable, str(Path(sys.executable).resolve()))
            self.assertEqual(argv[:4], [executable, "-m", "pytest", "tests/test_media_audio_hosted.py"])
            self.assertNotIn("ARBITRARY_TOKEN", env)
            self.assertNotIn("GITHUB_ACTIONS", env)
            self.assertTrue(Path(env["DCC_MCP_CHECKPOINT_DIR"]).is_dir())

    # The supervisor targets Linux; create mocked killpg on Windows too so these
    # host-free failure-path tests exercise the same contract without native calls.
    def fake_process(self, *, stdout_size=0, stderr_size=0, returncode=0):
        def start(argv, **kwargs):
            kwargs["stdout"].write(b"o" * stdout_size)
            kwargs["stderr"].write(b"e" * stderr_size)
            process = mock.Mock(pid=987654, returncode=returncode)
            process.poll.return_value = returncode
            process.wait.return_value = returncode
            self.process = process
            return process

        return start

    def test_terminal_stdout_overflow_rejected_before_read(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(
            qualification.subprocess, "Popen", self.fake_process(stdout_size=4 * 1024**2 + 1)
        ), mock.patch.object(qualification.os, "killpg", create=True), mock.patch.object(
            qualification, "_bounded_text"
        ) as read:
            with self.assertRaisesRegex(RuntimeError, "Closed command logs exceeded"):
                qualification.run(["test-double"], Path(tmp), {})
            read.assert_not_called()
            self.process.wait.assert_called_once_with(timeout=10)
            receipt = json.loads((Path(tmp) / "command-000.json").read_text())
            self.assertTrue(receipt["log_limit_exceeded"])
            self.assertTrue(receipt["launcher_reaped"])

    def test_terminal_stderr_overflow_rejected_before_diagnostics(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(
            qualification.subprocess, "Popen", self.fake_process(stderr_size=4 * 1024**2 + 1, returncode=1)
        ), mock.patch.object(qualification.os, "killpg", create=True), mock.patch.object(
            qualification, "_stderr_tail"
        ) as tail:
            with self.assertRaisesRegex(RuntimeError, "Closed command logs exceeded"):
                qualification.run(["test-double"], Path(tmp), {})
            tail.assert_not_called()

    def test_failure_diagnostic_reads_only_tail(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(
            qualification.subprocess, "Popen", self.fake_process(stderr_size=100000, returncode=1)
        ), mock.patch.object(qualification.os, "killpg", create=True):
            with self.assertRaises(RuntimeError) as raised:
                qualification.run(["test-double"], Path(tmp), {})
            self.assertEqual(raised.exception.args[0][2], "e" * 4000)

    def test_cleanup_uncertainty_preserved(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(
            qualification.subprocess, "Popen", self.fake_process()
        ), mock.patch.object(qualification.os, "killpg", create=True, side_effect=OSError("test cleanup failure")):
            with self.assertRaisesRegex(RuntimeError, "cleanup is uncertain"):
                qualification.run(["test-double"], Path(tmp), {})
            receipt = json.loads((Path(tmp) / "command-000.json").read_text())
            self.assertEqual(receipt["cleanup_status"], "unknown")
            self.assertIn("test cleanup failure", receipt["cleanup_errors"][0])
            self.assertTrue(receipt["launcher_reaped"])


if __name__ == "__main__":
    unittest.main()
