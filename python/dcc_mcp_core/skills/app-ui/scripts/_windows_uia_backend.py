"""Windows UI Automation backend for the bundled app_ui skill.

The backend is intentionally optional and Windows-only. It uses PowerShell's
standard UIAutomationClient assembly instead of adding a Python dependency.
"""

from __future__ import annotations

from contextlib import suppress
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import time
from typing import Any
from typing import Dict
from typing import Iterable
from typing import List
from typing import Optional

from dcc_mcp_core.adapter_contracts import AppUiAuditRecord
from dcc_mcp_core.adapter_contracts import AppUiPolicy
from dcc_mcp_core.adapter_contracts import UiActionKind
from dcc_mcp_core.adapter_contracts import UiActionResult
from dcc_mcp_core.adapter_contracts import UiBounds
from dcc_mcp_core.adapter_contracts import UiControlNode
from dcc_mcp_core.adapter_contracts import UiErrorCode
from dcc_mcp_core.adapter_contracts import UiSnapshot
from dcc_mcp_core.adapter_contracts import UiWaitCondition
from dcc_mcp_core.adapter_contracts import UiWaitConditionKind
from dcc_mcp_core.adapter_contracts import UiWaitResult
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success


def _key_set(value: str) -> frozenset:
    return frozenset(value.split())


_POLICY_KEYS = _key_set(
    "allow_snapshot allow_find allow_mutating_actions allow_text_entry allow_keyboard_shortcuts "
    "allow_raw_coordinates allowed_window_titles allowed_process_ids audit_sensitive_values"
)
_CONDITION_KEYS = _key_set("kind control_id query role label text value checked timeout_ms interval_ms")

