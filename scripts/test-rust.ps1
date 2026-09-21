# Run the whole Rust test suite: nextest unit/integration passes plus doctests.
#
# Invoked by `just test-rust` on Windows; `scripts/test-rust.sh` is the unix twin
# and must stay in sync with this file.
#
# Why a script instead of four recipe lines: `just` aborts a recipe at the first
# non-zero line, so one failing `cargo nextest run --workspace` silently skipped
# the ~72 `job-persist-sqlite` tests and both doctest passes. Every suite here
# runs regardless of its predecessors; the gate is preserved because the script
# exits non-zero when any suite failed.

$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'

$suites = @(
    @('cargo', 'nextest', 'run', '--workspace', '--no-fail-fast'),
    @('cargo', 'nextest', 'run', '-p', 'dcc-mcp-job', '--features', 'job-persist-sqlite', '--no-fail-fast'),
    # `--no-fail-fast` is a cargo flag on the doctest passes (before `--`): it
    # keeps the remaining crates' doctests running after one crate fails. The
    # libtest flag of the same name does not exist —
    # `cargo test --doc -- --no-fail-fast` aborts with
    # "Unrecognized option: 'no-fail-fast'" before running a single doctest.
    @('cargo', 'test', '--workspace', '--doc', '--no-fail-fast'),
    @('cargo', 'test', '-p', 'dcc-mcp-job', '--features', 'job-persist-sqlite', '--doc', '--no-fail-fast')
)

$status = 0
$failures = New-Object System.Collections.Generic.List[string]

foreach ($suite in $suites) {
    $exe = $suite[0]
    $arguments = @()
    if ($suite.Count -gt 1) {
        $arguments = $suite[1..($suite.Count - 1)]
    }
    $display = [string]::Join(' ', $suite)

    if (-not (Get-Command $exe -ErrorAction SilentlyContinue)) {
        Write-Host "==> FAILED: '$exe' is not on PATH: $display"
        $failures.Add("  - $display ($exe is not on PATH)")
        $status = 1
        continue
    }

    Write-Host "==> $display"
    & $exe @arguments
    $code = $LASTEXITCODE
    if ($code -lt 0 -or $code -gt 127) {
        # Died on a signal or crashed (Ctrl-C surfaces as a large NTSTATUS value
        # here) rather than reporting a test failure. Stop instead of launching
        # the remaining suites.
        Write-Host "==> interrupted (exit $code): $display"
        exit 130
    }
    if ($code -ne 0) {
        Write-Host "==> FAILED (exit $code): $display"
        $failures.Add("  - $display (exit $code)")
        $status = 1
    }
}

if ($failures.Count -gt 0) {
    Write-Host ''
    Write-Host 'Rust test suites failed:'
    foreach ($failure in $failures) {
        Write-Host $failure
    }
}

exit $status
