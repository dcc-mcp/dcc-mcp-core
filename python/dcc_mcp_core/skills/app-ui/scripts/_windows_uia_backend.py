"""Windows UI Automation backend for the bundled app_ui skill.

The backend is intentionally optional and Windows-only. It uses PowerShell's
standard UIAutomationClient assembly instead of adding a Python dependency.
"""

from __future__ import annotations

import atexit
import base64
from contextlib import suppress
from functools import wraps
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import threading
import time
from typing import Any
from typing import Callable
from typing import Dict
from typing import Iterable
from typing import List
from typing import Optional
from typing import Set

from dcc_mcp_core.adapter_contracts import AppUiAuditRecord
from dcc_mcp_core.adapter_contracts import AppUiPolicy
from dcc_mcp_core.adapter_contracts import UiActionKind
from dcc_mcp_core.adapter_contracts import UiActionRequest
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

try:
    from dcc_mcp_core import ComputerUseSession as _ComputerUseSession
except (AttributeError, ImportError):
    _ComputerUseSession = None


def _key_set(value: str) -> frozenset:
    return frozenset(value.split())


_POLICY_KEYS = _key_set(
    "allow_snapshot allow_find allow_mutating_actions allow_text_entry allow_keyboard_shortcuts "
    "allow_raw_coordinates allowed_window_titles allowed_process_ids audit_sensitive_values"
)
_CONDITION_KEYS = _key_set("kind control_id query role label text value checked timeout_ms interval_ms")
_COMPUTER_USE_SESSIONS: Dict[str, Dict[str, Any]] = {}
_COMPUTER_USE_OBSERVATIONS: Dict[str, Dict[str, str]] = {}
_COMPUTER_USE_INTERRUPTED: Set[str] = set()
_COMPUTER_USE_STOPPING: Dict[str, int] = {}
_SESSION_STOP_GENERATIONS: Dict[str, int] = {}
_SESSION_LOCKS: Dict[str, threading.RLock] = {}
_SESSION_STOP_LOCKS: Dict[str, threading.Lock] = {}
_SESSION_LOCKS_GUARD = threading.Lock()
_CLEANUP_REQUESTED = threading.Event()
_MAX_DRAG_POINTS = 256
_MAX_KEY_TOKENS = 16
_MAX_TEXT_UTF16_UNITS = 4096
_DENIED_PROCESS_NAMES = frozenset(
    {
        "1password",
        "authhost",
        "bitwarden",
        "cmd",
        "conhost",
        "consent",
        "credentialuibroker",
        "dashlane",
        "enpass",
        "keeperpasswordmanager",
        "keepass",
        "keepassxc",
        "lastpass",
        "lockapp",
        "logonui",
        "nordpass",
        "openconsole",
        "powershell",
        "powershell_ise",
        "pwsh",
        "roboform",
        "sechealthui",
        "securityhealthhost",
        "systemsettings",
        "windowsterminal",
        "wt",
    }
)
_DESKTOP_UNAVAILABLE_MESSAGE = (
    "The Windows desktop is locked or disconnected. Unlock it before using app_ui; no UI input was attempted."
)

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
    if ([bool]$element.Current.IsPassword) { return $null }
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
  $explicitIds = @()
  foreach ($processId in As-Array $payload.scope.process_ids) {
    if ($processId -ne $null -and [int]$processId -gt 0) { $explicitIds += [int]$processId }
  }
  $names = As-Array $payload.scope.process_names
  $nameIds = @()
  foreach ($name in $names) {
    if (-not [string]::IsNullOrWhiteSpace([string]$name)) {
      try {
        foreach ($proc in Get-Process -Name ([string]$name) -ErrorAction SilentlyContinue) {
          $nameIds += [int]$proc.Id
        }
      } catch {}
    }
  }
  if ($names.Count -gt 0) {
    if ($explicitIds.Count -gt 0) {
      return @($explicitIds | Where-Object { $nameIds -contains $_ } | Select-Object -Unique)
    }
    return @($nameIds | Select-Object -Unique)
  }
  return @($explicitIds | Select-Object -Unique)
}

function Match-Scope($element, $processIds) {
  $titles = As-Array $payload.scope.window_titles
  $excludedProcessIds = As-Array $payload.scope.excluded_process_ids
  if ($excludedProcessIds -contains [int]$element.Current.ProcessId) {
    return $false
  }
  if ($payload.scope.require_process_match -and $processIds.Count -eq 0) {
    return $false
  }
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
  $requestedHandles = As-Array $payload.scope.window_handles
  $handleMatches = @()
  foreach ($windowHandle in $requestedHandles) {
    try {
      $windowRoot = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]([long]$windowHandle))
      if ($null -ne $windowRoot -and (Match-Scope $windowRoot $processIds)) { $handleMatches += $windowRoot }
    } catch {}
  }
  if ($handleMatches.Count -eq 1) { return $handleMatches[0] }
  if ($handleMatches.Count -gt 1) {
    $script:scopeError = "Multiple scoped HWND values matched; provide one unique target."
    return $null
  }
  if ($requestedHandles.Count -gt 0) {
    $script:scopeError = "The explicit HWND did not satisfy every PID, process-name, and title constraint."
    return $null
  }
  $titles = As-Array $payload.scope.window_titles
  if ($titles.Count -eq 0) {
    $processRoot = Find-Process-Root $processIds
    if ($null -ne $processRoot -and (Match-Scope $processRoot $processIds)) { return $processRoot }
  }
  $windows = Candidate-Windows
  $matches = @()
  for ($i = 0; $i -lt $windows.Count; $i++) {
    $candidate = $windows.Item($i)
    if (Match-Scope $candidate $processIds) {
      $matches += $candidate
    }
  }
  if ($matches.Count -eq 1) { return $matches[0] }
  if ($matches.Count -gt 1) {
    $script:scopeError = "Multiple Windows UIA windows matched; provide a unique PID, HWND, or narrower title."
  }
  return $null
}

function Denied-Target-Reason([System.Windows.Automation.AutomationElement]$element) {
  try {
    $processName = [string](Get-Process -Id ([int]$element.Current.ProcessId) -ErrorAction Stop).ProcessName
  } catch {
    return "The scoped target process identity could not be verified."
  }
  $processName = $processName.Trim().ToLowerInvariant()
  $className = ([string]$element.Current.ClassName).Trim().ToLowerInvariant()
  $deniedProcesses = @(
    "1password", "authhost", "bitwarden", "cmd", "conhost", "consent",
    "credentialuibroker", "dashlane", "enpass", "keeperpasswordmanager",
    "keepass", "keepassxc", "lastpass", "lockapp", "logonui", "nordpass",
    "openconsole", "powershell", "powershell_ise", "pwsh", "roboform", "sechealthui",
    "securityhealthhost", "systemsettings", "windowsterminal", "wt"
  )
  $deniedClasses = @(
    "cascadia_hosting_window_class", "consolewindowclass",
    "credential dialog xaml host", "lockscreenroot"
  )
  if ($deniedProcesses -contains $processName) {
    return "System, terminal, authentication, or password-manager targets are not allowed."
  }
  if ($deniedClasses -contains $className) {
    return "System, terminal, authentication, or lock-screen windows are not allowed."
  }
  if ($processName -eq "explorer" -and $className -eq "#32770") {
    return "The Windows Run dialog is not an allowed app_ui target."
  }
  return $null
}

