"""Real archive fixtures for curated install catalog publication gates."""

from __future__ import annotations

import copy
import hashlib
from http.client import IncompleteRead
import io
import json
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request
import zipfile

import pytest
from scripts.ci import prepare_install_catalog as publisher

REVISION = "a" * 40
RELEASE_COMMIT = "b" * 40
PACKAGE = "dcc-mcp-maya"
VERSION = "1.2.3"
WHEEL_URL = "https://files.pythonhosted.org/packages/example/dcc_mcp_maya-1.2.3-py3-none-any.whl"
PYPI_URL = "https://pypi.org/pypi/dcc-mcp-maya/1.2.3/json"
REF_URL = "https://api.github.com/repos/dcc-mcp/dcc-mcp-maya/git/ref/tags/v1.2.3"
RAW_BASE = "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-maya/" + RELEASE_COMMIT + "/"


class FakeTransport:
    def __init__(self, responses):
        self.responses = responses
        self.calls = []

    def fetch(self, url, limit, optional=False):
        self.calls.append((url, limit, optional))
        assert url in self.responses, "unexpected request: " + url
        return self.responses[url]


def _wheel(name=PACKAGE, version=VERSION, requires_python=">=3.7", extra=None, compression=zipfile.ZIP_STORED):
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w", compression=compression) as archive:
        archive.writestr("dcc_mcp_maya/__init__.py", "raise RuntimeError('must never execute')\n")
        archive.writestr("dcc_mcp_maya/cli.py", "def main(): pass\n")
        archive.writestr(
            "dcc_mcp_maya-1.2.3.dist-info/METADATA",
            f"Metadata-Version: 2.1\nName: {name}\nVersion: {version}\nRequires-Python: {requires_python}\n\n",
        )
        for member, data in (extra or {}).items():
            archive.writestr(member, data)
    return stream.getvalue()


def _fixture(data=None):
    data = _wheel() if data is None else data
    digest = hashlib.sha256(data).hexdigest()
    document = {
        "version": "1",
        "entries": [
            {
                "name": PACKAGE,
                "description": "Maya adapter",
                "dcc": ["maya"],
                "tags": ["adapter", "official"],
                "version": VERSION,
                "url": "https://github.com/dcc-mcp/dcc-mcp-maya",
                "min_core_version": "0.20.0",
                "install": {
                    "type": "pip",
                    "pip_package": PACKAGE,
                    "url": WHEEL_URL,
                    "sha256": digest,
                    "entry_point": "dcc_mcp_maya.cli:main",
                    "instructions_url": "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-maya/main/README.md",
                },
            }
        ],
    }
    pypi = {
        "info": {"name": PACKAGE, "version": VERSION, "requires_python": ">=3.7", "yanked": False},
        "urls": [
            {
                "filename": WHEEL_URL.rsplit("/", 1)[-1],
                "url": WHEEL_URL,
                "digests": {"sha256": digest},
                "size": len(data),
                "packagetype": "bdist_wheel",
                "requires_python": ">=3.7",
                "yanked": False,
            }
        ],
    }
    transport = FakeTransport(
        {
            PYPI_URL: json.dumps(pypi).encode(),
            WHEEL_URL: data,
            REF_URL: json.dumps({"object": {"type": "commit", "sha": RELEASE_COMMIT}}).encode(),
            RAW_BASE + "install.md": b"# Install this release\n",
        }
    )
    return document, transport, pypi


def _prepare(document, transport):
    return publisher.prepare_catalog(document, REVISION, 1234567, transport)


def test_prepares_exact_curated_wheel_and_release_pinned_instructions_without_importing():
    document, transport, _ = _fixture()
    original = copy.deepcopy(document)
    payload = _prepare(document, transport)
    assert document == original
    assert payload["schema_version"] == 1
    assert payload["source_revision"] == REVISION
    assert payload["issued_at"] == 1234567
    assert payload["expires_at"] == 1234567 + 7 * 86400
    entry = payload["entries"][0]
    assert entry["version"] == VERSION
    assert entry["install"]["url"] == WHEEL_URL
    assert entry["install"]["sha256"] == original["entries"][0]["install"]["sha256"]
    assert entry["install"]["instructions_url"] == RAW_BASE + "install.md"
    assert not any("latest" in call[0] for call in transport.calls)


