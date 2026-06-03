param(
    [string] $Version = $env:DCC_MCP_VERSION,
    [string] $InstallDir = $env:DCC_MCP_INSTALL_DIR,
    [string] $Repo = $env:DCC_MCP_REPO
)

# One-line install:
# powershell -c "irm https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-core/main/scripts/install-cli.ps1 | iex"

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = "latest"
}
if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    $InstallDir = Join-Path $env:LOCALAPPDATA "dcc-mcp\bin"
}
if ([string]::IsNullOrWhiteSpace($Repo)) {
    $Repo = "dcc-mcp/dcc-mcp-core"
}

$asset = "dcc-mcp-cli-windows-x86_64.exe"
if ($Version -eq "latest") {
    $url = "https://github.com/$Repo/releases/latest/download/$asset"
} else {
    $url = "https://github.com/$Repo/releases/download/$Version/$asset"
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$target = Join-Path $InstallDir "dcc-mcp-cli.exe"
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("dcc-mcp-cli-" + [System.Guid]::NewGuid() + ".exe")

try {
    Write-Host "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $tmp
    Move-Item -Force -Path $tmp -Destination $target
} finally {
    if (Test-Path $tmp) {
        Remove-Item -Force $tmp
    }
}

Write-Host "Installed dcc-mcp-cli to $target"

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$pathParts = @()
if ($userPath) {
    $pathParts = $userPath -split ";"
}

if ($pathParts -notcontains $InstallDir) {
    $newPath = if ($userPath) { "$userPath;$InstallDir" } else { $InstallDir }
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    Write-Host "Added $InstallDir to the user PATH. Open a new terminal before running dcc-mcp-cli."
}
