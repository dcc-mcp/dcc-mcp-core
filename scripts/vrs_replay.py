#!/usr/bin/env python3
r"""Verified Regression Suite (VRS) — replay HTTP traces against a gateway or per-DCC REST surface.

Usage:
  python scripts/vrs_replay.py --base-url http://127.0.0.1:9765 \
      --trace tests/vrs/traces/gateway-smoke.jsonl

Environment:
  VRS_HTTP_TIMEOUT_SECS — request timeout (default 120)

Exit codes:
  0 — all steps passed, or trace was skipped by skip_preflight (e.g. no matching hosts).
  1 — a step failed or the trace file was invalid.
"""

import argparse
import json
from pathlib import Path
import sys
import time
from typing import Any
from typing import Dict
from typing import List
from typing import Mapping
from typing import Optional
from typing import Tuple
import urllib.error
import urllib.request


def _get_by_pointer(data: Any, pointer: str) -> Any:
    """Resolve a /-separated pointer (JSON Pointer-like, no escapes). Empty returns root."""
    if not pointer or pointer == "/":
        return data
    cur: Any = data
    for part in pointer.strip("/").split("/"):
        if cur is None:
            return None
        if isinstance(cur, list):
            cur = cur[int(part)]
        elif isinstance(cur, dict):
            cur = cur.get(part)
        else:
            return None
    return cur


def _json_subset_match(big: Any, small: Any) -> bool:
    """Return whether *small* is a recursive subset of *big* (dict keys; list index-wise)."""
    if isinstance(small, dict):
        if not isinstance(big, dict):
            return False
        return all(k in big and _json_subset_match(big[k], v) for k, v in small.items())
    if isinstance(small, list):
        if not isinstance(big, list) or len(big) < len(small):
            return False
        return all(_json_subset_match(big[i], small[i]) for i in range(len(small)))
    return big == small


def _substitute_captures(obj: Any, captures: Mapping[str, str]) -> Any:
    if isinstance(obj, str):
        out = obj
        for k, v in captures.items():
            out = out.replace("{{capture:" + k + "}}", v)
        return out
    if isinstance(obj, list):
        return [_substitute_captures(x, captures) for x in obj]
    if isinstance(obj, dict):
        return {k: _substitute_captures(v, captures) for k, v in obj.items()}
    return obj


def _do_request(
    base: str,
    method: str,
    path: str,
    body: Optional[Any],
    extra_headers: Optional[Mapping[str, Any]],
    timeout: float,
) -> Tuple[int, str, Any, Dict[str, str]]:
    url = base.rstrip("/") + path
    data_bytes: Optional[bytes] = None
    headers = {"Accept": "application/json", "Content-Type": "application/json"}
    if extra_headers:
        for key, value in extra_headers.items():
            if value is not None:
                headers[str(key)] = str(value)
    if body is not None:
        data_bytes = json.dumps(body).encode("utf-8")
    req = urllib.request.Request(url, data=data_bytes, headers=headers, method=method.upper())
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read().decode("utf-8", errors="replace")
            try:
                parsed: Any = json.loads(raw)
            except json.JSONDecodeError:
                parsed = None
            return resp.status, raw, parsed, {k.lower(): v for k, v in resp.headers.items()}
    except urllib.error.HTTPError as e:
        raw = e.read().decode("utf-8", errors="replace")
        try:
            parsed = json.loads(raw)
        except json.JSONDecodeError:
            parsed = None
        return e.code, raw, parsed, {k.lower(): v for k, v in e.headers.items()}