def test_annotated_release_tag_and_pinned_document_fallback():
    document, transport, _ = _fixture()
    annotated_sha = "c" * 40
    transport.responses[REF_URL] = json.dumps({"object": {"type": "tag", "sha": annotated_sha}}).encode()
    transport.responses["https://api.github.com/repos/dcc-mcp/dcc-mcp-maya/git/tags/" + annotated_sha] = json.dumps(
        {"object": {"type": "commit", "sha": RELEASE_COMMIT}}
    ).encode()
    transport.responses[RAW_BASE + "install.md"] = None
    transport.responses[RAW_BASE + "README.md"] = b"# Release installation\n"
    assert _prepare(document, transport)["entries"][0]["install"]["instructions_url"] == RAW_BASE + "README.md"


@pytest.mark.parametrize("where", ["info", "wheel"])
def test_yanked_release_or_file_blocks_before_artifact_download(where):
    document, transport, pypi = _fixture()
    (pypi["info"] if where == "info" else pypi["urls"][0])["yanked"] = True
    transport.responses[PYPI_URL] = json.dumps(pypi).encode()
    with pytest.raises(publisher.CatalogPreparationError, match="yanked"):
        _prepare(document, transport)
    assert all(call[0] != WHEEL_URL for call in transport.calls)


@pytest.mark.parametrize("field,value", [("version", "99.0.0"), ("name", "different-package")])
def test_pypi_identity_mismatch_blocks_catalog(field, value):
    document, transport, pypi = _fixture()
    pypi["info"][field] = value
    transport.responses[PYPI_URL] = json.dumps(pypi).encode()
    with pytest.raises(publisher.CatalogPreparationError, match="identity mismatch"):
        _prepare(document, transport)


@pytest.mark.parametrize("remote_metadata", [True, False])
def test_checksum_mismatch_in_metadata_or_download_blocks_catalog(remote_metadata):
    document, transport, pypi = _fixture()
    if remote_metadata:
        pypi["urls"][0]["digests"]["sha256"] = "0" * 64
        transport.responses[PYPI_URL] = json.dumps(pypi).encode()
    else:
        transport.responses[WHEEL_URL] += b"corrupted in transit"
    with pytest.raises(publisher.CatalogPreparationError, match="SHA-256"):
        _prepare(document, transport)


@pytest.mark.parametrize(
    "kwargs,message",
    [
        ({"name": "wrong-name"}, "name/version mismatch"),
        ({"version": "0.0.1"}, "name/version mismatch"),
        ({"requires_python": ">=3.12"}, "Requires-Python mismatch"),
        ({"extra": {"other.dist-info/METADATA": "Name: other\n"}}, "exactly one METADATA"),
        ({"extra": {"../escape.py": "bad"}}, "unsafe archive"),
    ],
)
def test_matching_digest_does_not_bypass_wheel_metadata_and_archive_checks(kwargs, message):
    document, transport, _ = _fixture(_wheel(**kwargs))
    with pytest.raises(publisher.CatalogPreparationError, match=message):
        _prepare(document, transport)


def test_crc_failure_is_detected_even_when_catalog_and_pypi_hash_match_corrupt_bytes():
    data = _wheel().replace(b"def main(): pass", b"def main(): fail")
    document, transport, _ = _fixture(data)
    with pytest.raises(publisher.CatalogPreparationError, match="CRC mismatch"):
        _prepare(document, transport)


def test_declared_entry_point_must_exist_in_wheel_without_execution():
    document, transport, _ = _fixture()
    document["entries"][0]["install"]["entry_point"] = "dcc_mcp_maya.missing:main"
    with pytest.raises(publisher.CatalogPreparationError, match="entry-point module"):
        _prepare(document, transport)


def test_missing_canonical_and_fallback_document_blocks_publication():
    document, transport, _ = _fixture()
    transport.responses[RAW_BASE + "install.md"] = None
    transport.responses[RAW_BASE + "README.md"] = None
    with pytest.raises(publisher.CatalogPreparationError, match="no content"):
        _prepare(document, transport)


def test_disabled_entry_is_preserved_without_accessing_revoked_artifact():
    document, transport, _ = _fixture()
    document["entries"][0]["policy"] = {"installation": "not_available"}
    payload = _prepare(document, transport)
    assert payload["entries"] == document["entries"]
    assert transport.calls == []