_UIA_SCRIPT = r"""
$ErrorActionPreference = "Stop"
$rawInput = [Console]::In.ReadToEnd()
if ([string]::IsNullOrWhiteSpace($rawInput)) {
  $payload = @{}
} else {
  $payload = $rawInput | ConvertFrom-Json
}

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
# DCC_MCP_UIA_HELPERS

$ChildScope = [System.Windows.Automation.TreeScope]::Children
$TrueCondition = [System.Windows.Automation.Condition]::TrueCondition

function As-Array($value) {
  if ($null -eq $value) { return @() }
  if ($value -is [System.Array]) { return $value }
  return @($value)
}

function Runtime-Id([System.Windows.Automation.AutomationElement]$element) {
  try {
    $ids = $element.GetRuntimeId()
    if ($null -eq $ids) { return "" }
    return (($ids | ForEach-Object { [string]$_ }) -join ".")
  } catch {
    return ""
  }
}

function Bounds-Object($rect) {
  if ($null -eq $rect -or $rect.IsEmpty) { return $null }
  return [ordered]@{
    x = [double]$rect.X
    y = [double]$rect.Y
    width = [double]$rect.Width
    height = [double]$rect.Height
  }
}

function Pattern-Value([System.Windows.Automation.AutomationElement]$element) {
  try {
    $pattern = $null
    if ($element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
      return $pattern.Current.Value
    }
  } catch {}
  return $null
}

function Pattern-Checked([System.Windows.Automation.AutomationElement]$element) {
  try {
    $pattern = $null
    if ($element.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) {
      return ([string]$pattern.Current.ToggleState) -eq "On"
    }
  } catch {}
  return $null
}

function Element-Raw([System.Windows.Automation.AutomationElement]$element, [int]$depth, [string]$path) {
  $script:nodeCount = $script:nodeCount + 1
  $current = $element.Current
  $runtimeId = Runtime-Id $element
  $children = @()
  if ($depth -lt $payload.max_depth -and $script:nodeCount -lt $payload.max_nodes) {
    try {
      $items = $element.FindAll($ChildScope, $TrueCondition)
      for ($i = 0; $i -lt $items.Count; $i++) {
        if ($script:nodeCount -ge $payload.max_nodes) { break }
        $children += Element-Raw $items.Item($i) ($depth + 1) "$path.$i"
      }
    } catch {}
  }
  return [ordered]@{
    runtime_id = $runtimeId
    fallback_path = $path
    name = $current.Name
    automation_id = $current.AutomationId
    class_name = $current.ClassName
    control_type = $current.ControlType.ProgrammaticName
    process_id = [int]$current.ProcessId
    native_window_handle = [int]$current.NativeWindowHandle
    enabled = [bool]$current.IsEnabled
    offscreen = [bool]$current.IsOffscreen
    focused = [bool]$current.HasKeyboardFocus
    bounds = Bounds-Object $current.BoundingRectangle
    value = Pattern-Value $element
    checked = Pattern-Checked $element
    children = $children
  }
}

function Candidate-Windows() {
  $root = [System.Windows.Automation.AutomationElement]::RootElement
  return $root.FindAll($ChildScope, $TrueCondition)
}

function Scope-Process-Ids() {
  $ids = @()
  foreach ($processId in As-Array $payload.scope.process_ids) {
    if ($processId -ne $null -and [int]$processId -gt 0) { $ids += [int]$processId }
  }
  foreach ($name in As-Array $payload.scope.process_names) {
    if (-not [string]::IsNullOrWhiteSpace([string]$name)) {
      try {
        foreach ($proc in Get-Process -Name ([string]$name) -ErrorAction SilentlyContinue) {
          $ids += [int]$proc.Id
        }
      } catch {}
    }
  }
  return $ids
}

function Match-Scope($element, $processIds) {
  $titles = As-Array $payload.scope.window_titles
  if ($processIds.Count -gt 0 -and $processIds -notcontains [int]$element.Current.ProcessId) {
    return $false
  }
  if ($titles.Count -gt 0) {
    $name = [string]$element.Current.Name
    foreach ($title in $titles) {
      if ($name.IndexOf([string]$title, [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
        return $true
      }
    }
    return $false
  }
  return $true
}

function Find-Scoped-Root() {
  $processIds = Scope-Process-Ids
  $titles = As-Array $payload.scope.window_titles
  if ($titles.Count -eq 0) {
    $processRoot = Find-Process-Root $processIds
    if ($null -ne $processRoot) { return $processRoot }
  }
  $windows = Candidate-Windows
  for ($i = 0; $i -lt $windows.Count; $i++) {
    $candidate = $windows.Item($i)
    if (Match-Scope $candidate $processIds) {
      return $candidate
    }
  }
  return $null
}

function Find-By-Id($element, [string]$controlId, [int]$depth, [string]$path) {
  if ($null -eq $element) { return $null }
  $runtimeId = Runtime-Id $element
  $candidateId = if ([string]::IsNullOrWhiteSpace($runtimeId)) { "uia:path:$path" } else { "uia:$runtimeId" }
  if ($candidateId -eq $controlId) { return $element }
  if ($depth -ge $payload.max_depth) { return $null }
  try {
    $items = $element.FindAll($ChildScope, $TrueCondition)
    for ($i = 0; $i -lt $items.Count; $i++) {
      $found = Find-By-Id $items.Item($i) $controlId ($depth + 1) "$path.$i"
      if ($null -ne $found) { return $found }
    }
  } catch {}
  return $null
}

function Invoke-Action($element) {
  $action = [string]$payload.action.action
  if ($action -eq "focus") {
    $element.SetFocus()
    return @{ok = $true; message = "focused control"}
  }
  if ($action -eq "set_text") {
    $pattern = $null
    if ($element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
      $pattern.SetValue([string]$payload.action.text)
      return @{ok = $true; message = "set text"}
    }
    return @{ok = $false; error = "unsupported_action"; message = "set_text requires ValuePattern"}
  }
  if ($action -eq "toggle" -or $action -eq "set_checked") {
    $pattern = $null
    if (-not $element.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) {
      return @{ok = $false; error = "unsupported_action"; message = "$action requires TogglePattern"}
    }
    if ($action -eq "toggle") {
      $pattern.Toggle()
      return @{ok = $true; message = "toggled control"}
    }
    $desired = [bool]$payload.action.checked
    $current = ([string]$pattern.Current.ToggleState) -eq "On"
    if ($current -ne $desired) { $pattern.Toggle() }
    return @{ok = $true; message = "set checked state"}
  }
  if ($action -eq "click") {
    $pattern = $null
    if ($element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
      try {
        $pattern.Invoke()
        return @{ok = $true; message = "invoked control"}
      } catch {
        if (Invoke-NativeButtonClick $element) {
          return @{ok = $true; message = "invoked native button"}
        }
      }
    }
    if ($element.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) {
      $pattern.Toggle()
      return @{ok = $true; message = "toggled control"}
    }
    if (Invoke-NativeButtonClick $element) {
      return @{ok = $true; message = "invoked native button"}
    }
    try {
      $element.SetFocus()
      return @{ok = $true; message = "focused control because InvokePattern is unavailable"}
    } catch {
      return @{ok = $false; error = "unsupported_action"; message = "click requires InvokePattern or TogglePattern"}
    }
  }
  return @{ok = $false; error = "unsupported_action"; message = "unsupported Windows UIA action"}
}

try {
  $root = Find-Scoped-Root
  if ($null -eq $root) {
    @{ok = $false; error = "missing_window"; message = "No scoped Windows UIA window matched the supplied policy."} |
      ConvertTo-Json -Depth 64 -Compress
    exit 0
  }
  $script:nodeCount = 0
  if ($payload.mode -eq "act") {
    $target = Find-By-Id $root ([string]$payload.action.control_id) 0 "0"
    if ($null -eq $target) {
      @{ok = $false; error = "not_found"; message = "Control not found in scoped Windows UIA window."} |
        ConvertTo-Json -Depth 64 -Compress
      exit 0
    }
    $beforeFocus = Runtime-Id ([System.Windows.Automation.AutomationElement]::FocusedElement)
    $actionResult = Invoke-Action $target
    $afterFocus = Runtime-Id ([System.Windows.Automation.AutomationElement]::FocusedElement)
    @{
      ok = [bool]$actionResult.ok
      error = $actionResult.error
      message = $actionResult.message
      before_focus_runtime_id = $beforeFocus
      after_focus_runtime_id = $afterFocus
      control = Element-Raw $target 0 "target"
    } | ConvertTo-Json -Depth 64 -Compress
    exit 0
  }
  @{
    ok = $true
    root = Element-Raw $root 0 "0"
    focus_runtime_id = Runtime-Id ([System.Windows.Automation.AutomationElement]::FocusedElement)
    node_count = $script:nodeCount
  } | ConvertTo-Json -Depth 64 -Compress
} catch {
  @{ok = $false; error = "backend_error"; message = $_.Exception.Message} |
    ConvertTo-Json -Depth 64 -Compress
}
"""

