# Windows MSVC toolchain

Windows native builds use the MSVC Rust host and the `x86_64-pc-windows-msvc`
target. The repository pins the Rust version in `rust-toolchain.toml`, while
the host triple still follows the local rustup default unless a toolchain is
selected explicitly.

## Recommended setup

Open a Visual Studio Developer PowerShell, change to the repository root, then
select the pinned MSVC host for the current shell:

```powershell
Set-Location "C:\path\to\dcc-mcp-core"
$env:RUSTUP_TOOLCHAIN = "1.95.0-x86_64-pc-windows-msvc"
rustc -vV
Get-Command cl.exe
Get-Command link.exe
vx just check
```

`rustc -vV` must report `host: x86_64-pc-windows-msvc`. `cl.exe` and
`link.exe` must resolve from the Visual Studio installation.

For a one-shot command from a regular PowerShell, discover the Visual Studio
environment, change to the repository root, and set the Rust host in the same
`cmd.exe` process. Replace the repository path with the checkout location.

```powershell
$repo = "C:\path\to\dcc-mcp-core"
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vsDevCmd = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -find Common7\Tools\VsDevCmd.bat | Select-Object -First 1
if (-not $vsDevCmd) {
    throw "Visual Studio C++ tools were not found. Set `$vsDevCmd to your VsDevCmd.bat path."
}
$command = "call `"$vsDevCmd`" -arch=x64 && cd /d `"$repo`" && set RUSTUP_TOOLCHAIN=1.95.0-x86_64-pc-windows-msvc && vx just check"
cmd.exe /d /s /c $command
```

## Diagnosing `dlltool.exe`

A `dlltool could not create import library` error during a Windows MSVC
target build usually means the active Rust host is GNU. Selecting only
`--target x86_64-pc-windows-msvc` does not change the host compiler. Check the
active host before changing dependencies or installing another GNU utility.

The GitHub Actions wheel and native Python 3.7 jobs select
`x86_64-pc-windows-msvc` explicitly and verify the Windows build contract.