def _check_expect(
    status: int,
    raw_body: str,
    parsed: Any,
    headers: Mapping[str, str],
    expect: Mapping[str, Any],
) -> Optional[str]:
    exp_status = expect.get("status")
    if exp_status is not None:
        allowed = exp_status if isinstance(exp_status, list) else [exp_status]
        if status not in allowed:
            return f"status {status} not in expected {allowed}"
    if "body_contains" in expect:
        needle = str(expect["body_contains"])
        if needle not in raw_body:
            return f"body does not contain substring {needle!r}"
    if "body_contains_all" in expect:
        needles = expect["body_contains_all"]
        if not isinstance(needles, list):
            return "body_contains_all must be a list"
        missing = [str(needle) for needle in needles if str(needle) not in raw_body]
        if missing:
            return f"body does not contain substring(s) {missing!r}"
    if "json_subset" in expect and parsed is not None and not _json_subset_match(parsed, expect["json_subset"]):
        return f"json_subset mismatch; got {parsed!r}"
    if "headers_present" in expect:
        names = expect["headers_present"]
        if not isinstance(names, list):
            return "headers_present must be a list"
        missing = [str(name).lower() for name in names if str(name).lower() not in headers]
        if missing:
            return f"missing response header(s) {missing!r}"
    if "headers_subset" in expect:
        wanted = expect["headers_subset"]
        if not isinstance(wanted, dict):
            return "headers_subset must be an object"
        mismatched = {
            str(name).lower(): value for name, value in wanted.items() if headers.get(str(name).lower()) != str(value)
        }
        if mismatched:
            return f"headers_subset mismatch; expected {mismatched!r}, got {headers!r}"
    return None


def _check_expect_any(
    status: int,
    raw_body: str,
    parsed: Any,
    headers: Mapping[str, str],
    alternatives: List[Mapping[str, Any]],
) -> Optional[str]:
    """Return None if any alternative passes; otherwise aggregate error message."""
    errs: List[str] = []
    for alt in alternatives:
        err = _check_expect(status, raw_body, parsed, headers, alt)
        if err is None:
            return None
        errs.append(f"({err})")
    return "none of expect_any matched: " + " ".join(errs)


def _run_skip_preflight(
    base: str,
    spec: Mapping[str, Any],
    timeout: float,
) -> bool:
    """Return True if the trace should be skipped (preflight `skip_when` matched)."""
    http = spec.get("http")
    if not isinstance(http, dict):
        return False
    when = spec.get("skip_when")
    if not isinstance(when, dict):
        return False
    method = str(http.get("method", "POST")).upper()
    path = str(http["path"])
    body = http.get("json")
    headers = http.get("headers")
    st, raw, parsed, _headers = _do_request(
        base,
        method,
        path,
        body,
        headers if isinstance(headers, dict) else None,
        timeout,
    )
    if st >= 400:
        print(f"skip_preflight: request failed status={st} body={raw[:500]!r}", file=sys.stderr)
        return True
    if "body_contains" in when:
        needle = str(when["body_contains"])
        if needle in raw:
            print(f"SKIP: skip_preflight matched raw body contains {needle!r}. Trace not applicable.")
            return True
    if "body_not_contains" in when:
        needle = str(when["body_not_contains"])
        if needle not in raw:
            print(f"SKIP: skip_preflight matched raw body missing {needle!r}. Trace not applicable.")
            return True
    ptr = str(when.get("json_pointer", "/total"))
    val = _get_by_pointer(parsed, ptr)
    if when.get("equals") is not None and val == when["equals"]:
        print(f"SKIP: skip_preflight matched ({ptr} == {val}). Trace not applicable.")
        return True
    if when.get("less_than") is not None:
        try:
            thr = int(when["less_than"])
            n = int(val)  # type: ignore[arg-type]
        except (TypeError, ValueError):
            pass
        else:
            if n < thr:
                print(f"SKIP: skip_preflight matched ({ptr} == {n} < less_than={thr}). Trace not applicable.")
                return True
    return False