_UIA_HELPERS = Path(__file__).with_name("_windows_uia_helpers.ps1").read_text(encoding="utf-8")
_UIA_SCRIPT = _UIA_SCRIPT.replace("# DCC_MCP_UIA_HELPERS", _UIA_HELPERS)


def _read_params() -> Dict[str, Any]:
    raw = ""
    try:
        if not sys.stdin.isatty():
            raw = sys.stdin.read()
    except Exception:
        raw = ""
    if raw.strip():
        try:
            parsed = json.loads(raw)
            return parsed if isinstance(parsed, dict) else {}
        except json.JSONDecodeError:
            return {}
    return {}


def _safe_session_id(session_id: Any) -> str:
    text = str(session_id or "default")
    cleaned = "".join(ch if ch.isalnum() or ch in "_.-" else "_" for ch in text)
    return cleaned[:80] or "default"


def _state_dir() -> Path:
    root = os.environ.get("DCC_MCP_APP_UI_UIA_STATE_DIR")
    path = Path(root) if root else Path(tempfile.gettempdir()) / "dcc-mcp-app-ui-uia"
    path.mkdir(parents=True, exist_ok=True)
    return path


def _state_path(session_id: str) -> Path:
    return _state_dir() / f"{_safe_session_id(session_id)}.json"


def _load_state(session_id: str) -> Dict[str, Any]:
    path = _state_path(session_id)
    if not path.exists():
        return {"session_id": session_id, "revision": 0, "last_snapshot_id": ""}
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        data = {}
    state = {"session_id": session_id, "revision": 0, "last_snapshot_id": ""}
    if isinstance(data, dict):
        state.update(data)
    return state


def _save_state(state: Dict[str, Any]) -> None:
    path = _state_path(str(state.get("session_id") or "default"))
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(state, sort_keys=True), encoding="utf-8")
    tmp.replace(path)


def _policy_from_params(params: Dict[str, Any]) -> AppUiPolicy:
    raw = params.get("policy") or {}
    if not isinstance(raw, dict):
        raw = {}
    return AppUiPolicy(**{key: raw[key] for key in _POLICY_KEYS if key in raw})


