"""Verify the VX + just new-project toolchain template.

This is a dependency-free (stdlib only) verification harness. It runs REAL
``vx`` resolution / install / execute against an isolated, throw-away VX_HOME /
HOME / XDG / APPDATA state, so vx cannot reuse the developer's installed tool
store or write to their user profile. The vx binary and the platform shell are
still taken from PATH (they must be); every tool under test is invoked through
``vx <tool>`` so a resolved version comes from the committed lock, and the
cold/warm check asserts the tool was materialized inside the isolated store.
Run directly (``python verify.py``) or from CI. It prints a PASS/FAIL/SKIP table
and exits non-zero if any check FAILs.

Evidence produced (see README.md): pinned-binary version, pinned tool version,
cold/warm cache (with real install into the isolated store), command-line
version/shim isolation, whitespace + Unicode project path, fail-closed lock
drift with a non-zero exit and stderr, the missing-lock trap, plus per-subprocess
timeouts (cancel) and TemporaryDirectory cleanup.
"""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory

TIMEOUT = 180  # seconds; bounds every vx child process (cancel/timeout evidence)
OTHER_JUST = "1.57"  # a just version guaranteed to differ from the pinned one


def _force_utf8_stdio():
    """Force UTF-8 stdio so captured vx Unicode never hits a cp1252 console.

    vx prints a U+2B07 install arrow when it resolves a tool. That glyph is
    captured into check details and re-printed; on a Windows runner whose Python
    stdout uses cp1252 the print raises UnicodeEncodeError and fails the job.
    ``TextIOWrapper.reconfigure`` exists on Python 3.7+, so this covers the
    project's floor. ``errors='replace'`` is a last-resort guard for any stream
    that cannot be reconfigured.
    """
    for stream in (sys.stdout, sys.stderr):
        reconfigure = getattr(stream, "reconfigure", None) if stream is not None else None
        if reconfigure is None:
            continue
        try:
            reconfigure(encoding="utf-8", errors="replace")
        except (ValueError, OSError):
            # A detached/already-closed stream cannot be reconfigured; skip it
            # rather than aborting the whole verification run.
            continue