def run_trace(base_url: str, trace_path: str, dry_run: bool, verbose: bool) -> int:
    """Execute one trace file; return process exit code (0 or 1)."""
    timeout = float(__import__("os").environ.get("VRS_HTTP_TIMEOUT_SECS", "120"))
    captures: Dict[str, str] = {}

    with Path(trace_path).open(encoding="utf-8") as fh:
        lines = [ln.strip() for ln in fh if ln.strip() and not ln.strip().startswith("#")]

    records: List[Any] = []
    header: Optional[Dict[str, Any]] = None
    for raw_line in lines:
        rec = json.loads(raw_line)
        if isinstance(rec, dict) and "_vrs" in rec:
            header = rec
        else:
            records.append(rec)

    sp = None
    if header and isinstance(header.get("_vrs"), dict):
        sp = header["_vrs"].get("skip_preflight")
    if isinstance(sp, dict) and not dry_run and _run_skip_preflight(base_url, sp, timeout):
        return 0

    for idx, step in enumerate(records, start=1):
        if not isinstance(step, dict):
            print(f"Step {idx}: invalid record (not an object)", file=sys.stderr)
            return 1
        sid = step.get("id", str(idx))
        sleep_ms = step.get("sleep_ms")
        if sleep_ms is not None:
            try:
                delay_ms = int(sleep_ms)
            except (TypeError, ValueError):
                print(f"FAIL step {sid}: sleep_ms must be an integer", file=sys.stderr)
                return 1
            if delay_ms < 0:
                print(f"FAIL step {sid}: sleep_ms must be >= 0", file=sys.stderr)
                return 1
            if dry_run:
                print(f"[dry-run] sleep {delay_ms}ms")
            else:
                if verbose:
                    print(f"--- step {sid} sleep {delay_ms}ms")
                time.sleep(delay_ms / 1000)
            continue

        http = step.get("http")
        if not isinstance(http, dict):
            print(f"Step {sid}: missing 'http'", file=sys.stderr)
            return 1
        method = str(http.get("method", "GET")).upper()
        path = str(http["path"])
        body = http.get("json")
        headers = http.get("headers")
        body = _substitute_captures(body, captures) if body is not None else None
        headers = _substitute_captures(headers, captures) if isinstance(headers, dict) else None

        if dry_run:
            suffix = json.dumps(body) if body else ""
            header_suffix = f" headers={json.dumps(headers)}" if headers else ""
            print(f"[dry-run] {method} {path}{header_suffix} {suffix}")
            continue

        st, raw, parsed, response_headers = _do_request(base_url, method, path, body, headers, timeout)
        if verbose:
            print(f"--- step {sid} {method} {path} -> {st}")

        expect = step.get("expect") or {}
        expect_any = step.get("expect_any")
        if expect_any:
            if not isinstance(expect_any, list):
                print(f"FAIL step {sid}: expect_any must be a list", file=sys.stderr)
                return 1
            err = _check_expect_any(st, raw, parsed, response_headers, expect_any)
        else:
            err = _check_expect(st, raw, parsed, response_headers, expect)
        if err:
            print(f"FAIL step {sid}: {err}", file=sys.stderr)
            print(f"  status={st} body={raw[:2000]!r}", file=sys.stderr)
            return 1

        cap = step.get("capture")
        if isinstance(cap, dict):
            ptr = str(cap.get("json_pointer", ""))
            key = str(cap.get("as", ""))
            if not key:
                print(f"FAIL step {sid}: capture missing 'as'", file=sys.stderr)
                return 1
            got = _get_by_pointer(parsed, ptr)
            if got is None:
                print(f"FAIL step {sid}: capture pointer {ptr!r} resolved to None", file=sys.stderr)
                return 1
            captures[key] = str(got)
            if verbose:
                print(f"  capture {key} = {captures[key]!r}")

    if dry_run:
        return 0
    print(f"OK: trace {trace_path!r} passed ({len(records)} steps).")
    return 0


def main(argv: Optional[List[str]] = None) -> int:
    """CLI entry point."""
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--base-url", required=True, help="Gateway or per-DCC REST root (e.g. http://127.0.0.1:9765)")
    p.add_argument("--trace", required=True, help="Path to a .jsonl trace file")
    p.add_argument("--dry-run", action="store_true", help="Print steps without sending HTTP")
    p.add_argument("-v", "--verbose", action="store_true", help="Per-step logging")
    args = p.parse_args(argv)
    try:
        return run_trace(args.base_url, args.trace, args.dry_run, args.verbose)
    except FileNotFoundError:
        print(f"Trace file not found: {args.trace!r}", file=sys.stderr)
        return 1
    except json.JSONDecodeError as e:
        print(f"Invalid JSON in trace: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
