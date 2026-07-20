Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class DccMcpNativeUi {
  [DllImport("user32.dll", CharSet = CharSet.Auto)]
  public static extern IntPtr SendMessage(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);
}
"@

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
