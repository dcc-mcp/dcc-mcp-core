"""PowerShell UI Automation script for the Windows UIA backend."""

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
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class DccMcpNativeUi {
  [DllImport("user32.dll", CharSet = CharSet.Auto)]
  public static extern IntPtr SendMessage(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);
}
"@

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

function Find-Process-Root($processIds) {
  if ($processIds.Count -ne 1) { return $null }
  try {
    $proc = Get-Process -Id ([int]$processIds[0]) -ErrorAction Stop
    if ($proc.MainWindowHandle -eq [IntPtr]::Zero) { return $null }
    return [System.Windows.Automation.AutomationElement]::FromHandle($proc.MainWindowHandle)
  } catch {
    return $null
  }
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

function Invoke-NativeButtonClick($element) {
  try {
    $handle = [IntPtr]::new([long]$element.Current.NativeWindowHandle)
    if ($handle -eq [IntPtr]::Zero) { return $false }
    [void][DccMcpNativeUi]::SendMessage($handle, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero)
    return $true
  } catch {
    return $false
  }
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