def _scope_from_params(params: Dict[str, Any], policy: AppUiPolicy) -> Dict[str, Any]:
    titles = list(policy.allowed_window_titles)
    if params.get("window_title"):
        titles.append(str(params["window_title"]))
    if os.environ.get("DCC_MCP_APP_UI_UIA_WINDOW_TITLE"):
        titles.append(str(os.environ["DCC_MCP_APP_UI_UIA_WINDOW_TITLE"]))

    process_ids = list(policy.allowed_process_ids)
    raw_pid = params.get("process_id") or os.environ.get("DCC_MCP_APP_UI_UIA_PROCESS_ID")
    if raw_pid:
        with suppress(TypeError, ValueError):
            process_ids.append(int(raw_pid))

    process_names = []
    if params.get("process_name"):
        process_names.append(str(params["process_name"]))
    if os.environ.get("DCC_MCP_APP_UI_UIA_PROCESS_NAME"):
        process_names.append(str(os.environ["DCC_MCP_APP_UI_UIA_PROCESS_NAME"]))

    return {
        "window_titles": [item for item in titles if str(item).strip()],
        "process_ids": [item for item in process_ids if int(item) > 0],
        "process_names": [item for item in process_names if str(item).strip()],
    }


def _scope_is_explicit(scope: Dict[str, Any]) -> bool:
    return bool(scope["window_titles"] or scope["process_ids"] or scope["process_names"])


def _snapshot_id(state: Dict[str, Any]) -> str:
    return f"{state['session_id']}:{int(state.get('revision') or 0)}"


def _powershell_bin() -> Optional[str]:
    return (
        shutil.which("powershell.exe") or shutil.which("pwsh.exe") or shutil.which("powershell") or shutil.which("pwsh")
    )


def _backend_unavailable(message: str) -> Dict[str, Any]:
    return skill_error(
        message,
        "backend_unavailable",
        backend="windows-uia",
        setup_instructions=(
            "Run on Windows with PowerShell and the UIAutomationClient assembly available. "
            "Scope the backend with policy.allowed_window_titles, policy.allowed_process_ids, "
            "DCC_MCP_APP_UI_UIA_WINDOW_TITLE, DCC_MCP_APP_UI_UIA_PROCESS_ID, or "
            "DCC_MCP_APP_UI_UIA_PROCESS_NAME."
        ),
    )


def _run_uia(payload: Dict[str, Any]) -> Dict[str, Any]:
    if os.name != "nt":
        raise RuntimeError("Windows UIA backend is only available on Windows")
    ps = _powershell_bin()
    if not ps:
        raise RuntimeError("PowerShell executable not found for Windows UIA backend")
    with tempfile.NamedTemporaryFile("w", suffix=".ps1", delete=False, encoding="utf-8") as handle:
        handle.write(_UIA_SCRIPT)
        script_path = handle.name
    try:
        completed = subprocess.run(
            [ps, "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", script_path],
            input=json.dumps(payload),
            capture_output=True,
            text=True,
            timeout=float(os.environ.get("DCC_MCP_APP_UI_UIA_TIMEOUT_SECS", "12")),
        )
    finally:
        with suppress(OSError):
            Path(script_path).unlink()
    if completed.returncode != 0:
        raise RuntimeError((completed.stderr or completed.stdout or "Windows UIA command failed").strip())
    try:
        parsed = json.loads(completed.stdout or "{}")
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"Windows UIA command returned invalid JSON: {exc}") from exc
    return parsed if isinstance(parsed, dict) else {}


def _role_from_control_type(control_type: Any) -> str:
    name = str(control_type or "").split(".")[-1].lower()
    return {
        "button": "button",
        "calendar": "calendar",
        "checkbox": "checkbox",
        "combobox": "combo_box",
        "custom": "custom",
        "dataitem": "row",
        "edit": "text_field",
        "group": "group",
        "header": "header",
        "hyperlink": "link",
        "image": "image",
        "list": "list",
        "listitem": "list_item",
        "menu": "menu",
        "menuitem": "menu_item",
        "pane": "pane",
        "progressbar": "progress_bar",
        "radiobutton": "radio_button",
        "scrollbar": "scroll_bar",
        "slider": "slider",
        "splitbutton": "button",
        "tab": "tab",
        "tabitem": "tab_item",
        "text": "label",
        "thumb": "thumb",
        "titlebar": "title_bar",
        "toolbar": "tool_bar",
        "tree": "tree",
        "treeitem": "tree_item",
        "window": "window",
    }.get(name, name or "control")


def _bounds_from_raw(raw: Dict[str, Any]) -> Optional[UiBounds]:
    bounds = raw.get("bounds")
    if not isinstance(bounds, dict):
        return None
    try:
        return UiBounds(
            x=float(bounds.get("x") or 0),
            y=float(bounds.get("y") or 0),
            width=float(bounds.get("width") or 0),
            height=float(bounds.get("height") or 0),
        )
    except (TypeError, ValueError):
        return None