function Denied-Action-Target-Reason(
  [System.Windows.Automation.AutomationElement]$root,
  [System.Windows.Automation.AutomationElement]$target
) {
  try {
    $rootProcessId = [int]$root.Current.ProcessId
    if ($rootProcessId -le 0) {
      return "The scoped target process boundary could not be verified."
    }
    $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
    $current = $target
    for ($depth = 0; $depth -lt 512 -and $null -ne $current; $depth++) {
      $currentInfo = $current.Current
      $currentProcessId = [int]$currentInfo.ProcessId
      if ($currentProcessId -ne $rootProcessId) {
        return "Cross-process descendant controls are not allowed mutation targets."
      }
      if ([bool]$currentInfo.IsPassword) {
        return "Password controls are not allowed mutation targets."
      }
      $deniedReason = Denied-Target-Reason $current
      if ($null -ne $deniedReason) { return $deniedReason }
      if ([System.Windows.Automation.Automation]::Compare($current, $root)) { return $null }
      $current = $walker.GetParent($current)
    }
  } catch {
    return "The mutation target security boundary could not be verified."
  }
  return "The mutation target is outside the scoped UI Automation root."
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
    return @{
      ok = $false
      error = "unsupported_action"
      message = "click requires InvokePattern, TogglePattern, or a native button handle"
    }
  }
  return @{ok = $false; error = "unsupported_action"; message = "unsupported Windows UIA action"}
}

