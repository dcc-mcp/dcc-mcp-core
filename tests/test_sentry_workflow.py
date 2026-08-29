"""CI contract for the bounded real-ingest Sentry probe."""

from __future__ import annotations

import re

from conftest import REPO_ROOT

CI_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "ci.yml"


def _sentry_e2e_job() -> str:
    workflow = CI_WORKFLOW.read_text(encoding="utf-8")
    match = re.search(r"(?ms)^  sentry-e2e:\n(.*?)(?=^  [a-zA-Z0-9_-]+:|\Z)", workflow)
    assert match is not None
    return match.group(1)


def _sentry_e2e_command() -> str:
    job = _sentry_e2e_job()
    command_match = re.search(r"(?ms)^      - name: Run Sentry real-ingest E2E\n(.*?)(?=^      - |\Z)", job)
    assert command_match is not None
    return command_match.group(1)


def test_sentry_real_ingest_job_has_bounded_process_and_job_deadlines() -> None:
    job = _sentry_e2e_job()
    assert "timeout-minutes: 20" in job
    command = _sentry_e2e_command()
    assert "timeout --signal=TERM --kill-after=60s 15m" in command
    assert "sentry_real_ingest_e2e" in command


def test_sentry_real_ingest_job_requires_the_target_test_to_run() -> None:
    command = _sentry_e2e_command()

    # The test is nested under `sentry_init::tests`; a bare selector can
    # produce a green cargo-test exit status while running zero tests.
    assert "sentry_init::tests::sentry_real_ingest_e2e" in command
    assert re.search(r"grep\s+-Eq\s+['\"]running \[1-9\]\[0-9\]\* tests?", command)
    assert "exit 1" in command