def _control_id(raw: Dict[str, Any]) -> str:
    runtime_id = str(raw.get("runtime_id") or "").strip()
    if runtime_id:
        return f"uia:{runtime_id}"
    return f"uia:path:{raw.get('fallback_path') or '0'}"


def _node_from_uia_dict(raw: Dict[str, Any], snapshot_id: str) -> UiControlNode:
    children = [
        _node_from_uia_dict(child, snapshot_id) for child in raw.get("children", []) or [] if isinstance(child, dict)
    ]
    runtime_id = str(raw.get("runtime_id") or "")
    metadata = {
        "app_ui": {
            "backend": "windows-uia",
            "snapshot_id": snapshot_id,
            "runtime_id": runtime_id,
            "fallback_path": raw.get("fallback_path"),
            "process_id": raw.get("process_id"),
            "class_name": raw.get("class_name"),
            "native_window_handle": raw.get("native_window_handle"),
            "control_type": raw.get("control_type"),
        }
    }
    value = raw.get("value")
    checked = raw.get("checked")
    name = str(raw.get("name") or "")
    role = _role_from_control_type(raw.get("control_type"))
    text = name if role == "label" else None
    return UiControlNode(
        id=_control_id(raw),
        role=role,
        label=name or None,
        text=text,
        object_name=str(raw.get("automation_id") or "") or None,
        enabled=bool(raw.get("enabled", True)),
        visible=not bool(raw.get("offscreen", False)),
        bounds=_bounds_from_raw(raw),
        value=str(value) if value is not None else None,
        checked=bool(checked) if checked is not None else None,
        children=children,
        metadata=metadata,
    )


def _iter_nodes(node: Dict[str, Any]) -> Iterable[Dict[str, Any]]:
    yield node
    for child in node.get("children", []) or []:
        if isinstance(child, dict):
            yield from _iter_nodes(child)


def _find_by_id(snapshot: Dict[str, Any], control_id: str) -> Optional[Dict[str, Any]]:
    for node in _iter_nodes(snapshot["root"]):
        if node.get("id") == control_id:
            return node
    return None


def _find_controls(snapshot: Dict[str, Any], params: Dict[str, Any]) -> List[Dict[str, Any]]:
    query = str(params.get("query") or "").lower()
    role = str(params.get("role") or "").lower()
    label = str(params.get("label") or "").lower()
    object_name = str(params.get("object_name") or "").lower()
    limit = int(params.get("limit") or 10)
    matches = []
    for node in _iter_nodes(snapshot["root"]):
        if role and str(node.get("role") or "").lower() != role:
            continue
        if label and label not in str(node.get("label") or "").lower():
            continue
        if object_name and object_name not in str(node.get("object_name") or "").lower():
            continue
        if query:
            haystack = " ".join(
                str(node.get(key) or "") for key in ("id", "label", "text", "value", "object_name", "role")
            ).lower()
            if query not in haystack:
                continue
        matches.append(node)
        if len(matches) >= limit:
            break
    return matches


def _capture_snapshot(
    session_id: str,
    policy: AppUiPolicy,
    params: Dict[str, Any],
    *,
    bump_revision: bool,
) -> Dict[str, Any]:
    scope = _scope_from_params(params, policy)
    if not _scope_is_explicit(scope):
        return {
            "success": False,
            "error": UiErrorCode.MISSING_WINDOW,
            "message": (
                "Windows UIA backend requires an explicit scoped window title, "
                "process id, or process name; whole-desktop snapshots are disabled."
            ),
        }
    state = _load_state(session_id)
    if bump_revision:
        state["revision"] = int(state.get("revision") or 0) + 1
    snapshot_id = _snapshot_id(state)
    payload = {
        "mode": "snapshot",
        "scope": scope,
        "max_depth": int(os.environ.get("DCC_MCP_APP_UI_UIA_MAX_DEPTH", "5")),
        "max_nodes": int(os.environ.get("DCC_MCP_APP_UI_UIA_MAX_NODES", "250")),
    }
    try:
        raw = _run_uia(payload)
    except RuntimeError as exc:
        return {"success": False, "error": "backend_unavailable", "message": str(exc)}
    if not raw.get("ok"):
        return {
            "success": False,
            "error": str(raw.get("error") or UiErrorCode.BACKEND_ERROR),
            "message": str(raw.get("message") or "Windows UIA snapshot failed."),
        }
    root = _node_from_uia_dict(raw["root"], snapshot_id)
    focus_runtime_id = str(raw.get("focus_runtime_id") or "")
    snapshot = UiSnapshot(
        root=root,
        session_id=session_id,
        focus_id=f"uia:{focus_runtime_id}" if focus_runtime_id else None,
        truncated=int(raw.get("node_count") or 0) >= payload["max_nodes"],
        node_count=int(raw.get("node_count") or 1),
        metadata={
            "snapshot_id": snapshot_id,
            "app_ui": {
                "backend": "windows-uia",
                "scope": scope,
                "max_depth": payload["max_depth"],
                "max_nodes": payload["max_nodes"],
            },
        },
    ).to_dict()
    state["last_snapshot_id"] = snapshot_id
    _save_state(state)
    return {"success": True, "snapshot": snapshot, "snapshot_id": snapshot_id, "scope": scope}