@pytest.mark.parametrize("revision", ["main", "a" * 39, "g" * 40])
def test_invalid_source_revision_never_starts_network(revision):
    document, transport, _ = _fixture()
    with pytest.raises(publisher.CatalogPreparationError, match="full commit SHA"):
        publisher.prepare_catalog(document, revision, 1, transport)
    assert transport.calls == []


def test_size_limit_blocks_large_response_before_archive_parsing(monkeypatch):
    document, transport, _ = _fixture()
    monkeypatch.setattr(publisher, "MAX_ARTIFACT_BYTES", 1)
    with pytest.raises(publisher.CatalogPreparationError, match="size limit"):
        _prepare(document, transport)


def test_expansion_limit_blocks_zip_bomb_before_crc_read(monkeypatch):
    document, transport, _ = _fixture()
    monkeypatch.setattr(publisher, "MAX_EXPANDED_BYTES", 1)
    with pytest.raises(publisher.CatalogPreparationError, match="expansion limit"):
        _prepare(document, transport)


def test_zip_sources_preserve_checksum_and_are_validated():
    document, transport, _ = _fixture()
    install = document["entries"][0]["install"]
    install["type"] = "zip"
    digest = install["sha256"]
    payload = _prepare(document, transport)
    assert payload["entries"][0]["install"]["sha256"] == digest
    assert all(call[0] != PYPI_URL for call in transport.calls)


def test_pinned_git_source_resolves_exact_commit_and_keeps_pin():
    document, transport, _ = _fixture()
    install = document["entries"][0]["install"]
    install.update(type="git", url="https://github.com/dcc-mcp/dcc-mcp-maya.git", ref=RELEASE_COMMIT)
    transport.responses["https://api.github.com/repos/dcc-mcp/dcc-mcp-maya/git/commits/" + RELEASE_COMMIT] = json.dumps(
        {"sha": RELEASE_COMMIT}
    ).encode()
    assert _prepare(document, transport)["entries"][0]["install"]["ref"] == RELEASE_COMMIT
    assert all(call[0] not in (PYPI_URL, REF_URL, WHEEL_URL) for call in transport.calls)


def test_failed_cli_validation_preserves_previous_feed(tmp_path: Path, monkeypatch):
    document, transport, pypi = _fixture()
    pypi["urls"][0]["yanked"] = True
    transport.responses[PYPI_URL] = json.dumps(pypi).encode()
    monkeypatch.setattr(publisher, "HttpTransport", lambda token: transport)
    catalog = tmp_path / "catalog.yml"
    catalog.write_text(json.dumps(document), encoding="utf-8")
    output = tmp_path / "install-catalog.json"
    output.write_bytes(b"previous attested feed")
    assert (
        publisher.main(
            ["--catalog", str(catalog), "--output", str(output), "--source-revision", REVISION, "--now", "1"]
        )
        == 1
    )
    assert output.read_bytes() == b"previous attested feed"


def test_payload_bytes_are_deterministic_and_contain_no_signature_claim(tmp_path: Path):
    document, transport, _ = _fixture()
    payload = _prepare(document, transport)
    output = tmp_path / "payload.json"
    publisher.write_payload(payload, output)
    first = output.read_bytes()
    publisher.write_payload(payload, output)
    assert output.read_bytes() == first
    assert json.loads(first) == payload
    assert set(payload) == {"schema_version", "source_revision", "issued_at", "expires_at", "entries"}


@pytest.mark.parametrize(
    "url",
    [
        "http://files.pythonhosted.org/file.whl",
        "https://evil.example/file.whl",
        "https://token@api.github.com/repos/dcc-mcp/test",
        "https://api.github.com:8443/repos/dcc-mcp/test",
    ],
)
def test_transport_rejects_untrusted_urls_without_network(url):
    with pytest.raises(publisher.CatalogPreparationError, match="approved HTTPS"):
        publisher.HttpTransport().fetch(url, 1024)


def test_authenticated_api_redirect_never_forwards_credentials():
    request = Request(REF_URL, headers={"Authorization": "Bearer test-token"})
    with pytest.raises(publisher.CatalogPreparationError, match="must not redirect"):
        publisher._SafeRedirects().redirect_request(request, None, 302, "redirect", {}, RAW_BASE + "install.md")


