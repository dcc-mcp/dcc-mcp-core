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

function Value-Pattern-Metadata([System.Windows.Automation.AutomationElement]$element) {
  $result = [ordered]@{
    available = $null
    value = $null
  }
  try {
    $pattern = $null
    if ($element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
      $result.available = $true
      if (-not [bool]$element.Current.IsPassword) {
        try { $result.value = $pattern.Current.Value } catch {}
      }
    } else {
      $result.available = $false
    }
  } catch {}
  return $result
}

function Pattern-Available(
  [System.Windows.Automation.AutomationElement]$element,
  [System.Windows.Automation.AutomationPattern]$patternId
) {
  try {
    $pattern = $null
    return [bool]$element.TryGetCurrentPattern($patternId, [ref]$pattern)
  } catch {
    return $null
  }
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
  $valuePattern = Value-Pattern-Metadata $element
  $textPatternAvailable = Pattern-Available $element ([System.Windows.Automation.TextPattern]::Pattern)
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
    is_password = [bool]$current.IsPassword
    process_id = [int]$current.ProcessId
    native_window_handle = [int]$current.NativeWindowHandle
    enabled = [bool]$current.IsEnabled
    offscreen = [bool]$current.IsOffscreen
    focused = [bool]$current.HasKeyboardFocus
    bounds = Bounds-Object $current.BoundingRectangle
    value = $valuePattern.value
    value_pattern_available = $valuePattern.available
    text_pattern_available = $textPatternAvailable
    checked = Pattern-Checked $element
    children = $children
  }
}

function Is-Common-Save-Label([string]$value) {
  $normalized = (($value -replace '[^A-Za-z0-9]+', ' ').Trim()).ToLowerInvariant()
  return $normalized -in @(
    "save", "save as", "save button", "save as button",
    "save menu item", "save as menu item", "save command", "save as command"
  )
}

function Has-Authentication-Secret-Marker([System.Windows.Automation.AutomationElement]$element) {
  try {
    $current = $element.Current
    $normalized = ((([string]$current.Name) + " " + ([string]$current.AutomationId) + " " + ([string]$current.ClassName)) -replace '[^A-Za-z0-9]+', ' ').Trim().ToLowerInvariant()
    foreach ($phrase in @("password", "credential", "authentication code", "auth code", "verification code", "one time code", "passcode")) {
      if ($normalized.Contains($phrase)) { return $true }
    }
    foreach ($token in ($normalized -split ' ')) {
      if ($token -in @("otp", "mfa", "2fa")) { return $true }
    }
  } catch {
    return $true
  }
  return $false
}

function Control-Policy-Tier([System.Windows.Automation.AutomationElement]$element) {
  try {
    $current = $element.Current
    if ([bool]$current.IsPassword -or (Has-Authentication-Secret-Marker $element)) { return "hard_deny" }
    $text = (([string]$current.Name) + " " + ([string]$current.AutomationId) + " " + ([string]$current.ClassName)).ToLowerInvariant()
    foreach ($needle in @("password", "credential", "authentication code", "security settings", "privacy settings", "windows run", "command prompt", "powershell", "terminal")) {
      if ($text.Contains($needle)) { return "hard_deny" }
    }
    if ((Is-Common-Save-Label ([string]$current.Name)) -or
        (Is-Common-Save-Label ([string]$current.AutomationId)) -or
        (Is-Common-Save-Label ([string]$current.ClassName))) {
      return "action_confirmation"
    }
    foreach ($needle in @("delete", "remove permanently", "overwrite", "install", "purchase", "buy now", "pay", "send", "publish", "submit", "share", "grant access", "revoke access", "remote control", "remote connection", "allow remote")) {
      if ($text.Contains($needle)) { return "action_confirmation" }
    }
    foreach ($needle in @("sign in", "log in", "login", "permission", "upload", "move", "rename", "connect account")) {
      if ($text.Contains($needle)) { return "pre_approval" }
    }
  } catch {
    return "hard_deny"
  }
  return "task_grant"
}

function Matches-Expected-Fence([System.Windows.Automation.AutomationElement]$element, $expected) {
  if ($null -eq $expected) { return $false }
  try {
    $current = $element.Current
    $runtimeId = Runtime-Id $element
    if ([string]::IsNullOrWhiteSpace($runtimeId)) {
      $controlId = [string]$payload.action.control_id
      if (-not $controlId.StartsWith("uia:path:")) { return $false }
      $identity = $controlId.Substring(9)
    } else {
      $identity = $runtimeId
    }
    return $identity -ceq ([string]$expected.identity) -and
      ([bool]$current.IsPassword) -eq ([bool]$expected.is_password) -and
      ([string]$current.Name).ToLowerInvariant() -ceq ([string]$expected.name) -and
      ([string]$current.AutomationId).ToLowerInvariant() -ceq ([string]$expected.automation_id) -and
      ([string]$current.ClassName).ToLowerInvariant() -ceq ([string]$expected.class_name) -and
      (Control-Policy-Tier $element) -ceq ([string]$expected.policy_tier)
  } catch {
    return $false
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
  if ($handleMatches.Count -eq 1) {
    $ownedMenuPopup = Find-Owned-Standard-Menu-Popup $handleMatches[0]
    if ($null -ne $ownedMenuPopup) { return $ownedMenuPopup }
    return $handleMatches[0]
  }
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
    return "The Windows Run dialog is not an allowed ui_control target."
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
      if ([string]$payload.action.action -eq "set_text" -and (Has-Authentication-Secret-Marker $current)) {
        return "Authentication secret fields must be handed off to the user."
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
  if ($action -eq "select_option") {
    $pattern = $null
    if ($element.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
      $pattern.Select()
      return @{ok = $true; message = "selected option"}
    }
    return @{ok = $false; error = "unsupported_action"; message = "select_option requires SelectionItemPattern"}
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
    $beforeFocus = Runtime-Id ([System.Windows.Automation.AutomationElement]::FocusedElement)
    if (-not (Matches-Expected-Fence $target $payload.expected_fence)) {
      @{ok = $false; error = "stale_observation"; message = "The action-time UI Automation target changed after confirmation."} |
        ConvertTo-Json -Depth 64 -Compress
      exit 0
    }
    $deniedActionTargetReason = Denied-Action-Target-Reason $root $target
    if ($null -ne $deniedActionTargetReason) {
      @{ok = $false; error = "permission_denied"; message = $deniedActionTargetReason} |
        ConvertTo-Json -Depth 64 -Compress
      exit 0
    }
    $actionResult = Invoke-Action $target
    if (-not [bool]$actionResult.ok) {
      @{
        ok = $false
        error = $actionResult.error
        message = $actionResult.message
        before_focus_runtime_id = $beforeFocus
      } | ConvertTo-Json -Depth 64 -Compress
      exit 0
    }
    $afterFocus = $null
    try {
      $afterFocus = Runtime-Id ([System.Windows.Automation.AutomationElement]::FocusedElement)
    } catch {}
    $control = $null
    try {
      $control = Element-Raw $target 0 "target"
    } catch {}
    @{
      ok = $true
      message = $actionResult.message
      before_focus_runtime_id = $beforeFocus
      after_focus_runtime_id = $afterFocus
      control = $control
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
