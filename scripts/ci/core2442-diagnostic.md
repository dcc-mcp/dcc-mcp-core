# Core #2442: one-shot same-platform diagnosis

Diagnostic/test-only branch; not a product fix, merge candidate, or release.
No public API or Skill changes are needed.

The original failure occurred in Windows Server 2022 / Python 3.14.7.
A completed, separately archived local Windows 11 experiment used 150 calls
over 74.2223914 seconds without a natural normal-exit failure. It is not
repeated by this delivery and is not evidence of a runner flake.

The single branch-push workflow pins `windows-2022`, official `3.14.7` x64,
read-only repository permission, and `GITHUB_RUN_ATTEMPT=1`. It runs one job
with a five-minute job and two-minute diagnostic-step limit. No PR is required;
the workflow does not use the name `CI` or any release-triggering event.
The actual hosted image version is recorded because the image may have
changed since the failure; runner label equality is not exact image identity.

Exactly three distinct subjects execute once, never in a retry loop:

1. Original `run_bounded([sys.executable, '-c', 'raise SystemExit(0)'],
   timeout_seconds=5)`, with passive observation.
2. An event-synchronized live-descendant control using the original helper.
   It must prove a live identity-bound member, reject it, and retire the Job.
3. A separate no-child handle-retirement control. It queries accounting before
   the leader wait, after signaling, after closing the leader handle, and after
   the helper's existing bounded accounting-retirement wait.

The original normal and live-descendant calls retain the exact helper hash,
original five-second timeout, original return values, and original cleanup
order. No original helper or test assertion is changed. A failure keeps the
job red after evidence and controls are saved. No rerun or repair follows
automatically, regardless of the outcome.

Ranked hypotheses remain unconfirmed:

- Accounting retirement lags the leader's physical signaled state. A same-seam
  nonzero count with TotalProcesses=1 and fixed leader exit=0 would support it.
- A genuine descendant survives. An owned-Job member with a fixed creation
  identity, confirmed membership and zero-wait WAIT_TIMEOUT proves liveness.
- Runner/query behavior differs. Failed queries, truncated lists, failed
  identity opening, or inconsistent observations remain inconclusive.

Initial before-wait observation happens after Job assignment while the leader
is still suspended, before the original helper resumes it. After the original wait returns,
only its timestamp is recorded before the original accounting query. The
observer captures that native result first, and only then enumerates the owned
Job and reads identities. No disk writes occur at that seam; later counts never
replace the original count. Extra query handles are closed immediately and
their lifetimes are recorded. A fixed 256-PID buffer comfortably covers the
controlled subjects; overflow is explicitly inconclusive, never empty.

The original helper closes the Job before the leader handle. Therefore its
post-leader-close accounting is unavailable and is labeled as such. Holding a
duplicate Job handle would change its kill-on-last-close fence, so no duplicate
is held. Post-leader-close accounting is captured only by the separate control,
which keeps its own Job fence until completion. That control is not a proposed
product fix or an excuse to accept a surviving descendant.

Evidence includes allowlisted runner provenance, monotonic query/wait/close
timing, raw accounting BOOL/error/counts, member lists and completeness,
PID/creation time, held-handle signaled state, exit codes, Job membership,
outcomes and validation failures. It excludes environment dumps, arbitrary
process enumeration, credentials, unrelated process names, and command-line
scans. The workflow preserves stdout/stderr and JSON artifacts even on failure.

Local preflight is limited to syntax, lint and pure evidence-classification
tests. It must not invoke the process diagnostic. These are not native/full
Core gates or an additional reproduction result. A step/job watchdog timeout
is incomplete evidence, not a successful cleanup or a natural defect result.

Sources:

- https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information
- https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_process_id_list
- https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-queryinformationjobobject