@pytest.mark.parametrize("status,optional_absence", [(404, True), (403, False), (503, False)])
def test_only_404_can_select_document_fallback(status, optional_absence):
    class FailingOpener:
        def open(self, request, timeout):
            raise HTTPError(request.full_url, status, "request failed", {}, None)

    transport = publisher.HttpTransport()
    transport.opener = FailingOpener()
    if optional_absence:
        assert transport.fetch(RAW_BASE + "install.md", 1024, optional=True) is None
    else:
        with pytest.raises(publisher.CatalogPreparationError, match="HTTP verification"):
            transport.fetch(RAW_BASE + "install.md", 1024, optional=True)


def test_http_token_is_only_attached_to_github_api():
    requests = []

    class RecordingOpener:
        def open(self, request, timeout):
            requests.append(request)
            return io.BytesIO(b"{}").__enter__()

    transport = publisher.HttpTransport("test-token")
    transport.opener = RecordingOpener()
    transport.fetch(REF_URL, 1024)
    transport.fetch(WHEEL_URL, 1024)
    assert requests[0].get_header("Authorization") == "Bearer test-token"
    assert requests[1].get_header("Authorization") is None


def _with_yanked_obs(document, transport, pypi):
    broken = json.loads(json.dumps(document["entries"][0]).replace("maya", "obs"))
    document["entries"].append(broken)
    yanked = json.loads(json.dumps(pypi).replace("maya", "obs"))
    yanked["urls"][0]["yanked"] = True
    transport.responses["https://pypi.org/pypi/dcc-mcp-obs/1.2.3/json"] = json.dumps(yanked).encode()
    return broken


def test_refresh_quarantines_yanked_artifact_without_blocking_healthy_adapter():
    document, transport, pypi = _fixture()
    broken = _with_yanked_obs(document, transport, pypi)
    original = copy.deepcopy(document)
    payload = publisher.prepare_catalog(document, REVISION, 1, transport, quarantine_invalid=True)
    healthy, unavailable = payload["entries"]
    assert healthy["install"]["url"] == WHEEL_URL
    assert healthy["install"]["instructions_url"] == RAW_BASE + "install.md"
    assert unavailable["name"] == broken["name"]
    assert unavailable["version"] == broken["version"]
    assert unavailable["policy"] == {"installation": "not_available", "reason": publisher.QUARANTINE_REASON}
    assert "install" not in unavailable
    assert document == original


def test_unrelated_failed_validation_cannot_block_explicit_revocation():
    document, transport, pypi = _fixture()
    _with_yanked_obs(document, transport, pypi)
    document["entries"][0]["policy"] = {"installation": "not_available", "reason": "Release was revoked."}
    payload = publisher.prepare_catalog(document, REVISION, 1, transport, quarantine_invalid=True)
    revoked, failed = payload["entries"]
    assert revoked["policy"]["reason"] == "Release was revoked."
    assert revoked["policy"]["installation"] == failed["policy"]["installation"] == "not_available"
    assert all("install" not in entry for entry in payload["entries"])
    assert all(url != PYPI_URL for url, _, _ in transport.calls)


def test_quarantine_revalidates_curated_source_and_recovers_on_next_refresh():
    document, transport, pypi = _fixture()
    good_metadata = transport.responses[PYPI_URL]
    pypi["urls"][0]["yanked"] = True
    transport.responses[PYPI_URL] = json.dumps(pypi).encode()
    quarantined = publisher.prepare_catalog(document, REVISION, 1, transport, quarantine_invalid=True)
    assert quarantined["entries"][0]["policy"]["installation"] == "not_available"
    transport.responses[PYPI_URL] = good_metadata
    recovered = publisher.prepare_catalog(document, REVISION, 2, transport, quarantine_invalid=True)
    assert recovered["entries"][0]["install"]["url"] == WHEEL_URL
    assert "policy" not in recovered["entries"][0]