def _error_from_capture(capture: Dict[str, Any]) -> Dict[str, Any]:
    error = str(capture.get("error") or UiErrorCode.BACKEND_ERROR)
    message = str(capture.get("message") or "Windows UIA backend failed.")
    if error == "backend_unavailable":
        return _backend_unavailable(message)
    return skill_error(message, error, error_code=error, backend="windows-uia")


def snapshot_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    if not policy.allow_snapshot:
        return skill_error(
            "app_ui snapshot disabled by policy",
            UiErrorCode.POLICY_DISABLED,
            error_code=UiErrorCode.POLICY_DISABLED,
        )
    capture = _capture_snapshot(session_id, policy, params, bump_revision=True)
    if not capture.get("success"):
        return _error_from_capture(capture)
    return skill_success(
        "Captured Windows UIA app_ui snapshot.",
        prompt="Use app_ui__find to resolve a control, then app_ui__act with the returned snapshot_id.",
        session_id=session_id,
        snapshot_id=capture["snapshot_id"],
        snapshot=capture["snapshot"],
        policy=policy.to_dict(),
    )


def find_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    if not policy.allow_find:
        return skill_error(
            "app_ui find disabled by policy",
            UiErrorCode.POLICY_DISABLED,
            error_code=UiErrorCode.POLICY_DISABLED,
        )
    capture = _capture_snapshot(session_id, policy, params, bump_revision=True)
    if not capture.get("success"):
        return _error_from_capture(capture)
    matches = _find_controls(capture["snapshot"], params)
    return skill_success(
        f"Found {len(matches)} Windows UIA app_ui control(s).",
        prompt="Use app_ui__act with a returned control id, then app_ui__wait_for.",
        session_id=session_id,
        snapshot_id=capture["snapshot_id"],
        matches=matches,
        count=len(matches),
    )


def _audit_record(
    action: str,
    success: bool,
    control: Optional[Dict[str, Any]],
    session_id: str,
    policy: AppUiPolicy,
    before_focus_id: Optional[str],
    after_focus_id: Optional[str],
    error_code: Optional[str] = None,
    message: Optional[str] = None,
) -> Dict[str, Any]:
    redacted = []
    if action == UiActionKind.SET_TEXT and not policy.audit_sensitive_values:
        redacted.append("text")
    return AppUiAuditRecord(
        action_kind=action,
        success=success,
        target_control_id=control.get("id") if control else None,
        target_role=control.get("role") if control else None,
        target_label=control.get("label") if control else None,
        before_focus_id=before_focus_id,
        after_focus_id=after_focus_id,
        error_code=error_code,
        message=message,
        session_id=session_id,
        redacted_fields=redacted,
        metadata={"backend": "windows-uia"},
    ).to_dict()


def _stale_result(control_id: str, session_id: str, requested: str, current: str) -> Dict[str, Any]:
    result = UiActionResult.stale(control_id).to_dict()
    result["metadata"] = {
        "requested_snapshot_id": requested,
        "current_snapshot_id": current,
    }
    audit = AppUiAuditRecord(
        action_kind="unknown",
        success=False,
        target_control_id=control_id,
        error_code=UiErrorCode.STALE_CONTROL,
        message="control is stale; refresh the UI snapshot",
        session_id=session_id,
        metadata=result["metadata"],
    ).to_dict()
    return skill_error(
        "Control is stale; refresh the app_ui snapshot.",
        UiErrorCode.STALE_CONTROL,
        result=result,
        audit=audit,
        current_snapshot_id=current,
    )


