"""Execute registry confirmation with offline responses and check transport metadata."""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
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
    result = subprocess.run([bash, "-c", script], env=env, capture_output=True, text=True, timeout=15)
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
