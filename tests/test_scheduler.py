"""Python-facing tests for the scheduler subsystem (issue #352).

These tests are skipped when the extension was built without the
``scheduler`` Cargo feature.
"""

from __future__ import annotations

import json
import textwrap

import pytest

import dcc_mcp_core

pytestmark = pytest.mark.skipif(
    getattr(dcc_mcp_core, "ScheduleSpec", None) is None,
    reason="Built without the `scheduler` Cargo feature",
)


CRON_DAILY_3AM = "0 0 3 * * *"  # sec min hour day month weekday


def test_trigger_spec_cron_valid() -> None:
    t = dcc_mcp_core.TriggerSpec.cron(CRON_DAILY_3AM, timezone="UTC", jitter_secs=120)
    assert t.kind == "cron"
    assert t.expression == CRON_DAILY_3AM
    assert t.timezone == "UTC"
    assert t.jitter_secs == 120
    assert t.path is None


def test_trigger_spec_cron_invalid_expression() -> None:
    with pytest.raises(ValueError):
        dcc_mcp_core.TriggerSpec.cron("not-a-cron")


def test_trigger_spec_cron_invalid_timezone() -> None:
    with pytest.raises(ValueError):
        dcc_mcp_core.TriggerSpec.cron(CRON_DAILY_3AM, timezone="Mars/Olympus")


def test_trigger_spec_webhook_rejects_relative_path() -> None:
    with pytest.raises(ValueError):
        dcc_mcp_core.TriggerSpec.webhook("no-slash")


def test_trigger_spec_webhook_with_secret() -> None:
    t = dcc_mcp_core.TriggerSpec.webhook("/webhooks/upload", secret_env="UPLOAD_WEBHOOK_SECRET")
    assert t.kind == "webhook"
    assert t.path == "/webhooks/upload"
    assert t.secret_env == "UPLOAD_WEBHOOK_SECRET"


def test_schedule_spec_build_and_validate() -> None:
    trigger = dcc_mcp_core.TriggerSpec.cron(CRON_DAILY_3AM)
    spec = dcc_mcp_core.ScheduleSpec(
        id="nightly_cleanup",
        workflow="scene_cleanup",
        trigger=trigger,
        inputs='{"scope": "all-scenes"}',
        enabled=True,
        max_concurrent=1,
    )
    assert spec.id == "nightly_cleanup"
    assert spec.workflow == "scene_cleanup"
    assert spec.max_concurrent == 1
    assert spec.enabled is True
    assert json.loads(spec.inputs_json) == {"scope": "all-scenes"}


def test_schedule_spec_rejects_empty_workflow() -> None:
    trigger = dcc_mcp_core.TriggerSpec.cron(CRON_DAILY_3AM)
    with pytest.raises(ValueError):
        dcc_mcp_core.ScheduleSpec(id="x", workflow=" ", trigger=trigger)


def test_parse_schedules_yaml_minimal() -> None:
    yaml = textwrap.dedent(
        """
        schedules:
          - id: nightly_cleanup
            workflow: scene_cleanup
            inputs:
              scope: all-scenes
            trigger:
              kind: cron
              expression: "0 0 3 * * *"
              timezone: UTC
              jitter_secs: 120
            enabled: true
            max_concurrent: 1

          - id: on_upload
            workflow: validate_upload
            inputs:
              path: "{{trigger.payload.file_path}}"
            trigger:
              kind: webhook
              path: /webhooks/upload
              secret_env: UPLOAD_WEBHOOK_SECRET
        """
    )
    specs = dcc_mcp_core.parse_schedules_yaml(yaml)
    assert len(specs) == 2
    nightly, on_upload = specs
    assert nightly.id == "nightly_cleanup"
    assert nightly.trigger.kind == "cron"
    assert nightly.trigger.jitter_secs == 120
    assert on_upload.trigger.kind == "webhook"
    assert on_upload.trigger.secret_env == "UPLOAD_WEBHOOK_SECRET"


def test_parse_schedules_yaml_reports_invalid_cron() -> None:
    yaml = textwrap.dedent(
        """
        schedules:
          - id: broken
            workflow: w
            trigger:
              kind: cron
              expression: "this is not a cron"
        """
    )
    with pytest.raises(ValueError):
        dcc_mcp_core.parse_schedules_yaml(yaml)


def test_hmac_round_trip() -> None:
    secret = b"top-secret"
    body = b'{"hello":"world"}'
    sig = dcc_mcp_core.hmac_sha256_hex(secret, body)
    assert sig.startswith("sha256=")
    assert dcc_mcp_core.verify_hub_signature_256(secret, body, sig)


def test_hmac_rejects_tampered_body() -> None:
    secret = b"top-secret"
    sig = dcc_mcp_core.hmac_sha256_hex(secret, b"a")
    assert not dcc_mcp_core.verify_hub_signature_256(secret, b"b", sig)


def test_hmac_rejects_missing_header() -> None:
    assert not dcc_mcp_core.verify_hub_signature_256(b"s", b"body", None)