@pytest.mark.parametrize("failure", ["deflate", "incomplete_http"])
def test_archive_and_stream_failures_cannot_block_healthy_entries_or_revocation(failure):
    document, fixture_transport, _ = _fixture()
    data = bytearray(_wheel(compression=zipfile.ZIP_DEFLATED))
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        member = archive.infolist()[0]
        offset = member.header_offset + 30 + len(member.filename.encode()) + len(member.extra)
    data[offset] = 7  # A reserved DEFLATE block type with an otherwise valid ZIP directory.
    broken_url = "https://files.pythonhosted.org/packages/example/broken.zip"
    broken = copy.deepcopy(document["entries"][0])
    broken.update(name="dcc-mcp-blender", dcc=["blender"])
    broken["install"] = {"type": "zip", "url": broken_url, "sha256": hashlib.sha256(data).hexdigest()}
    revoked = copy.deepcopy(document["entries"][0])
    revoked.update(name="dcc-mcp-obs", dcc=["obs"], policy={"installation": "not_available"})
    document["entries"].extend([broken, revoked])
    fixture_transport.responses[broken_url] = bytes(data)

    class InterruptedResponse(io.BytesIO):
        def read(self, size=-1):
            raise IncompleteRead(b"partial artifact", 100)

    class FixtureOpener:
        def open(self, request, timeout):
            if failure == "incomplete_http" and request.full_url == broken_url:
                return InterruptedResponse()
            return io.BytesIO(fixture_transport.responses[request.full_url])

    transport = publisher.HttpTransport()
    transport.opener = FixtureOpener()
    payload = publisher.prepare_catalog(document, REVISION, 1, transport, quarantine_invalid=True)
    healthy, quarantined, withdrawn = payload["entries"]
    assert healthy["install"]["url"] == WHEEL_URL
    assert quarantined["policy"] == {"installation": "not_available", "reason": publisher.QUARANTINE_REASON}
    assert withdrawn["policy"]["installation"] == "not_available"
    assert all("install" not in entry for entry in (quarantined, withdrawn))


def test_quarantine_reason_never_serializes_raw_exception_or_transport_details():
    document, _, _ = _fixture()

    class FailedTransport:
        def fetch(self, url, limit, optional=False):
            raise publisher.CatalogPreparationError("private-token-and-local-path")

    payload = publisher.prepare_catalog(document, REVISION, 1, FailedTransport(), quarantine_invalid=True)
    assert payload["entries"][0]["policy"]["reason"] == publisher.QUARANTINE_REASON
    assert "private-token-and-local-path" not in json.dumps(payload)


@pytest.mark.parametrize(
    "document",
    [
        {"version": "2", "entries": [{"name": "example", "description": "example"}]},
        {"entries": "invalid"},
        {"entries": [{"name": "example", "description": "example", "install": "invalid"}]},
        {"entries": [{"name": "example", "description": "example", "policy": {"installation": False}}]},
        {"entries": [{"name": "example", "description": "example", "policy": {}}]},
        {"entries": [{"name": "example", "description": "example", "policy": {"reason": "Withdrawn"}}]},
        {
            "entries": [
                {"name": "example", "description": "example", "policy": {"installation": "available", "reason": ""}}
            ]
        },
    ],
)
def test_quarantine_does_not_accept_invalid_catalog_schema(document):
    with pytest.raises(publisher.CatalogPreparationError):
        publisher.prepare_catalog(document, REVISION, 1, FakeTransport({}), quarantine_invalid=True)


def test_quarantine_cli_writes_unavailable_entry_instead_of_retaining_yanked_feed(tmp_path, monkeypatch):
    document, transport, pypi = _fixture()
    pypi["urls"][0]["yanked"] = True
    transport.responses[PYPI_URL] = json.dumps(pypi).encode()
    monkeypatch.setattr(publisher, "HttpTransport", lambda token: transport)
    catalog = tmp_path / "catalog.yml"
    catalog.write_text(json.dumps(document), encoding="utf-8")
    output = tmp_path / "feed.json"
    output.write_bytes(b"previous signed feed")
    assert (
        publisher.main(
            [
                "--catalog",
                str(catalog),
                "--output",
                str(output),
                "--source-revision",
                REVISION,
                "--now",
                "1",
                "--quarantine-invalid",
            ]
        )
        == 0
    )
    entry = json.loads(output.read_bytes())["entries"][0]
    assert "install" not in entry
    assert entry["policy"]["installation"] == "not_available"