def act_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    action = str(params.get("action") or "")
    control_id = str(params.get("control_id") or "")
    requested_snapshot_id = str(params.get("snapshot_id") or "")
    state = _load_state(session_id)
    current_snapshot_id = str(state.get("last_snapshot_id") or "")
    if requested_snapshot_id and requested_snapshot_id != current_snapshot_id:
        return _stale_result(control_id, session_id, requested_snapshot_id, current_snapshot_id)
    if not policy.allows_action(action):
        result = UiActionResult(
            success=False,
            control_id=control_id,
            error_code=UiErrorCode.POLICY_DISABLED,
            message=f"app_ui action {action!r} disabled by policy",
        ).to_dict()
        audit = _audit_record(action, False, None, session_id, policy, None, None, UiErrorCode.POLICY_DISABLED)
        return skill_error(result["message"], UiErrorCode.POLICY_DISABLED, result=result, audit=audit)
    if action in (UiActionKind.RAW_COORDINATE_CLICK, UiActionKind.KEYBOARD_SHORTCUT):
        result = UiActionResult(
            success=False,
            control_id=control_id,
            error_code=UiErrorCode.UNSUPPORTED_ACTION,
            message="Windows UIA backend does not expose raw coordinates or keyboard shortcuts.",
        ).to_dict()
        audit = _audit_record(action, False, None, session_id, policy, None, None, UiErrorCode.UNSUPPORTED_ACTION)
        return skill_error(result["message"], UiErrorCode.UNSUPPORTED_ACTION, result=result, audit=audit)

    capture = _capture_snapshot(session_id, policy, params, bump_revision=False)
    if not capture.get("success"):
        return _error_from_capture(capture)
    control = _find_by_id(capture["snapshot"], control_id)
    if not control:
        result = UiActionResult(
            success=False,
            control_id=control_id,
            error_code=UiErrorCode.NOT_FOUND,
            message="control not found in scoped Windows UIA window",
        ).to_dict()
        return skill_error("Control not found in scoped Windows UIA window.", UiErrorCode.NOT_FOUND, result=result)

    payload = {
        "mode": "act",
        "scope": capture["scope"],
        "max_depth": int(os.environ.get("DCC_MCP_APP_UI_UIA_MAX_DEPTH", "5")),
        "max_nodes": int(os.environ.get("DCC_MCP_APP_UI_UIA_MAX_NODES", "250")),
        "action": {
            "control_id": control_id,
            "action": action,
            "text": params.get("text") or "",
            "checked": bool(params.get("checked")),
        },
    }
    try:
        raw = _run_uia(payload)
    except RuntimeError as exc:
        return _backend_unavailable(str(exc))

    before_focus = f"uia:{raw.get('before_focus_runtime_id')}" if raw.get("before_focus_runtime_id") else None
    after_focus = f"uia:{raw.get('after_focus_runtime_id')}" if raw.get("after_focus_runtime_id") else None
    if not raw.get("ok"):
        error = str(raw.get("error") or UiErrorCode.BACKEND_ERROR)
        message = str(raw.get("message") or "Windows UIA action failed.")
        result = UiActionResult(
            success=False,
            control_id=control_id,
            error_code=error,
            message=message,
            before_focus_id=before_focus,
            after_focus_id=after_focus,
        ).to_dict()
        audit = _audit_record(action, False, control, session_id, policy, before_focus, after_focus, error, message)
        return skill_error(message, error, result=result, audit=audit)

    state["revision"] = int(state.get("revision") or 0) + 1
    state["last_snapshot_id"] = _snapshot_id(state)
    _save_state(state)
    message = str(raw.get("message") or "Windows UIA action completed")
    result = UiActionResult(
        success=True,
        control_id=control_id,
        message=message,
        before_focus_id=before_focus,
        after_focus_id=after_focus,
        metadata={"snapshot_id": state["last_snapshot_id"]},
    ).to_dict()
    audit = _audit_record(action, True, control, session_id, policy, before_focus, after_focus, None, message)
    return skill_success(
        f"Completed Windows UIA action {action!r} on {control_id}.",
        prompt="Use app_ui__wait_for to poll for the expected UI state, then app_ui__snapshot to verify.",
        session_id=session_id,
        snapshot_id=state["last_snapshot_id"],
        result=result,
        audit=audit,
    )


def _condition_from_params(raw: Dict[str, Any]) -> UiWaitCondition:
    data = {key: raw[key] for key in _CONDITION_KEYS if key in raw}
    data.setdefault("kind", UiWaitConditionKind.CONTROL_EXISTS)
    return UiWaitCondition(**data)


