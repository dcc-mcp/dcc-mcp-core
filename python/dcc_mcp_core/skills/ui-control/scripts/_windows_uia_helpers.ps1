Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class DccMcpNativeUi {
  private const uint GuiInMenuMode = 0x00000004;
  private const uint GetWindowOwner = 4;

  [StructLayout(LayoutKind.Sequential)]
  private struct NativeRect {
    public int Left;
    public int Top;
    public int Right;
    public int Bottom;
  }

  [StructLayout(LayoutKind.Sequential)]
  private struct GuiThreadInfo {
    public uint Size;
    public uint Flags;
    public IntPtr ActiveWindow;
    public IntPtr FocusWindow;
    public IntPtr CaptureWindow;
    public IntPtr MenuOwnerWindow;
    public IntPtr MoveSizeWindow;
    public IntPtr CaretWindow;
    public NativeRect CaretRect;
  }

  [DllImport("user32.dll", CharSet = CharSet.Auto)]
  public static extern IntPtr SendMessage(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);

  [DllImport("user32.dll")]
  public static extern IntPtr GetWindow(IntPtr hWnd, uint command);

  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern int GetClassName(IntPtr hWnd, StringBuilder className, int maxCount);

  [DllImport("user32.dll")]
  public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

  [DllImport("user32.dll")]
  [return: MarshalAs(UnmanagedType.Bool)]
  public static extern bool IsWindowVisible(IntPtr hWnd);

  [DllImport("user32.dll")]
  [return: MarshalAs(UnmanagedType.Bool)]
  private static extern bool GetGUIThreadInfo(uint threadId, ref GuiThreadInfo info);

  public static bool IsActiveOwnedStandardMenuPopup(
    IntPtr authorizedRoot,
    IntPtr popup,
    uint expectedProcessId
  ) {
    if (authorizedRoot == IntPtr.Zero || popup == IntPtr.Zero) {
      return false;
    }
    uint rootProcessId;
    uint popupProcessId;
    uint rootThreadId = GetWindowThreadProcessId(authorizedRoot, out rootProcessId);
    uint popupThreadId = GetWindowThreadProcessId(popup, out popupProcessId);
    if (rootThreadId == 0 || popupThreadId != rootThreadId ||
        rootProcessId != expectedProcessId || popupProcessId != expectedProcessId) {
      return false;
    }
    var className = new StringBuilder(256);
    if (GetClassName(popup, className, className.Capacity) <= 0 ||
        !String.Equals(className.ToString(), "#32768", StringComparison.Ordinal)) {
      return false;
    }
    if (!IsWindowVisible(popup) || GetWindow(popup, GetWindowOwner) != authorizedRoot) {
      return false;
    }
    var info = new GuiThreadInfo {
      Size = (uint)Marshal.SizeOf(typeof(GuiThreadInfo))
    };
    return GetGUIThreadInfo(rootThreadId, ref info) &&
      (info.Flags & GuiInMenuMode) != 0 &&
      info.ActiveWindow == authorizedRoot &&
      info.MenuOwnerWindow == authorizedRoot;
  }
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

function Find-Owned-Standard-Menu-Popup(
  [System.Windows.Automation.AutomationElement]$authorizedRoot
) {
  if (-not [bool]$payload.scope.allow_owned_standard_menu_popup) { return $null }
  try {
    $rootInfo = $authorizedRoot.Current
    $rootHandle = [IntPtr]::new([long]$rootInfo.NativeWindowHandle)
    if ($rootHandle -eq [IntPtr]::Zero) { return $null }
    $rootProcessId = [uint32]$rootInfo.ProcessId
    $condition = New-Object System.Windows.Automation.PropertyCondition(
      [System.Windows.Automation.AutomationElement]::ClassNameProperty,
      "#32768"
    )
    $matches = $authorizedRoot.FindAll([System.Windows.Automation.TreeScope]::Children, $condition)
    if ($matches.Count -ne 1) { return $null }
    $popup = $matches.Item(0)
    $popupInfo = $popup.Current
    if (([string]$popupInfo.ClassName) -cne "#32768") { return $null }
    if ([uint32]$popupInfo.ProcessId -ne $rootProcessId) { return $null }
    $popupHandle = [IntPtr]::new([long]$popupInfo.NativeWindowHandle)
    if (-not [DccMcpNativeUi]::IsActiveOwnedStandardMenuPopup(
      $rootHandle,
      $popupHandle,
      $rootProcessId
    )) { return $null }
    return [System.Windows.Automation.AutomationElement]::FromHandle($popupHandle)
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
