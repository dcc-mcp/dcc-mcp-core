"""CI contract for the bounded real-ingest Sentry probe."""

from __future__ import annotations

import re

from conftest import REPO_ROOT

CI_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "ci.yml"


def test_sentry_real_ingest_job_has_bounded_process_and_job_deadlines() -> None:
    workflow = CI_WORKFLOW.read_text(encoding="utf-8")
    match = re.search(r"(?ms)^  sentry-e2e:\n(.*?)(?=^  [a-zA-Z0-9_-]+:|\Z)", workflow)
    assert match is not None
    job = match.group(1)

    assert "timeout-minutes: 20" in job
    command_match = re.search(r"(?ms)^      - name: Run Sentry real-ingest E2E\n(.*?)(?=^      - |\Z)", job)
    assert command_match is not None
    command = command_match.group(1)
    assert "timeout --signal=TERM --kill-after=60s 15m" in command
    assert "sentry_real_ingest_e2e" in command