def _resolve_condition_control(snapshot: Dict[str, Any], condition: UiWaitCondition) -> Optional[Dict[str, Any]]:
    if condition.control_id:
        return _find_by_id(snapshot, condition.control_id)
    matches = _find_controls(snapshot, condition.to_dict())
    return matches[0] if matches else None


def _condition_matches(snapshot: Dict[str, Any], condition: UiWaitCondition) -> bool:
    control = _resolve_condition_control(snapshot, condition)
    if condition.kind == UiWaitConditionKind.CONTROL_MISSING:
        return control is None
    if control is None:
        return False
    if condition.kind == UiWaitConditionKind.CONTROL_EXISTS:
        return True
    if condition.kind == UiWaitConditionKind.TEXT_EQUALS:
        return str(control.get("text") or "") == str(condition.text or "")
    if condition.kind == UiWaitConditionKind.VALUE_EQUALS:
        return str(control.get("value") or "") == str(condition.value or "")
    if condition.kind == UiWaitConditionKind.CHECKED_EQUALS:
        return bool(control.get("checked")) is bool(condition.checked)
    if condition.kind == UiWaitConditionKind.ENABLED:
        return bool(control.get("enabled"))
    if condition.kind == UiWaitConditionKind.DISABLED:
        return not bool(control.get("enabled"))
    if condition.kind == UiWaitConditionKind.FOCUSED:
        return snapshot.get("focus_id") == control.get("id")
    return False


def wait_for_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    condition = _condition_from_params(params.get("condition") or {})
    timeout_ms = max(0, int(condition.timeout_ms))
    interval_ms = max(10, int(condition.interval_ms))
    deadline = time.monotonic() + (timeout_ms / 1000.0)
    attempts = 0
    last_snapshot = None
    start = time.monotonic()
    while True:
        capture = _capture_snapshot(session_id, policy, params, bump_revision=True)
        attempts += 1
        if not capture.get("success"):
            return _error_from_capture(capture)
        last_snapshot = capture["snapshot"]
        if _condition_matches(last_snapshot, condition):
            elapsed_ms = round((time.monotonic() - start) * 1000.0, 1)
            result = UiWaitResult(
                success=True,
                condition=condition,
                elapsed_ms=elapsed_ms,
                attempts=attempts,
                snapshot=UiSnapshot(
                    root=_node_from_dict(last_snapshot["root"]),
                    session_id=session_id,
                    focus_id=last_snapshot.get("focus_id"),
                    truncated=bool(last_snapshot.get("truncated")),
                    node_count=int(last_snapshot.get("node_count") or 1),
                    metadata=last_snapshot.get("metadata") or {},
                ),
                message="condition became true",
            ).to_dict()
            return skill_success(
                "app_ui wait condition satisfied.",
                session_id=session_id,
                snapshot_id=capture["snapshot_id"],
                result=result,
            )
        if time.monotonic() >= deadline:
            break
        time.sleep(min(interval_ms / 1000.0, max(0.0, deadline - time.monotonic())))

    elapsed_ms = round((time.monotonic() - start) * 1000.0, 1)
    result = UiWaitResult(
        success=False,
        condition=condition,
        elapsed_ms=elapsed_ms,
        attempts=attempts,
        snapshot=None,
        error_code=UiErrorCode.TIMEOUT,
        message="condition did not become true before timeout",
        metadata={"last_snapshot": last_snapshot},
    ).to_dict()
    return skill_error(
        "app_ui wait_for timed out.",
        UiErrorCode.TIMEOUT,
        session_id=session_id,
        result=result,
        attempts=attempts,
    )


def _node_from_dict(raw: Dict[str, Any]) -> UiControlNode:
    bounds = raw.get("bounds") or {}
    return UiControlNode(
        id=str(raw.get("id") or ""),
        role=str(raw.get("role") or "control"),
        label=raw.get("label"),
        text=raw.get("text"),
        object_name=raw.get("object_name"),
        enabled=bool(raw.get("enabled", True)),
        visible=bool(raw.get("visible", True)),
        bounds=UiBounds(
            x=float(bounds.get("x") or 0),
            y=float(bounds.get("y") or 0),
            width=float(bounds.get("width") or 0),
            height=float(bounds.get("height") or 0),
        )
        if bounds
        else None,
        value=raw.get("value"),
        checked=raw.get("checked"),
        children=[_node_from_dict(child) for child in raw.get("children", []) or []],
        metadata=raw.get("metadata") or {},
    )


def _dedent_for_tests() -> str:
    return textwrap.dedent(_UIA_SCRIPT).strip()
