"""Pure evidence tests: no Win32 calls or local process reproductions."""

from __future__ import annotations

import ast
import copy
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


def load(name):
    path = ROOT / "scripts" / "ci" / (name + ".py")
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


OBSERVER = load("core2442_windows_observer")
DIAGNOSTIC = load("core2442_windows_diagnostic")


def snapshot(*, live=False, phase="original_failure_seam"):
    leader = {"pid": 11, "identity_ok": True, "exit_ok": True, "wait_result": 0, "exit_code": 0}
    members = [{"pid": 12, "identity_ok": True, "exit_ok": True, "wait_result": 258, "close_ok": True}] if live else []
    value = {
        "kind": "snapshot",
        "phase": phase,
        "leader": leader,
        "members": members,
        "process_list": {"complete": True, "pids": [12] if live else []},
        "accounting": {"bool": True, "values": {"ActiveProcesses": int(live)}},
    }
    value["classification"] = OBSERVER.classification(value)
    return value


class EvidenceClassificationTests(unittest.TestCase):
    def test_signaled_leader_is_not_a_live_descendant(self):
        value = snapshot()
        value["members"] = [value["leader"]]
        value["process_list"]["pids"] = [11]
        self.assertEqual(OBSERVER.classification(value)["live_descendant_pids"], [])

    def test_live_member_is_identity_bound(self):
        self.assertEqual(OBSERVER.classification(snapshot(live=True))["live_descendant_pids"], [12])

    def test_incomplete_list_is_unknown_not_empty(self):
        value = snapshot()
        value["process_list"]["complete"] = False
        self.assertFalse(OBSERVER.classification(value)["conclusive_membership"])

    def test_open_or_membership_failure_is_unknown(self):
        value = snapshot(live=True)
        value["members"][0]["identity_ok"] = False
        self.assertFalse(OBSERVER.classification(value)["conclusive_membership"])

    def test_failed_wait_is_unknown_not_dead(self):
        value = snapshot(live=True)
        value["members"][0]["wait_result"] = 0xFFFFFFFF
        self.assertFalse(OBSERVER.classification(value)["conclusive_membership"])

    def test_failed_exit_code_is_unknown(self):
        value = snapshot(live=True)
        value["members"][0]["exit_ok"] = False
        self.assertFalse(OBSERVER.classification(value)["conclusive_membership"])

    def test_missing_listed_identity_is_unknown(self):
        value = snapshot(live=True)
        value["members"] = []
        self.assertFalse(OBSERVER.classification(value)["conclusive_membership"])

    def test_duplicate_listed_pid_is_unknown(self):
        value = snapshot(live=True)
        value["process_list"]["pids"] = [12, 12]
        self.assertFalse(OBSERVER.classification(value)["conclusive_membership"])

    def test_original_failure_is_never_swallowed(self):
        record = {"mode": "original_normal_exit", "outcome": "raised", "events": [snapshot()]}
        self.assertIn("original_normal_exit_raised", DIAGNOSTIC.validate_record(record))

    def test_empty_evidence_does_not_pass(self):
        record = {"mode": "original_normal_exit", "outcome": "returned", "events": []}
        self.assertIn("no_snapshots", DIAGNOSTIC.validate_record(record))

    def test_observer_failure_cannot_be_misreported_as_natural_green(self):
        record = {
            "mode": "original_normal_exit",
            "outcome": "returned",
            "events": [snapshot(), {"kind": "observer_error"}],
        }
        self.assertIn("observer_error", DIAGNOSTIC.validate_record(record))

    def test_live_guard_requires_rejection_and_retirement(self):
        record = {
            "mode": "live_descendant",
            "outcome": "raised",
            "exception_type": "RuntimeError",
            "exception_message": "process containment failed; descendants survived completion: 1 Windows Job Object process(es)",
            "events": [
                snapshot(phase="before_leader_resume"),
                snapshot(live=True),
                {"kind": "original_returned_count", "count": 1},
                snapshot(phase="before_original_job_close"),
            ],
        }
        self.assertEqual(DIAGNOSTIC.validate_record(record), [])
        escaped = copy.deepcopy(record)
        escaped["events"][-1]["accounting"]["values"]["ActiveProcesses"] = 1
        self.assertIn("live_descendant_cleanup_unproven", DIAGNOSTIC.validate_record(escaped))
        swallowed = copy.deepcopy(record)
        swallowed["outcome"] = "returned"
        self.assertIn("live_descendant_not_rejected", DIAGNOSTIC.validate_record(swallowed))

    def test_unknown_membership_cannot_satisfy_live_control(self):
        record = {"mode": "live_descendant", "outcome": "raised", "events": [snapshot()]}
        self.assertIn("live_descendant_not_proven", DIAGNOSTIC.validate_record(record))

    def test_fixed_delivery_has_no_reproduction_loop(self):
        self.assertEqual(DIAGNOSTIC.MODES, ("original_normal_exit", "live_descendant", "separate_handle_retirement"))
        self.assertEqual(DIAGNOSTIC.TIMEOUT_SECONDS, 5)
        tree = ast.parse((ROOT / "scripts/ci/core2442_windows_diagnostic.py").read_text(encoding="utf-8"))
        self.assertFalse(any(isinstance(node, ast.While) for node in ast.walk(tree)))
        self.assertFalse(any(isinstance(node, ast.Attribute) and node.attr == "sleep" for node in ast.walk(tree)))


if __name__ == "__main__":
    unittest.main()
