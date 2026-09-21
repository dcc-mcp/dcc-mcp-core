#!/usr/bin/env sh
# Run the whole Rust test suite: nextest unit/integration passes plus doctests.
#
# Invoked by `just test-rust` on unix; `scripts/test-rust.ps1` is the Windows
# twin and must stay in sync with this file.
#
# Why a script instead of four recipe lines: `just` aborts a recipe at the first
# non-zero line, so one failing `cargo nextest run --workspace` silently skipped
# the ~72 `job-persist-sqlite` tests and both doctest passes. Every suite here
# runs regardless of its predecessors; the gate is preserved because the script
# exits non-zero when any suite failed.

# `set -e` is deliberately off: it would abort at the first failing suite, which
# is exactly the behaviour this script exists to remove.
set -u

status=0
failures=""

# Run one suite, remember a failure, and keep going.
run() {
    printf '==> %s\n' "$*"
    "$@"
    code=$?
    if [ "$code" -gt 128 ]; then
        # Killed by a signal (Ctrl-C is 130). Stop instead of launching the
        # remaining suites.
        printf '==> interrupted (exit %s): %s\n' "$code" "$*" >&2
        exit "$code"
    fi
    if [ "$code" -ne 0 ]; then
        printf '==> FAILED (exit %s): %s\n' "$code" "$*" >&2
        failures="$failures
  - $* (exit $code)"
        status=1
    fi
}

run cargo nextest run --workspace --no-fail-fast
run cargo nextest run -p dcc-mcp-job --features job-persist-sqlite --no-fail-fast
# `--no-fail-fast` is a cargo flag on the doctest passes (before `--`): it keeps
# the remaining crates' doctests running after one crate fails. The libtest flag
# of the same name does not exist — `cargo test --doc -- --no-fail-fast` aborts
# with "Unrecognized option: 'no-fail-fast'" before running a single doctest.
run cargo test --workspace --doc --no-fail-fast
run cargo test -p dcc-mcp-job --features job-persist-sqlite --doc --no-fail-fast

if [ -n "$failures" ]; then
    printf '\nRust test suites failed:%s\n' "$failures" >&2
fi

exit "$status"
