"""Execute registry confirmation with offline responses and check transport metadata."""

from __future__ import annotations

import contextlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time
from urllib.parse import urlsplit

import pytest

from dcc_mcp_core import yaml_loads

REPO_ROOT = Path(__file__).resolve().parents[1]
SERVER_NAME = "io.github.dcc-mcp/dcc-mcp-core"
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "publish-mcp-registry.yml"


def _response(version, name=SERVER_NAME):
    """Build a registry detail response that also models historical backfills."""
    return {
        "server": {"name": name, "version": version},
        "_meta": {"io.modelcontextprotocol.registry/official": {"isLatest": False}},
    }


def _spawn_kwargs() -> dict:
    """Isolate POSIX fixture groups and preserve the Windows no-console convention."""
    if os.name == "nt":
        return {"creationflags": getattr(subprocess, "CREATE_NO_WINDOW", 0)}
    return {"start_new_session": True}


def _terminate_tree(proc: subprocess.Popen) -> None:
    """Stop this helper's isolated POSIX group or its Windows process tree."""
    if os.name == "nt":
        if proc.poll() is not None:
            return
        # `taskkill /T` is the only portable way to reap grandchildren here:
        # killing bash alone leaves the per-curl `python` fixture alive.
        subprocess.run(
            ["taskkill", "/PID", str(proc.pid), "/T", "/F"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=10,
            **_spawn_kwargs(),
        )
    else:
        # _run_script creates a new session whose group ID is the shell PID.
        # Signal before wait/poll can reap that leader and permit PID reuse.
        with contextlib.suppress(ProcessLookupError):
            os.killpg(proc.pid, signal.SIGKILL)


def _run_script(bash, script, env, tmp_path, timeout=15):
    """Run the workflow body with file-backed output capture.

    File capture avoids waiting for EOF on inherited pipe handles. A timeout
    stops the isolated group created for this invocation before reaping the
    shell.
    """
    stdout_path = tmp_path / "stdout.log"
    stderr_path = tmp_path / "stderr.log"
    with stdout_path.open("w", encoding="utf-8", errors="replace") as out, stderr_path.open(
        "w", encoding="utf-8", errors="replace"
    ) as err:
        proc = subprocess.Popen([bash, "-c", script], env=env, stdout=out, stderr=err, **_spawn_kwargs())
        try:
            returncode = proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            _terminate_tree(proc)
            with contextlib.suppress(subprocess.TimeoutExpired):
                proc.wait(timeout=5)
            returncode = 124
    return subprocess.CompletedProcess(
        proc.args,
        returncode,
        stdout_path.read_text(encoding="utf-8", errors="replace"),
        stderr_path.read_text(encoding="utf-8", errors="replace"),
    )


def _confirm(tmp_path, responses, version="0.20.42"):
    """Run the real confirmation shell against bounded, offline curl fixtures."""
    bash = shutil.which("bash")
    if os.name == "nt" or bash is None:
        pytest.skip("the registry workflow executes on an Ubuntu bash runner")
    workflow = yaml_loads(WORKFLOW.read_text(encoding="utf-8"))
    step = next(
        step for step in workflow["jobs"]["publish"]["steps"] if step.get("name") == "Confirm the entry is queryable"
    )
    (tmp_path / "responses.json").write_text(json.dumps(responses), encoding="utf-8")
    (tmp_path / "curl.py").write_text(
        "import json, os, sys\n"
        "from pathlib import Path\n"
        "root = Path(os.environ['FIXTURE_ROOT'])\n"
        "log = root / 'requests.json'\n"
        "requests = json.loads(log.read_text()) if log.exists() else []\n"
        "responses = json.loads((root / 'responses.json').read_text())\n"
        "response = responses[min(len(requests), len(responses) - 1)]\n"
        "requests.append(sys.argv[1:])\n"
        "log.write_text(json.dumps(requests))\n"
        "print(response.get('body', ''))\n"
        "sys.exit(response.get('exit_code', 0))\n",
        encoding="utf-8",
    )
    env = {
        "PATH": os.pathsep.join([str(Path(sys.executable).parent), os.defpath]),
        "SERVER_NAME": SERVER_NAME,
        "VERSION": version,
        "MCP_REGISTRY_URL": "https://registry.invalid/",
        "GITHUB_STEP_SUMMARY": str(tmp_path / "summary.md"),
        "FIXTURE_ROOT": str(tmp_path),
    }
    env.update({key: os.environ[key] for key in ("SYSTEMROOT", "WINDIR") if key in os.environ})
    # Execute the real workflow body. Only curl and sleep are replaced; no
    # registry request, publication or authentication can occur in these tests.
    script = 'curl() { python "$FIXTURE_ROOT/curl.py" "$@"; }\nsleep() { :; }\n' + step["run"]
    result = _run_script(bash, script, env, tmp_path)
    requests = json.loads((tmp_path / "requests.json").read_text(encoding="utf-8"))
    return result, requests, tmp_path / "summary.md"


def _body(payload):
    """Encode a registry payload as the fixture HTTP response body."""
    return {"body": json.dumps(payload)}


@pytest.mark.parametrize("version", ["0.20.42", "0.20.40", "0.20.42-rc.1+build.2"])
def test_confirmation_queries_exact_version_including_backfills(tmp_path, version):
    """Accept exact identities and encode path components for current or older versions."""
    result, requests, summary = _confirm(tmp_path, [_body(_response(version))], version)
    assert result.returncode == 0, result.stderr
    assert len(requests) == 1
    encoded_version = version.replace("+", "%2B")
    assert requests[0][-1] == (
        "https://registry.invalid/v0.1/servers/io.github.dcc-mcp%2Fdcc-mcp-core/versions/" + encoded_version
    )
    assert f"{SERVER_NAME} {version} is queryable" in result.stdout
    assert SERVER_NAME in summary.read_text(encoding="utf-8")


@pytest.mark.parametrize(
    "payload",
    [
        _response("0.20.41"),
        _response("0.20.42", "io.github.other/other-server"),
        {"server": {"name": SERVER_NAME}},
        {},
        {"server": None},
        {"servers": [_response("0.20.42")]},
        {"servers": [_response("0.20.41")]},
    ],
    ids=[
        "stale-version",
        "wrong-name",
        "missing-version",
        "missing-server",
        "null-server",
        "list-response",
        "stale-list",
    ],
)
def test_confirmation_rejects_unverified_responses_after_bounded_retries(tmp_path, payload):
    """Reject mismatched or malformed identities without announcing publication success."""
    result, requests, summary = _confirm(tmp_path, [_body(payload)])
    assert result.returncode == 1
    assert len(requests) == 10
    assert "was published but is not queryable yet" in result.stdout
    assert "::notice::" not in result.stdout
    assert not summary.exists()


def test_confirmation_recovers_after_stale_response_http_failure_and_invalid_json(tmp_path):
    """Retry transient failures until the requested server version can be verified."""
    responses = [
        _body(_response("0.20.41")),
        {"exit_code": 22},
        {"body": "temporarily invalid JSON"},
        _body(_response("0.20.42")),
    ]
    result, requests, summary = _confirm(tmp_path, responses)
    assert result.returncode == 0, result.stderr
    assert len(requests) == 4
    assert result.stdout.count("not indexed yet") == 3
    assert summary.exists()


def test_curl_failure_cannot_be_masked_by_a_matching_body(tmp_path):
    """Preserve pipefail when curl fails despite returning a matching JSON body."""
    response = _body(_response("0.20.42"))
    response["exit_code"] = 22
    result, requests, summary = _confirm(tmp_path, [response])
    assert result.returncode == 1
    assert len(requests) == 10
    assert not summary.exists()


def _pypi_body(marker=True):
    """Encode a PyPI JSON response, optionally carrying the ownership marker."""
    description = f"<!-- mcp-name: {SERVER_NAME} -->" if marker else "A server with no marker."
    return {"body": json.dumps({"info": {"description": description}})}


def _wait_for_marker(tmp_path, responses, version="0.20.42", wait_seconds="3", clock=False):
    """Run the real PyPI wait body against bounded, offline curl fixtures."""
    bash = shutil.which("bash")
    if os.name == "nt" or bash is None:
        pytest.skip("the registry workflow executes on an Ubuntu bash runner")
    workflow = yaml_loads(WORKFLOW.read_text(encoding="utf-8"))
    step = next(
        step
        for step in workflow["jobs"]["publish"]["steps"]
        if step.get("name") == "Wait for PyPI to carry the ownership marker"
    )
    (tmp_path / "responses.json").write_text(json.dumps(responses), encoding="utf-8")
    (tmp_path / "curl.py").write_text(
        "import json, os, sys\n"
        "from pathlib import Path\n"
        "root = Path(os.environ['FIXTURE_ROOT'])\n"
        "log = root / 'requests.json'\n"
        "requests = json.loads(log.read_text()) if log.exists() else []\n"
        "responses = json.loads((root / 'responses.json').read_text())\n"
        "response = responses[min(len(requests), len(responses) - 1)]\n"
        "requests.append(sys.argv[1:])\n"
        "log.write_text(json.dumps(requests))\n"
        # Honour `-o <path>` the way curl does: the wait body reads the payload
        # back from that file, so a fixture that only printed it would hide the
        # marker check behind a missing-file error instead of exercising it.
        "if response.get('exit_code', 0) == 0:\n"
        "    args = sys.argv[1:]\n"
        "    for index, arg in enumerate(args):\n"
        "        if arg == '-o' and index + 1 < len(args):\n"
        "            Path(args[index + 1]).write_text(response.get('body', ''), encoding='utf-8')\n"
        "            break\n"
        "sys.exit(response.get('exit_code', 0))\n",
        encoding="utf-8",
    )
    env = {
        "PATH": os.pathsep.join([str(Path(sys.executable).parent), os.defpath]),
        "VERSION": version,
        "PYPI_PACKAGE": "dcc-mcp-server",
        "PYPI_WAIT_SECONDS": wait_seconds,
        "PYPI_POLL_INTERVAL_SECONDS": "1",
        "RUNNER_TEMP": str(tmp_path),
        "GITHUB_STEP_SUMMARY": str(tmp_path / "summary.md"),
        "FIXTURE_ROOT": str(tmp_path),
    }
    env.update({key: os.environ[key] for key in ("SYSTEMROOT", "WINDIR") if key in os.environ})
    # A stubbed clock turns the wall-clock budget into an exact poll count, so
    # "budget, not a fixed attempt count" can be asserted without a
    # timing-dependent test: every `date +%s` advances the fake clock 1s.
    clock_script = ""
    if clock:
        (tmp_path / "clock").write_text("0", encoding="utf-8")
        clock_script = (
            'date() { local tick; tick=$(cat "$FIXTURE_ROOT/clock"); '
            'echo "$tick"; echo "$(( tick + 1 ))" > "$FIXTURE_ROOT/clock"; }\n'
        )
    # Execute the real workflow body. Only curl, sleep and (optionally) the
    # clock are replaced; no PyPI request can leave the machine. The stubbed
    # sleep keeps a wall-clock budget short without shortening the polls it
    # allows.
    script = 'curl() { python "$FIXTURE_ROOT/curl.py" "$@"; }\nsleep() { :; }\n' + clock_script + step["run"]
    result = _run_script(bash, script, env, tmp_path, timeout=60)
    requests = json.loads((tmp_path / "requests.json").read_text(encoding="utf-8"))
    return result, requests


def test_marker_wait_accepts_a_marker_carried_by_the_published_readme(tmp_path):
    """Publish once the marker is readable on PyPI, and query the exact version."""
    result, requests = _wait_for_marker(tmp_path, [_pypi_body()])
    assert result.returncode == 0, result.stderr
    assert len(requests) == 1
    assert requests[0][-3] == "https://pypi.org/pypi/dcc-mcp-server/0.20.42/json"
    assert "carries the ownership marker" in result.stdout
    assert "::error::" not in result.stdout


def test_marker_wait_keeps_polling_until_the_upload_completes(tmp_path):
    """Absorb upload latency instead of reporting a missing marker too early."""
    result, requests = _wait_for_marker(tmp_path, [{"exit_code": 22}, {"exit_code": 22}, _pypi_body()])
    assert result.returncode == 0, result.stderr
    assert len(requests) == 3
    assert result.stdout.count("waiting for dcc-mcp-server 0.20.42 on PyPI") == 2
    assert "carries the ownership marker" in result.stdout


def test_marker_wait_rejects_a_version_published_without_the_marker(tmp_path):
    """Stop on a published-but-unmarked version: that release can never register."""
    result, requests = _wait_for_marker(tmp_path, [_pypi_body(marker=False)])
    assert result.returncode == 1
    assert len(requests) == 1
    assert "is on PyPI without the mcp-name marker" in result.stdout
    assert "::notice::" not in result.stdout


def test_marker_wait_budget_is_wall_clock_not_a_fixed_attempt_count(tmp_path):
    """Poll past the old 40-attempt cap and report the budget that ran out."""
    result, requests = _wait_for_marker(tmp_path, [{"exit_code": 22}], wait_seconds="100", clock=True)
    assert result.returncode == 1
    # The stubbed clock advances 2s per poll (one loop check, one remaining
    # check), so a 100s budget allows exactly 50 polls - deterministically more
    # than the 40 the fixed-attempt loop used to allow before it gave up.
    assert len(requests) == 50
    assert "never appeared on PyPI within 100s (50 attempts)" in result.stdout
    assert "publish-server-pypi" in result.stdout


@pytest.mark.parametrize("port", [None, "19765"])
def test_package_transport_uses_the_resolved_gateway_port(port):
    """Resolve the client endpoint from the same default or overridden package port."""
    metadata = json.loads((REPO_ROOT / "server.json").read_text(encoding="utf-8"))
    package = metadata["packages"][0]
    environment = {item["name"]: item["default"] for item in package["environmentVariables"]}
    if port is not None:
        environment["DCC_MCP_GATEWAY_PORT"] = port
    url = package["transport"]["url"].format_map(environment)
    endpoint = urlsplit(url)
    assert endpoint.scheme == "http"
    assert endpoint.hostname == "127.0.0.1"
    assert endpoint.path == "/mcp"
    assert endpoint.port == int(environment["DCC_MCP_GATEWAY_PORT"])


@pytest.mark.skipif(os.name == "nt", reason="POSIX session and process-group regression")
def test_timeout_stops_owned_group_and_preserves_other_session(tmp_path):
    """Stop a shell's live child on timeout without signaling a separate session."""
    bash = shutil.which("bash")
    ps = shutil.which("ps")
    if bash is None or ps is None:
        pytest.skip("this POSIX lifecycle check needs bash and ps")

    def active(child):
        """Check the recorded child without treating a reused PID as owned."""
        status = subprocess.run(
            [ps, "-p", str(child["pid"]), "-o", "stat=", "-o", "pgid="],
            capture_output=True,
            text=True,
            timeout=3,
        )
        assert status.returncode in (0, 1) and not status.stderr.strip(), status.stderr
        fields = status.stdout.split()
        # An exited child may remain briefly as an init-owned zombie.
        # A PID reused by another group no longer identifies our child.
        return bool(fields) and int(fields[1]) == child["group"] and not fields[0].startswith("Z")

    def wait_for_exit(child, timeout):
        """Observe child termination within a fixed wall-clock budget."""
        deadline = time.monotonic() + timeout
        while active(child):
            if time.monotonic() >= deadline:
                return False
            time.sleep(0.02)
        return True

    child_info = tmp_path / "child.json"
    script = """printf '%s' "$$" > "$FIXTURE_ROOT/shell.pid"
python -c 'import json, os, time; from pathlib import Path; Path(os.environ["FIXTURE_ROOT"], "child.json").write_text(json.dumps({"pid": os.getpid(), "group": os.getpgrp()})); time.sleep(8)' &
wait
"""
    env = {"PATH": os.pathsep.join([str(Path(sys.executable).parent), os.defpath]), "FIXTURE_ROOT": str(tmp_path)}
    child = None
    sibling = subprocess.Popen(
        [sys.executable, "-c", "import time; time.sleep(20)"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    try:
        started = time.monotonic()
        result = _run_script(bash, script, env, tmp_path, timeout=2)
        assert result.returncode == 124
        assert time.monotonic() - started < 7
        assert child_info.exists(), "fixture child must start before exercising timeout cleanup"
        child = json.loads(child_info.read_text(encoding="utf-8"))
        shell_pid = int((tmp_path / "shell.pid").read_text(encoding="utf-8"))
        assert child["group"] == shell_pid
        assert child["group"] != os.getpgrp()
        assert wait_for_exit(child, 2), "the timed-out fixture child survived"
        assert sibling.poll() is None, "cleanup must not signal a different session"
    finally:
        # The synthetic child self-exits after eight seconds even against a
        # regressed helper. Never compensate by signaling an observed PID.
        try:
            if child is None and child_info.exists():
                child = json.loads(child_info.read_text(encoding="utf-8"))
            if child is not None:
                assert wait_for_exit(child, 10), "finite fixture did not exit"
        finally:
            sibling.terminate()
            sibling.wait(timeout=5)