try {
  $script:scopeError = $null
  $root = Find-Scoped-Root
  if ($null -eq $root) {
    $scopeErrorCode = if ($null -eq $script:scopeError) { "missing_window" } else { "invalid_target" }
    $scopeMessage = if ($null -eq $script:scopeError) {
      "No scoped Windows UIA window matched the supplied policy."
    } else {
      $script:scopeError
    }
    @{ok = $false; error = $scopeErrorCode; message = $scopeMessage} |
      ConvertTo-Json -Depth 64 -Compress
    exit 0
  }
  $deniedTargetReason = Denied-Target-Reason $root
  if ($null -ne $deniedTargetReason) {
    @{ok = $false; error = "permission_denied"; message = $deniedTargetReason} |
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
    $deniedActionTargetReason = Denied-Action-Target-Reason $root $target
    if ($null -ne $deniedActionTargetReason) {
      @{ok = $false; error = "permission_denied"; message = $deniedActionTargetReason} |
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


def _session_lock(session_id: str) -> threading.RLock:
    safe_id = _safe_session_id(session_id)
    with _SESSION_LOCKS_GUARD:
        lock = _SESSION_LOCKS.get(safe_id)
        if lock is None:
            lock = threading.RLock()
            _SESSION_LOCKS[safe_id] = lock
        return lock


def _session_stop_lock(session_id: str) -> threading.Lock:
    safe_id = _safe_session_id(session_id)
    with _SESSION_LOCKS_GUARD:
        lock = _SESSION_STOP_LOCKS.get(safe_id)
        if lock is None:
            lock = threading.Lock()
            _SESSION_STOP_LOCKS[safe_id] = lock
        return lock


def _mark_session_stopping(session_id: str, delta: int) -> None:
    with _SESSION_LOCKS_GUARD:
        count = _COMPUTER_USE_STOPPING.get(session_id, 0) + delta
        if count > 0:
            _COMPUTER_USE_STOPPING[session_id] = count
        else:
            _COMPUTER_USE_STOPPING.pop(session_id, None)


def _session_stop_generation(session_id: str) -> int:
    with _SESSION_LOCKS_GUARD:
        return _SESSION_STOP_GENERATIONS.get(_safe_session_id(session_id), 0)


def _bump_session_stop_generation(session_id: str) -> None:
    safe_id = _safe_session_id(session_id)
    with _SESSION_LOCKS_GUARD:
        _SESSION_STOP_GENERATIONS[safe_id] = _SESSION_STOP_GENERATIONS.get(safe_id, 0) + 1


def _native_desktop_interactive() -> bool:
    if _ComputerUseSession is None:
        return True
    checker = getattr(_ComputerUseSession, "desktop_interactive", None)
    if not callable(checker):
        return True
    try:
        return bool(checker())
    except Exception:
        return False


def _desktop_unavailable_result(session_id: str) -> Dict[str, Any]:
    _COMPUTER_USE_OBSERVATIONS.pop(_safe_session_id(session_id), None)
    return skill_error(
        _DESKTOP_UNAVAILABLE_MESSAGE,
        UiErrorCode.DESKTOP_UNAVAILABLE,
        error_code=UiErrorCode.DESKTOP_UNAVAILABLE,
        backend="windows-uia",
    )


def _serialize_session_call(
    function: Callable[[Optional[Dict[str, Any]]], Dict[str, Any]],
) -> Callable[[Optional[Dict[str, Any]]], Dict[str, Any]]:
    @wraps(function)
    def wrapped(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        resolved = dict(params) if params is not None else _read_params()
        session_id = _safe_session_id(resolved.get("session_id"))
        with _session_lock(session_id):
            if session_id in _COMPUTER_USE_STOPPING:
                return skill_error(
                    "DCC MCP Computer Use is stopping; retry after stop completes.",
                    UiErrorCode.BACKEND_UNAVAILABLE,
                )
            if not _native_desktop_interactive():
                return _desktop_unavailable_result(session_id)
            return function(resolved)

    return wrapped


def _state_dir() -> Path:
    root = os.environ.get("DCC_MCP_APP_UI_UIA_STATE_DIR")
    path = Path(root) if root else Path(tempfile.gettempdir()) / "dcc-mcp-app-ui-uia" / f"process-{os.getpid()}"
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
    ceiling = _env_flag("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT")
    return AppUiPolicy(
        allow_raw_coordinates=ceiling,
        allow_keyboard_shortcuts=ceiling,
    ).narrowed({key: raw[key] for key in _POLICY_KEYS if key in raw})


def _env_flag(name: str) -> bool:
    return str(os.environ.get(name) or "").strip().lower() in {"1", "true", "yes", "on"}


def _positive_int(value: Any) -> Optional[int]:
    if value is None or value == "":
        return None
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        return None
    return parsed if parsed > 0 else None


def _intersect_title_constraints(left: str, right: str) -> Optional[str]:
    if not left:
        return right or None
    if not right:
        return left
    left_folded = left.casefold()
    right_folded = right.casefold()
    if left_folded in right_folded:
        return right
    if right_folded in left_folded:
        return left
    return None


def _process_name_key(value: str) -> str:
    return Path(value.strip()).stem.casefold()


def _scope_from_params(params: Dict[str, Any], policy: AppUiPolicy) -> Dict[str, Any]:
    invalid_reason = None
    trusted_title = str(os.environ.get("DCC_MCP_APP_UI_UIA_WINDOW_TITLE") or "").strip()
    requested_title = str(params.get("window_title") or "").strip()
    effective_title = _intersect_title_constraints(trusted_title, requested_title)
    if trusted_title and requested_title and effective_title is None:
        invalid_reason = "the requested window title does not intersect the runtime DCC title scope"
    allowed_titles = [str(item).strip() for item in policy.allowed_window_titles if str(item).strip()]
    if policy.scope_denied:
        invalid_reason = "the requested policy scope does not intersect the runtime allowlist"
    elif allowed_titles and effective_title:
        compatible = []
        for allowed in allowed_titles:
            narrowed = _intersect_title_constraints(effective_title, allowed)
            if narrowed is not None:
                compatible.append(narrowed)
        if not compatible:
            invalid_reason = "the requested window title is outside the policy allowlist"
        else:
            effective_title = max(compatible, key=len)
    titles = [effective_title] if effective_title else allowed_titles

    allowed_process_ids = {int(item) for item in policy.allowed_process_ids if int(item) > 0}
    raw_trusted_pid = os.environ.get("DCC_MCP_APP_UI_UIA_PROCESS_ID")
    trusted_process_id = _positive_int(raw_trusted_pid)
    if raw_trusted_pid and trusted_process_id is None:
        invalid_reason = invalid_reason or "the runtime DCC process id scope is invalid"
    raw_requested_pid = params.get("process_id")
    requested_process_id = _positive_int(raw_requested_pid)
    if raw_requested_pid is not None and requested_process_id is None:
        invalid_reason = invalid_reason or "the requested process id is invalid"
    if trusted_process_id and requested_process_id and trusted_process_id != requested_process_id:
        invalid_reason = invalid_reason or "the requested process id is outside the runtime DCC scope"
    effective_process_id = requested_process_id or trusted_process_id
    if effective_process_id and allowed_process_ids and effective_process_id not in allowed_process_ids:
        invalid_reason = invalid_reason or "the requested process id is outside the policy allowlist"
    process_ids = [effective_process_id] if effective_process_id else sorted(allowed_process_ids)

    trusted_process_name = str(os.environ.get("DCC_MCP_APP_UI_UIA_PROCESS_NAME") or "").strip()
    requested_process_name = str(params.get("process_name") or "").strip()
    if (
        trusted_process_name
        and requested_process_name
        and _process_name_key(trusted_process_name) != _process_name_key(requested_process_name)
    ):
        invalid_reason = invalid_reason or "the requested process name is outside the runtime DCC scope"
    effective_process_name = requested_process_name or trusted_process_name
    if effective_process_name and _process_name_key(effective_process_name) in _DENIED_PROCESS_NAMES:
        invalid_reason = invalid_reason or (
            "system, terminal, authentication, and password-manager processes are not allowed app_ui targets"
        )
    process_names = [effective_process_name] if effective_process_name else []

    raw_trusted_handle = os.environ.get("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE")
    trusted_window_handle = _positive_int(raw_trusted_handle)
    if raw_trusted_handle and trusted_window_handle is None:
        invalid_reason = invalid_reason or "the runtime DCC window handle scope is invalid"
    raw_requested_handle = params.get("window_handle")
    requested_window_handle = _positive_int(raw_requested_handle)
    if raw_requested_handle is not None and requested_window_handle is None:
        invalid_reason = invalid_reason or "the requested window handle is invalid"
    if trusted_window_handle and requested_window_handle and trusted_window_handle != requested_window_handle:
        invalid_reason = invalid_reason or "the requested window handle is outside the runtime DCC scope"
    effective_window_handle = requested_window_handle or trusted_window_handle
    window_handles = [effective_window_handle] if effective_window_handle else []

    explicit_scope = bool(titles or process_ids or process_names or window_handles)
    return {
        "window_titles": [item for item in titles if str(item).strip()],
        "process_ids": [item for item in process_ids if int(item) > 0],
        "process_names": [item for item in process_names if str(item).strip()],
        "window_handles": [item for item in window_handles if item > 0],
        "excluded_process_ids": [] if explicit_scope else [os.getpid()],
        "require_process_match": bool(process_ids or process_names),
        "native_scope_trusted": bool(trusted_process_id or trusted_window_handle),
        "invalid_reason": invalid_reason,
    }


def _scope_is_explicit(scope: Dict[str, Any]) -> bool:
    return bool(scope["window_titles"] or scope["process_ids"] or scope["process_names"] or scope["window_handles"])


def _scope_is_trusted_native_target(scope: Dict[str, Any]) -> bool:
    return bool(
        not scope.get("invalid_reason")
        and scope.get("native_scope_trusted")
        and not scope.get("process_names")
        and (len(scope.get("process_ids") or []) == 1 or len(scope.get("window_handles") or []) == 1)
    )


def _json_object(value: Any) -> Dict[str, Any]:
    try:
        parsed = json.loads(value)
    except (TypeError, ValueError):
        return {}
    return parsed if isinstance(parsed, dict) else {}


def _request_stop_computer_use_session(session_id: str) -> bool:
    entry = _COMPUTER_USE_SESSIONS.get(_safe_session_id(session_id))
    if not entry:
        return False
    request_stop = getattr(entry["session"], "request_stop", None)
    if callable(request_stop):
        with suppress(Exception):
            request_stop()
    return True


def _stop_computer_use_session(session_id: str) -> Dict[str, Any]:
    safe_id = _safe_session_id(session_id)
    _request_stop_computer_use_session(safe_id)
    entry = _COMPUTER_USE_SESSIONS.get(safe_id)
    _COMPUTER_USE_OBSERVATIONS.pop(safe_id, None)
    if not entry:
        return {"success": True, "active": False, "cleanup_pending": False}
    try:
        raw = _json_object(entry["session"].stop())
    except Exception as exc:
        return {
            "success": False,
            "active": False,
            "cleanup_pending": True,
            "message": f"Computer Use cleanup could not be confirmed: {exc}",
        }
    if raw.get("cleanup_pending"):
        return {
            **raw,
            "success": False,
            "active": False,
            "cleanup_pending": True,
        }
    if _COMPUTER_USE_SESSIONS.get(safe_id) is entry:
        _COMPUTER_USE_SESSIONS.pop(safe_id, None)
    return {
        **raw,
        "success": bool(raw.get("success", True)),
        "active": False,
        "cleanup_pending": False,
    }


def _latch_user_interrupt(session_id: str) -> None:
    safe_id = _safe_session_id(session_id)
    _COMPUTER_USE_INTERRUPTED.add(safe_id)
    _stop_computer_use_session(safe_id)


def _user_interrupted_capture() -> Dict[str, Any]:
    return {
        "success": False,
        "error": UiErrorCode.USER_INTERRUPTED,
        "message": (
            "The user pressed Ctrl+Alt+Esc; DCC MCP Computer Use remains stopped. "
            "Only resume after explicit user approval with resume_computer_use=true."
        ),
    }


def _native_process_user_interrupted() -> bool:
    if _ComputerUseSession is None:
        return False
    checker = getattr(_ComputerUseSession, "process_user_interrupted", None)
    if not callable(checker):
        return False
    try:
        return bool(checker())
    except Exception:
        return False


def _stop_all_computer_use_sessions() -> None:
    for session_id in list(_COMPUTER_USE_SESSIONS):
        _stop_computer_use_session(session_id)


def request_stop() -> None:
    """Cooperatively cancel active native input before package cleanup waits."""
    _CLEANUP_REQUESTED.set()
    for session_id in list(_COMPUTER_USE_SESSIONS):
        _request_stop_computer_use_session(session_id)


def cleanup() -> None:
    """Release backend-owned input and overlays before package unload."""
    _CLEANUP_REQUESTED.set()
    _stop_all_computer_use_sessions()
    with suppress(Exception):
        atexit.unregister(_stop_all_computer_use_sessions)


atexit.register(_stop_all_computer_use_sessions)


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


def _uia_guard_failure(session_id: str) -> Optional[Dict[str, Any]]:
    safe_id = _safe_session_id(session_id)
    if safe_id in _COMPUTER_USE_INTERRUPTED:
        return {
            "ok": False,
            "error": UiErrorCode.USER_INTERRUPTED,
            "message": _user_interrupted_capture()["message"],
        }
    if _native_process_user_interrupted():
        _latch_user_interrupt(safe_id)
        return {
            "ok": False,
            "error": UiErrorCode.USER_INTERRUPTED,
            "message": _user_interrupted_capture()["message"],
        }
    if not _native_desktop_interactive():
        _COMPUTER_USE_OBSERVATIONS.pop(safe_id, None)
        return {
            "ok": False,
            "error": UiErrorCode.DESKTOP_UNAVAILABLE,
            "message": _DESKTOP_UNAVAILABLE_MESSAGE,
        }
    if safe_id in _COMPUTER_USE_STOPPING:
        return {
            "ok": False,
            "error": UiErrorCode.BACKEND_UNAVAILABLE,
            "message": "DCC MCP Computer Use was stopped while the Windows UIA action was running.",
        }
    entry = _COMPUTER_USE_SESSIONS.get(safe_id)
    if not entry or not hasattr(entry["session"], "status"):
        return None
    try:
        status = _json_object(entry["session"].status())
    except Exception:
        return {
            "ok": False,
            "error": UiErrorCode.BACKEND_ERROR,
            "message": "The Computer Use safety monitor could not verify the active session.",
        }
    if status.get("user_interrupted"):
        _latch_user_interrupt(safe_id)
        return {
            "ok": False,
            "error": UiErrorCode.USER_INTERRUPTED,
            "message": _user_interrupted_capture()["message"],
        }
    if status.get("desktop_interactive") is False:
        _COMPUTER_USE_OBSERVATIONS.pop(safe_id, None)
        return {
            "ok": False,
            "error": UiErrorCode.DESKTOP_UNAVAILABLE,
            "message": _DESKTOP_UNAVAILABLE_MESSAGE,
        }
    if status.get("active") is False:
        return {
            "ok": False,
            "error": UiErrorCode.BACKEND_UNAVAILABLE,
            "message": "DCC MCP Computer Use was stopped while the Windows UIA action was running.",
        }
    return None


def _stop_uia_process(process: Any) -> None:
    with suppress(Exception):
        process.terminate()
    try:
        process.communicate(timeout=0.5)
    except Exception:
        with suppress(Exception):
            process.kill()
        with suppress(Exception):
            process.communicate(timeout=0.5)


def _run_uia(payload: Dict[str, Any]) -> Dict[str, Any]:
    if os.name != "nt":
        raise RuntimeError("Windows UIA backend is only available on Windows")
    ps = _powershell_bin()
    if not ps:
        raise RuntimeError("PowerShell executable not found for Windows UIA backend")
    payload = dict(payload)
    guard_session_id = str(payload.pop("_session_id", ""))
    timeout = float(os.environ.get("DCC_MCP_APP_UI_UIA_TIMEOUT_SECS", "12"))
    with tempfile.NamedTemporaryFile("w", suffix=".ps1", delete=False, encoding="utf-8") as handle:
        handle.write(_UIA_SCRIPT)
        script_path = handle.name
    try:
        if guard_session_id:
            guarded_failure = _uia_guard_failure(guard_session_id)
            if guarded_failure is not None:
                return guarded_failure
            process = subprocess.Popen(
                [ps, "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", script_path],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            input_text: Optional[str] = json.dumps(payload)
            deadline = time.monotonic() + timeout
            while True:
                guarded_failure = _uia_guard_failure(guard_session_id)
                if guarded_failure is not None:
                    _stop_uia_process(process)
                    return guarded_failure
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    _stop_uia_process(process)
                    raise RuntimeError(f"Windows UIA command timed out after {timeout:g} seconds")
                try:
                    stdout, stderr = process.communicate(input=input_text, timeout=min(0.05, remaining))
                    break
                except subprocess.TimeoutExpired:
                    input_text = None
            guarded_failure = _uia_guard_failure(guard_session_id)
            if guarded_failure is not None:
                return guarded_failure
            returncode = process.returncode
        else:
            try:
                completed = subprocess.run(
                    [ps, "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", script_path],
                    input=json.dumps(payload),
                    capture_output=True,
                    text=True,
                    timeout=timeout,
                )
            except subprocess.TimeoutExpired as exc:
                raise RuntimeError(f"Windows UIA command timed out after {exc.timeout:g} seconds") from exc
            stdout, stderr, returncode = completed.stdout, completed.stderr, completed.returncode
    finally:
        with suppress(OSError):
            Path(script_path).unlink()
    if returncode != 0:
        raise RuntimeError((stderr or stdout or "Windows UIA command failed").strip())
    try:
        parsed = json.loads(stdout or "{}")
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
    guard_session_id: Optional[str] = None,
) -> Dict[str, Any]:
    scope = _scope_from_params(params, policy)
    if scope.get("invalid_reason"):
        return {
            "success": False,
            "error": UiErrorCode.INVALID_TARGET,
            "message": str(scope["invalid_reason"]),
        }
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
        _COMPUTER_USE_OBSERVATIONS.pop(session_id, None)
        state["revision"] = int(state.get("revision") or 0) + 1
    snapshot_id = _snapshot_id(state)
    payload = {
        "mode": "snapshot",
        "scope": scope,
        "max_depth": int(os.environ.get("DCC_MCP_APP_UI_UIA_MAX_DEPTH", "5")),
        "max_nodes": int(os.environ.get("DCC_MCP_APP_UI_UIA_MAX_NODES", "250")),
    }
    if guard_session_id:
        payload["_session_id"] = guard_session_id
    try:
        raw = _run_uia(payload)
    except RuntimeError as exc:
        return {
            "success": False,
            "error": "backend_unavailable",
            "message": str(exc),
            "scope": scope,
            "snapshot_id": snapshot_id,
            "_state": state,
        }
    if not raw.get("ok"):
        return {
            "success": False,
            "error": str(raw.get("error") or UiErrorCode.BACKEND_ERROR),
            "message": str(raw.get("message") or "Windows UIA snapshot failed."),
            "scope": scope,
            "snapshot_id": snapshot_id,
            "_state": state,
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
    return {
        "success": True,
        "snapshot": snapshot,
        "snapshot_id": snapshot_id,
        "scope": scope,
        "target": raw["root"],
    }


def _error_from_capture(capture: Dict[str, Any]) -> Dict[str, Any]:
    error = str(capture.get("error") or UiErrorCode.BACKEND_ERROR)
    message = str(capture.get("message") or "Windows UIA backend failed.")
    if error == "backend_unavailable":
        return _backend_unavailable(message)
    return skill_error(message, error, error_code=error, backend="windows-uia")


def _native_fallback_capture(
    session_id: str,
    policy: AppUiPolicy,
    params: Dict[str, Any],
    failure: Dict[str, Any],
) -> Optional[Dict[str, Any]]:
    scope = failure.get("scope") or _scope_from_params(params, policy)
    if not _scope_is_trusted_native_target(scope):
        return None

    state = failure.get("_state") or _load_state(session_id)
    _COMPUTER_USE_OBSERVATIONS.pop(session_id, None)
    snapshot_id = str(failure.get("snapshot_id") or _snapshot_id(state))
    process_id = scope["process_ids"][0] if scope["process_ids"] else 0
    window_handle = scope["window_handles"][0] if scope["window_handles"] else 0
    window_title = scope["window_titles"][0] if scope["window_titles"] else ""
    label = str(params.get("app_name") or window_title or "DCC application")
    backend_metadata = {
        "backend": "windows-native-fallback",
        "snapshot_id": snapshot_id,
        "process_id": process_id,
        "native_window_handle": window_handle,
        "fallback_reason": str(failure.get("message") or "Windows UIA snapshot failed."),
    }
    root = UiControlNode(
        id=f"native:window:{window_handle}" if window_handle else f"native:process:{process_id}",
        role="window",
        label=label,
        metadata={"app_ui": backend_metadata},
    )
    snapshot = UiSnapshot(
        root=root,
        session_id=session_id,
        metadata={
            "snapshot_id": snapshot_id,
            "app_ui": {
                **backend_metadata,
                "scope": scope,
            },
        },
    ).to_dict()
    state["last_snapshot_id"] = snapshot_id
    _save_state(state)
    return {
        "success": True,
        "snapshot": snapshot,
        "snapshot_id": snapshot_id,
        "scope": scope,
        "target": {
            "name": window_title,
            "process_id": process_id,
            "native_window_handle": window_handle,
        },
    }


def _computer_use_screenshot(
    session_id: str,
    capture: Dict[str, Any],
    params: Dict[str, Any],
) -> Dict[str, Any]:
    if _ComputerUseSession is None:
        return {
            "success": False,
            "error": "backend_unavailable",
            "message": "Native ComputerUseSession is unavailable in this dcc-mcp-core build.",
        }
    scope = capture.get("scope") or {}
    if not scope.get("native_scope_trusted"):
        return {
            "success": False,
            "error": UiErrorCode.PERMISSION_DENIED,
            "message": (
                "Native DCC MCP Computer Use requires an operator-bound DCC scope. "
                "Set DCC_MCP_APP_UI_UIA_PROCESS_ID or DCC_MCP_APP_UI_UIA_WINDOW_HANDLE "
                "in the adapter environment before enabling raw input."
            ),
        }
    if scope.get("process_names"):
        return {
            "success": False,
            "error": UiErrorCode.INVALID_TARGET,
            "message": (
                "process_name scopes are observation-only for native Computer Use; "
                "bind the adapter to an exact DCC process id or window handle instead."
            ),
        }
    if len(scope.get("process_ids") or []) != 1 and len(scope.get("window_handles") or []) != 1:
        return {
            "success": False,
            "error": UiErrorCode.INVALID_TARGET,
            "message": (
                "Native DCC MCP Computer Use requires one exact process_id or window_handle; "
                "title-only and process-name scopes are observation-only because they can match the wrong app."
            ),
        }

    target = capture.get("target")
    if not isinstance(target, dict):
        return {
            "success": False,
            "error": UiErrorCode.INVALID_TARGET,
            "message": "The UI snapshot did not resolve one native DCC window.",
        }
    target_process_id = _positive_int(target.get("process_id"))
    target_window_handle = _positive_int(target.get("native_window_handle"))
    scoped_process_ids = {int(item) for item in scope.get("process_ids") or []}
    scoped_window_handles = {int(item) for item in scope.get("window_handles") or []}
    target_title = str(target.get("name") or "").strip()
    scoped_titles = [str(item).strip() for item in scope.get("window_titles") or [] if str(item).strip()]
    if scoped_process_ids and target_process_id not in scoped_process_ids:
        return {
            "success": False,
            "error": UiErrorCode.INVALID_TARGET,
            "message": "The resolved UI window is outside the operator-bound DCC process scope.",
        }
    if scoped_window_handles and target_window_handle not in scoped_window_handles:
        return {
            "success": False,
            "error": UiErrorCode.INVALID_TARGET,
            "message": "The resolved UI window is outside the operator-bound DCC window scope.",
        }
    if scoped_titles and not any(title.casefold() in target_title.casefold() for title in scoped_titles):
        return {
            "success": False,
            "error": UiErrorCode.INVALID_TARGET,
            "message": "The resolved UI window is outside the scoped DCC title allowlist.",
        }

    resume = bool(params.get("resume_computer_use"))
    entry = _COMPUTER_USE_SESSIONS.get(session_id)
    if entry and hasattr(entry["session"], "status"):
        with suppress(Exception):
            current_status = _json_object(entry["session"].status())
            if current_status.get("user_interrupted"):
                _latch_user_interrupt(session_id)
                entry = None
    if session_id in _COMPUTER_USE_INTERRUPTED:
        if not resume:
            return _user_interrupted_capture()
        _COMPUTER_USE_INTERRUPTED.discard(session_id)

    process_id = target_process_id
    window_handle = target_window_handle
    window_title = str(target.get("name") or "") or None
    app_name = str(
        params.get("app_name") or os.environ.get("DCC_MCP_COMPUTER_USE_APP_NAME") or window_title or "DCC application"
    )
    spec = (process_id, window_handle, window_title, app_name)
    entry = _COMPUTER_USE_SESSIONS.get(session_id)
    if entry and entry["spec"] != spec:
        stop_status = _stop_computer_use_session(session_id)
        if stop_status.get("cleanup_pending"):
            return {
                "success": False,
                "error": UiErrorCode.BACKEND_UNAVAILABLE,
                "message": (
                    "The previous Computer Use session is still removing its input owner and overlays; retry shortly."
                ),
                "cleanup_pending": True,
            }
        entry = None
    if not entry:
        try:
            session = _ComputerUseSession(
                process_id=process_id,
                window_handle=window_handle,
                window_title=window_title,
                app_name=app_name,
            )
        except Exception as exc:
            return {"success": False, "error": "backend_unavailable", "message": str(exc)}
        entry = {"session": session, "spec": spec}
        _COMPUTER_USE_SESSIONS[session_id] = entry
    session = entry["session"]
    if resume:
        try:
            resumed = _json_object(session.resume_after_user_approval())
        except Exception as exc:
            _stop_computer_use_session(session_id)
            return {"success": False, "error": "backend_unavailable", "message": str(exc)}
        if not resumed.get("success"):
            _stop_computer_use_session(session_id)
            return resumed
    try:
        started = _json_object(session.start())
    except Exception as exc:
        _stop_computer_use_session(session_id)
        return {"success": False, "error": "backend_unavailable", "message": str(exc)}
    if not started.get("success"):
        if started.get("error") == UiErrorCode.USER_INTERRUPTED or started.get("user_interrupted"):
            _latch_user_interrupt(session_id)
            return _user_interrupted_capture()
        if started.get("error") == UiErrorCode.DESKTOP_UNAVAILABLE:
            _COMPUTER_USE_OBSERVATIONS.pop(session_id, None)
            return {
                "success": False,
                "error": UiErrorCode.DESKTOP_UNAVAILABLE,
                "message": str(started.get("message") or _DESKTOP_UNAVAILABLE_MESSAGE),
            }
        _stop_computer_use_session(session_id)
        return started
    try:
        metadata_json, image = session.screenshot()
    except Exception as exc:
        status = {}
        if hasattr(session, "status"):
            with suppress(Exception):
                status = _json_object(session.status())
        if status.get("user_interrupted"):
            _latch_user_interrupt(session_id)
            return _user_interrupted_capture()
        _stop_computer_use_session(session_id)
        return {"success": False, "error": "capture_failed", "message": str(exc)}
    metadata = _json_object(metadata_json)
    if not metadata.get("success") or image is None:
        if metadata.get("error") == UiErrorCode.USER_INTERRUPTED or metadata.get("user_interrupted"):
            _latch_user_interrupt(session_id)
            return _user_interrupted_capture()
        if metadata.get("error") == UiErrorCode.DESKTOP_UNAVAILABLE:
            _COMPUTER_USE_OBSERVATIONS.pop(session_id, None)
            return {
                "success": False,
                "error": UiErrorCode.DESKTOP_UNAVAILABLE,
                "message": str(metadata.get("message") or _DESKTOP_UNAVAILABLE_MESSAGE),
            }
        if metadata.get("error") in {UiErrorCode.MISSING_WINDOW, UiErrorCode.FOCUS_LOST}:
            status = {}
            if hasattr(session, "status"):
                with suppress(Exception):
                    status = _json_object(session.status())
            if status.get("active"):
                _COMPUTER_USE_OBSERVATIONS.pop(session_id, None)
                return metadata
        _stop_computer_use_session(session_id)
        return metadata or {
            "success": False,
            "error": "capture_failed",
            "message": "Native computer-use screenshot returned no PNG data.",
        }
    observation = metadata.get("observation")
    if not isinstance(observation, dict) or not observation.get("observation_id"):
        _stop_computer_use_session(session_id)
        return {
            "success": False,
            "error": "capture_failed",
            "message": "Native computer-use screenshot returned no observation id.",
        }
    _COMPUTER_USE_OBSERVATIONS[session_id] = {
        "snapshot_id": capture["snapshot_id"],
        "observation_id": str(observation["observation_id"]),
    }
    return {
        "success": True,
        "image": bytes(image),
        "mime_type": str(metadata.get("mime_type") or "image/png"),
        "observation": observation,
        "status": started,
    }


@_serialize_session_call
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
        fallback = None
        if capture.get("error") in {UiErrorCode.BACKEND_ERROR, UiErrorCode.BACKEND_UNAVAILABLE}:
            fallback = _native_fallback_capture(
                session_id,
                policy,
                params,
                capture,
            )
        if fallback is None:
            return _error_from_capture(capture)
        capture = fallback
    native_fallback = (
        capture["snapshot"].get("metadata", {}).get("app_ui", {}).get("backend") == "windows-native-fallback"
    )
    result = skill_success(
        (
            "Captured native DCC MCP Computer Use screenshot after Windows UIA was unavailable."
            if native_fallback
            else "Captured Windows UIA app_ui snapshot."
        ),
        prompt=(
            "Inspect the image, perform one scoped app_ui__act with this snapshot_id, then snapshot again."
            if native_fallback
            else "Use app_ui__find to resolve a control, then app_ui__act with the returned snapshot_id."
        ),
        session_id=session_id,
        snapshot_id=capture["snapshot_id"],
        snapshot=capture["snapshot"],
        policy=policy.to_dict(),
    )
    raw_input_enabled = policy.allow_raw_coordinates or policy.allow_keyboard_shortcuts
    if raw_input_enabled or _scope_is_trusted_native_target(capture["scope"]):
        computer_use = _computer_use_screenshot(session_id, capture, params)
        if not computer_use.get("success"):
            return _error_from_capture(computer_use)
        observation = computer_use["observation"]
        snapshot_app_ui = capture["snapshot"].get("metadata", {}).get("app_ui", {})
        if native_fallback:
            root = capture["snapshot"]["root"]
            root_app_ui = root.setdefault("metadata", {}).setdefault("app_ui", {})
            resolved_target = {
                "process_id": observation.get("process_id"),
                "native_window_handle": observation.get("window_handle"),
                "window_title": observation.get("window_title"),
            }
            root_app_ui.update(resolved_target)
            snapshot_app_ui.update(resolved_target)
            if observation.get("window_title"):
                root["label"] = str(observation["window_title"])
            source_rect = observation.get("source_rect")
            if isinstance(source_rect, list) and len(source_rect) == 4:
                root["bounds"] = {
                    "x": float(source_rect[0]),
                    "y": float(source_rect[1]),
                    "width": float(source_rect[2]),
                    "height": float(source_rect[3]),
                }
        capture["snapshot"]["metadata"]["computer_use"] = observation
        result["context"]["snapshot"] = capture["snapshot"]
        result["context"]["observation"] = observation
        result["context"]["computer_use"] = computer_use["status"]
        result["context"]["control_hint"] = computer_use["status"].get("hint")
        result["context"]["__rich__"] = {
            "kind": "image",
            "data": base64.b64encode(computer_use["image"]).decode("ascii"),
            "mime": computer_use["mime_type"],
            "alt": f"{params.get('app_name') or 'DCC'} computer-use screenshot",
        }
    return result


@_serialize_session_call
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
    if action in (UiActionKind.SET_TEXT, UiActionKind.TYPE) and not policy.audit_sensitive_values:
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


def _stale_observation_result(
    action: str,
    control_id: str,
    session_id: str,
    requested: str,
    current: str,
    policy: AppUiPolicy,
) -> Dict[str, Any]:
    message = "Computer-use observation is stale; take a new app_ui snapshot."
    result = UiActionResult(
        success=False,
        control_id=control_id,
        error_code=UiErrorCode.STALE_OBSERVATION,
        message=message,
        metadata={"requested_snapshot_id": requested, "current_snapshot_id": current},
    ).to_dict()
    audit = _audit_record(
        action,
        False,
        None,
        session_id,
        policy,
        None,
        None,
        UiErrorCode.STALE_OBSERVATION,
        message,
    )
    return skill_error(
        message,
        UiErrorCode.STALE_OBSERVATION,
        result=result,
        audit=audit,
        current_snapshot_id=current,
    )


def _is_native_action(action: str, params: Dict[str, Any]) -> bool:
    if action == UiActionKind.CLICK:
        return params.get("x") is not None or params.get("y") is not None
    return action in {
        UiActionKind.MOVE,
        UiActionKind.DOUBLE_CLICK,
        UiActionKind.SCROLL,
        UiActionKind.DRAG,
        UiActionKind.RAW_COORDINATE_CLICK,
        UiActionKind.TYPE,
        UiActionKind.KEYPRESS,
        UiActionKind.KEYBOARD_SHORTCUT,
    }


def _native_action_request(params: Dict[str, Any], observation_id: str) -> Dict[str, Any]:
    action = str(params.get("action") or "")
    request = {
        "action": {
            UiActionKind.RAW_COORDINATE_CLICK: UiActionKind.CLICK,
            UiActionKind.KEYBOARD_SHORTCUT: UiActionKind.KEYPRESS,
        }.get(action, action),
        "observation_id": observation_id,
    }
    for key in ("x", "y", "button", "text", "duration_ms"):
        if params.get(key) is not None:
            request[key] = params[key]
    for key in ("scroll_x", "scroll_y"):
        if params.get(key) is not None:
            request[key] = int(params[key])
    if params.get("path") is not None:
        request["path"] = params["path"]
    if params.get("keys") is not None:
        request["keys"] = params["keys"]
    return request


def _validate_action_limits(params: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    path = params.get("path") or []
    if not isinstance(path, list):
        return skill_error("path must be an array", UiErrorCode.INVALID_ACTION)
    if len(path) > _MAX_DRAG_POINTS:
        return skill_error(
            f"drag path exceeds the {_MAX_DRAG_POINTS}-point safety limit",
            UiErrorCode.INVALID_ACTION,
        )

    keys = params.get("keys") or []
    if not isinstance(keys, list):
        return skill_error("keys must be an array", UiErrorCode.INVALID_ACTION)
    key_count = sum(1 for item in keys for token in str(item).split("+") if token.strip())
    if key_count > _MAX_KEY_TOKENS:
        return skill_error(
            f"keypress exceeds the {_MAX_KEY_TOKENS}-key safety limit",
            UiErrorCode.INVALID_ACTION,
        )

    text = params.get("text")
    if text is not None:
        units = len(str(text).encode("utf-16-le")) // 2
        if units > _MAX_TEXT_UTF16_UNITS:
            return skill_error(
                f"text exceeds the {_MAX_TEXT_UTF16_UNITS}-UTF-16-unit safety limit",
                UiErrorCode.INVALID_ACTION,
            )
    return None


def _consume_action_observation(session_id: str, state: Dict[str, Any]) -> str:
    """Invalidate every coordinate binding before a UI dispatch can mutate state."""
    _COMPUTER_USE_OBSERVATIONS.pop(session_id, None)
    state["revision"] = int(state.get("revision") or 0) + 1
    state["last_snapshot_id"] = _snapshot_id(state)
    _save_state(state)
    return str(state["last_snapshot_id"])


def _run_native_action(
    session_id: str,
    state: Dict[str, Any],
    policy: AppUiPolicy,
    params: Dict[str, Any],
) -> Dict[str, Any]:
    action = str(params.get("action") or "")
    control_id = str(params.get("control_id") or "")
    requested_snapshot_id = str(params.get("snapshot_id") or "")
    current_snapshot_id = str(state.get("last_snapshot_id") or "")
    binding = _COMPUTER_USE_OBSERVATIONS.get(session_id)
    if (
        not requested_snapshot_id
        or requested_snapshot_id != current_snapshot_id
        or not binding
        or binding.get("snapshot_id") != requested_snapshot_id
    ):
        return _stale_observation_result(
            action,
            control_id,
            session_id,
            requested_snapshot_id,
            current_snapshot_id,
            policy,
        )
    entry = _COMPUTER_USE_SESSIONS.get(session_id)
    if not entry:
        return _backend_unavailable(
            "Native computer-use session is not available in this Python process; take a new snapshot in-process."
        )
    request = _native_action_request(params, binding["observation_id"])
    _consume_action_observation(session_id, state)
    try:
        raw = _json_object(entry["session"].act(json.dumps(request)))
    except Exception as exc:
        raw = {"success": False, "error": UiErrorCode.BACKEND_ERROR, "message": str(exc)}
    if not raw.get("success"):
        error = str(raw.get("error") or UiErrorCode.BACKEND_ERROR)
        message = str(raw.get("message") or "Native computer-use action failed.")
        result = UiActionResult(
            success=False,
            control_id=control_id,
            error_code=error,
            message=message,
            metadata={"snapshot_id": state["last_snapshot_id"], "requires_new_screenshot": True},
        ).to_dict()
        audit = _audit_record(action, False, None, session_id, policy, None, None, error, message)
        if error == UiErrorCode.USER_INTERRUPTED:
            _latch_user_interrupt(session_id)
        return skill_error(message, error, result=result, audit=audit)

    message = f"Completed native computer-use action {action!r}."
    result = UiActionResult(
        success=True,
        control_id=control_id,
        message=message,
        metadata={"snapshot_id": state["last_snapshot_id"], "requires_new_screenshot": True},
    ).to_dict()
    audit = _audit_record(action, True, None, session_id, policy, None, None, None, message)
    return skill_success(
        message,
        prompt="Take a new app_ui__snapshot before the next native action.",
        session_id=session_id,
        snapshot_id=state["last_snapshot_id"],
        result=result,
        audit=audit,
    )


@_serialize_session_call
def act_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    if _native_process_user_interrupted():
        _COMPUTER_USE_INTERRUPTED.add(session_id)
        return _user_interrupted_capture()
    policy = _policy_from_params(params)
    action = str(params.get("action") or "")
    limit_error = _validate_action_limits(params)
    if limit_error is not None:
        return limit_error
    control_id = str(params.get("control_id") or "")
    requested_snapshot_id = str(params.get("snapshot_id") or "")
    state = _load_state(session_id)
    current_snapshot_id = str(state.get("last_snapshot_id") or "")
    native_action = _is_native_action(action, params)
    if requested_snapshot_id and requested_snapshot_id != current_snapshot_id:
        if native_action:
            return _stale_observation_result(
                action,
                control_id,
                session_id,
                requested_snapshot_id,
                current_snapshot_id,
                policy,
            )
        return _stale_result(control_id, session_id, requested_snapshot_id, current_snapshot_id)
    request = UiActionRequest(
        control_id=control_id or None,
        action=action,
        x=params.get("x"),
        y=params.get("y"),
    )
    if not policy.allows_request(request):
        result = UiActionResult(
            success=False,
            control_id=control_id,
            error_code=UiErrorCode.POLICY_DISABLED,
            message=f"app_ui action {action!r} disabled by policy",
        ).to_dict()
        audit = _audit_record(action, False, None, session_id, policy, None, None, UiErrorCode.POLICY_DISABLED)
        return skill_error(result["message"], UiErrorCode.POLICY_DISABLED, result=result, audit=audit)
    if native_action:
        return _run_native_action(session_id, state, policy, params)

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

    if not _scope_is_trusted_native_target(capture["scope"]):
        return skill_error(
            (
                "Mutating Windows UIA actions require an operator-bound DCC process id or window handle "
                "so the visible Computer Use session and user interruption monitor target the same window."
            ),
            UiErrorCode.PERMISSION_DENIED,
        )
    computer_use = _computer_use_screenshot(session_id, capture, params)
    if not computer_use.get("success"):
        return _error_from_capture(computer_use)

    payload = {
        "mode": "act",
        "_session_id": session_id,
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
    _consume_action_observation(session_id, state)
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
            metadata={"snapshot_id": state["last_snapshot_id"], "requires_new_screenshot": True},
        ).to_dict()
        audit = _audit_record(action, False, control, session_id, policy, before_focus, after_focus, error, message)
        return skill_error(message, error, result=result, audit=audit)

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


def stop_computer_use_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Stop one visible Computer Use session without clearing the Ctrl+Alt+Esc latch."""
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    _bump_session_stop_generation(session_id)
    _mark_session_stopping(session_id, 1)
    try:
        was_active = _request_stop_computer_use_session(session_id)
        with _session_stop_lock(session_id), _session_lock(session_id):
            was_active = was_active or session_id in _COMPUTER_USE_SESSIONS
            stop_status = _stop_computer_use_session(session_id)
            state = _load_state(session_id)
            state["revision"] = int(state.get("revision") or 0) + 1
            state["last_snapshot_id"] = _snapshot_id(state)
            _save_state(state)
            cleanup_pending = bool(stop_status.get("cleanup_pending"))
            if cleanup_pending:
                return skill_error(
                    (
                        "Computer Use stop was requested, but input-owner or visual cleanup is pending. "
                        "retry stop shortly."
                    ),
                    UiErrorCode.BACKEND_UNAVAILABLE,
                    session_id=session_id,
                    active=False,
                    was_active=was_active,
                    cleanup_pending=True,
                    user_interrupted=(session_id in _COMPUTER_USE_INTERRUPTED or _native_process_user_interrupted()),
                )
            return skill_success(
                "Stopped DCC MCP Computer Use and removed its visible control effects.",
                session_id=session_id,
                active=False,
                was_active=was_active,
                cleanup_pending=False,
                user_interrupted=(session_id in _COMPUTER_USE_INTERRUPTED or _native_process_user_interrupted()),
            )
    finally:
        _mark_session_stopping(session_id, -1)


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
    timeout_ms = min(60_000, max(0, int(condition.timeout_ms)))
    condition.timeout_ms = timeout_ms
    interval_ms = max(10, int(condition.interval_ms))
    deadline = time.monotonic() + (timeout_ms / 1000.0)
    attempts = 0
    last_snapshot = None
    start = time.monotonic()
    stop_generation = _session_stop_generation(session_id)

    def interrupted() -> Optional[Dict[str, Any]]:
        if _session_stop_generation(session_id) != stop_generation:
            return skill_error(
                "app_ui wait cancelled because Computer Use was stopped.",
                UiErrorCode.BACKEND_UNAVAILABLE,
                session_id=session_id,
                attempts=attempts,
            )
        guarded_failure = _uia_guard_failure(session_id)
        if guarded_failure is None:
            return None
        error = str(guarded_failure.get("error") or UiErrorCode.BACKEND_UNAVAILABLE)
        return skill_error(
            str(guarded_failure.get("message") or "app_ui wait was interrupted."),
            error,
            session_id=session_id,
            attempts=attempts,
        )

    while True:
        interruption = interrupted()
        if interruption is not None:
            return interruption
        if _CLEANUP_REQUESTED.is_set():
            return skill_error(
                "app_ui wait cancelled because the backend is stopping.",
                UiErrorCode.BACKEND_UNAVAILABLE,
                session_id=session_id,
                attempts=attempts,
            )
        with _session_lock(session_id):
            interruption = interrupted()
            if interruption is not None:
                return interruption
            capture = _capture_snapshot(
                session_id,
                policy,
                params,
                bump_revision=True,
                guard_session_id=session_id,
            )
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
        sleep_deadline = min(deadline, time.monotonic() + interval_ms / 1000.0)
        while time.monotonic() < sleep_deadline:
            interruption = interrupted()
            if interruption is not None:
                return interruption
            if _CLEANUP_REQUESTED.wait(min(0.05, max(0.0, sleep_deadline - time.monotonic()))):
                break

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