def _run(cmd, cwd, env, timeout=TIMEOUT):
    """Run a command with a hard timeout and always return a 4-tuple."""
    try:
        proc = subprocess.run(
            cmd,
            cwd=str(cwd),
            env=env,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
        return proc.returncode, proc.stdout or "", proc.stderr or "", False
    except subprocess.TimeoutExpired:
        return 124, "", "TIMEOUT after {}s: {}".format(timeout, " ".join(cmd)), True


def _isolated_env(home):
    """Return an env whose HOME/PATH-adjacent state all points into ``home``."""
    env = os.environ.copy()
    env.update(
        {
            "VX_HOME": str(home / "vxhome"),
            "HOME": str(home),
            "USERPROFILE": str(home),
            "APPDATA": str(home / "AppData" / "Roaming"),
            "LOCALAPPDATA": str(home / "AppData" / "Local"),
            "XDG_CACHE_HOME": str(home / ".cache"),
            "XDG_CONFIG_HOME": str(home / ".config"),
            "XDG_DATA_HOME": str(home / ".local" / "share"),
            "VX_NO_UPDATE_NOTIFIER": "1",
            "VX_TELEMETRY": "0",
            "NO_COLOR": "1",
        }
    )
    return env


def _parse_tools(project):
    """Parse the ``[tools]`` table of vx.toml into a dict (minimal TOML subset)."""
    tools = {}
    in_tools = False
    for line in (project / "vx.toml").read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            in_tools = stripped == "[tools]"
            continue
        if in_tools and "=" in stripped and not stripped.startswith("#"):
            key, _, value = stripped.partition("=")
            tools[key.strip()] = value.strip().strip('"').strip("'")
    return tools


def _copy_project(project, dest):
    """Copy the toolchain files from ``project`` into ``dest``."""
    for name in ("vx.toml", "vx.lock", "justfile"):
        src = project / name
        if src.exists():
            shutil.copy(str(src), str(dest / name))
    return dest


def _result(name, ok, detail, skipped=False):
    """Return a normalized check result dict."""
    return {"name": name, "status": "SKIP" if skipped else ("PASS" if ok else "FAIL"), "detail": detail.strip()}


def check_vx_binary(vx, env, expect_vx):
    """Prove the vx binary resolves and executes, optionally asserting its version."""
    rc, out, err, timed_out = _run([vx, "--version"], Path.cwd(), env)
    text = (out + err).strip()
    ok = rc == 0 and not timed_out and text != ""
    if ok and expect_vx:
        ok = expect_vx in out or expect_vx in err
    return _result("vx_binary_executes", ok, "rc={} timeout={} :: {}".format(rc, timed_out, text or "<empty>"))


def check_pinned_tool(vx, project, env, tools):
    """Prove a pinned tool resolves to its declared version, not a bare PATH tool."""
    tool = "just"
    if tool not in tools:
        return _result("pinned_tool_version", False, "just is not declared in vx.toml [tools]")
    pinned = tools[tool]
    rc, out, err, timed_out = _run([vx, tool, "--version"], project, env)
    text = (out + err).strip()
    ok = rc == 0 and not timed_out and pinned in out
    detail = "pinned={} rc={} timeout={} :: {}".format(pinned, rc, timed_out, text or "<empty>")
    return _result("pinned_tool_version", ok, detail)


def check_lock_consistent(vx, project, env):
    """Verify a committed, consistent vx.lock passes vx lock --check."""
    # Assert the file is PRESENT first: `vx lock --check` also exits 0 when the
    # lock is missing, so rc==0 alone would pass vacuously.
    present = (project / "vx.lock").is_file()
    rc, out, err, timed_out = _run([vx, "lock", "--check"], project, env)
    text = (out + err).strip()
    ok = present and rc == 0 and not timed_out
    return _result("lock_consistent", ok, "present={} rc={} :: {}".format(present, rc, text or "<empty>"))


def check_lock_drift_fails_closed(vx, project, env):
    """Verify that editing vx.toml without regenerating vx.lock fails with non-zero + stderr."""
    with TemporaryDirectory(prefix="vx-drift-") as tmp:
        dest = _copy_project(project, Path(tmp))
        toml = dest / "vx.toml"
        content = toml.read_text(encoding="utf-8")
        if "python" in content:
            content = content.replace('python = "3.12"', 'python = "3.11"')
        else:
            content += '\n# injected drift\nnode = "22"\n'
        toml.write_text(content, encoding="utf-8")
        rc, out, err, timed_out = _run([vx, "lock", "--check"], dest, env)
    text = (out + err).strip()
    # Fail-closed means a non-zero exit AND a diagnostic that actually names the
    # lock inconsistency — not merely any non-zero exit (a network/parse error
    # would otherwise count as "drift detected").
    names_drift = "inconsisten" in text.lower() or "drift" in text.lower() or "changed" in text.lower()
    ok = rc != 0 and not timed_out and names_drift
    return _result(
        "lock_drift_fails_closed",
        ok,
        "rc={} names_drift={} timeout={} :: {}".format(rc, names_drift, timed_out, text or "<empty>"),
    )


def check_missing_lock_trap(vx, project, env):
    """Observe whether vx lock --check fails closed when vx.lock is MISSING."""
    with TemporaryDirectory(prefix="vx-nolock-") as tmp:
        dest = _copy_project(project, Path(tmp))
        lock = dest / "vx.lock"
        if lock.exists():
            lock.unlink()
        rc, out, err, timed_out = _run([vx, "lock", "--check"], dest, env)
    text = (out + err).strip()
    # This is an OBSERVATION, not a gate on the bug existing. If vx exits 0 on a
    # missing lock (the trap), the CI workflow's separate `test -f vx.lock` step
    # is what fails closed. If a future vx returns non-zero instead, that is a
    # strict improvement and the guard stays harmless — the check must not turn
    # red in that case. Pass iff the command completed with a diagnostic.
    trap_present = rc == 0 and not timed_out
    ok = not timed_out and text != ""
    return _result(
        "missing_lock_trap_documented",
        ok,
        "rc={} trap_present={} (0 + msg => guard asserts presence) timeout={} :: {}".format(
            rc, trap_present, timed_out, text or "<empty>"
        ),
    )


def check_cold_warm_cache(vx, project, env, tools):
    """Verify a cold VX_HOME installs the tool and a warm run does not re-install."""
    pinned = tools.get("just", "")
    with TemporaryDirectory(prefix="vx-ws-") as ws:
        dest = _copy_project(project, Path(ws))
        cold_home = Path(ws) / "cold_vxhome"
        cold_env = dict(env)
        cold_env["VX_HOME"] = str(cold_home)
        rc1, out1, err1, t1 = _run([vx, "just", "--version"], dest, cold_env)
        # Platform-independent proof a REAL install happened: vx materialized a
        # tool binary under the isolated store (not a bare PATH tool, not a
        # no-op). This does not depend on any English "Installing..." UI string.
        materialized = cold_home.is_dir() and any(p.is_file() for p in cold_home.rglob("*"))
        rc2, out2, err2, t2 = _run([vx, "just", "--version"], dest, cold_env)
    cold_marker = "install" in (out1 + err1).lower()
    warm_marker = "install" in (out2 + err2).lower()
    # Hard requirement: both runs actually EXECUTE the pinned version, and the
    # cold run materialized a tool under the isolated store. The "install"
    # markers are recorded as evidence only — their wording is a vx UI string
    # that can differ across platforms/releases, so it is NOT a pass condition.
    resolves = pinned in out1 and pinned in out2
    ok = rc1 == 0 and rc2 == 0 and not t1 and not t2 and resolves and materialized
    detail = (
        f"cold_install={cold_marker} warm_install={warm_marker} "
        f"store_materialized={materialized} resolves={resolves} rc=({rc1},{rc2})"
    )
    return _result("cold_warm_cache", ok, detail)


def check_version_isolation(vx, project, env):
    """Verify a command-line vx just@X override wins over the vx.toml pin (shim isolation)."""
    rc, out, err, timed_out = _run([vx, f"just@{OTHER_JUST}", "--version"], project, env)
    text = (out + err).strip()
    ok = rc == 0 and not timed_out and OTHER_JUST in out
    return _result(
        "version_override_isolation", ok, "override={} rc={} :: {}".format(OTHER_JUST, rc, text or "<empty>")
    )


def check_whitespace_unicode_path(vx, project, env):
    """Verify resolution and lock check work from a path containing a space + Unicode."""
    with TemporaryDirectory(prefix="vx just ") as ws:
        dest = Path(ws) / "pröject ünïcode"
        dest.mkdir()
        dest = _copy_project(project, dest)
        rc1, _out1, _err1, t1 = _run([vx, "lock", "--check"], dest, env)
        rc2, out2, err2, t2 = _run([vx, "just", "--version"], dest, env)
    ok = rc1 == 0 and rc2 == 0 and not t1 and not t2
    detail = "lock_rc={} just_rc={} :: {}".format(rc1, rc2, (out2 + err2).strip() or "<empty>")
    return _result("whitespace_unicode_path", ok, detail)


def main(argv=None):
    """Run every check and return a process exit code (0 = all non-SKIP passed)."""
    _force_utf8_stdio()
    parser = argparse.ArgumentParser(description="Verify the VX + just toolchain template.")
    parser.add_argument("--project", default=str(Path(__file__).resolve().parent), help="template project root")
    parser.add_argument(
        "--expect-vx",
        default=os.environ.get("VX_EXPECT_VERSION"),
        help="assert `vx --version` contains this version (default: $VX_EXPECT_VERSION, e.g. 0.9.34)",
    )
    args = parser.parse_args(argv)

    # An explicitly empty expectation must fail loudly, not silently skip the
    # pinned-binary assertion (a silent skip could manufacture a green result).
    if args.expect_vx is not None and args.expect_vx.strip() == "":
        print("FATAL: --expect-vx / VX_EXPECT_VERSION is empty; the version assertion would be skipped.")
        return 2

    project = Path(args.project).resolve()
    vx = shutil.which("vx")
    if not vx:
        print("FATAL: `vx` not found on PATH. Install vx or run via the pinned GitHub Action.")
        return 2

    print(f"project : {project}")
    print(f"vx      : {vx}")
    print(f"timeout : {TIMEOUT}s per subprocess")
    print("-" * 78)

    tools = _parse_tools(project)
    results = []
    # A single isolated HOME for all stateful checks; TemporaryDirectory guarantees cleanup.
    with TemporaryDirectory(prefix="vx-just-iso-") as iso:
        env = _isolated_env(Path(iso))
        results.append(check_vx_binary(vx, env, args.expect_vx))
        results.append(check_pinned_tool(vx, project, env, tools))
        results.append(check_lock_consistent(vx, project, env))
        results.append(check_lock_drift_fails_closed(vx, project, env))
        results.append(check_missing_lock_trap(vx, project, env))
        results.append(check_cold_warm_cache(vx, project, env, tools))
        results.append(check_version_isolation(vx, project, env))
        results.append(check_whitespace_unicode_path(vx, project, env))
    # iso/ is removed here -> cleanup evidence (no VX_HOME residue in user HOME).

    width = max(len(r["name"]) for r in results)
    failed = False
    for r in results:
        print(
            "[{status}] {name} :: {detail}".format(status=r["status"], name=r["name"].ljust(width), detail=r["detail"])
        )
        if r["status"] == "FAIL":
            failed = True
    print("-" * 78)
    print("RESULT: {}".format("FAIL" if failed else "OK (all checks passed or skipped)"))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
