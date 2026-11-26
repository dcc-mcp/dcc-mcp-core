param(
    [string] $Version = $env:DCC_MCP_VERSION,
    [string] $InstallDir = $env:DCC_MCP_INSTALL_DIR
)

$ErrorActionPreference = "Stop"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = "latest"
}
if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    $InstallDir = Join-Path $env:LOCALAPPDATA "dcc-mcp\bin"
}

$officialReleases = "https://github.com/dcc-mcp/dcc-mcp-cli/releases"
$platform = "windows-x86_64"
$asset = "dccmcp-windows-x86_64.exe"
$requestedVersion = $Version.TrimStart("v")
if ($Version -eq "latest") {
    $manifestUrl = "$officialReleases/latest/download/dcc-mcp-update-manifest-$platform.json"
} else {
    if ($requestedVersion -notmatch '^[0-9]+\.[0-9]+\.[0-9]+([.+-][0-9A-Za-z.-]+)?$') {
        throw "Invalid release version: $Version"
    }
    $manifestUrl = "$officialReleases/download/v$requestedVersion/dcc-mcp-update-manifest-$platform.json"
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$target = Join-Path $InstallDir "dccmcp.exe"
$compatibilityTarget = Join-Path $InstallDir "dcc-mcp-cli.exe"
$manifestPath = Join-Path ([System.IO.Path]::GetTempPath()) ("dccmcp-manifest-" + [System.Guid]::NewGuid() + ".json")
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("dccmcp-" + [System.Guid]::NewGuid() + ".exe")

try {
    Write-Host "Downloading $manifestUrl"
    Invoke-WebRequest -Uri $manifestUrl -OutFile $manifestPath
    $manifest = Get-Content -Raw -Path $manifestPath | ConvertFrom-Json
    $entry = $manifest.dccmcp
    if ($null -eq $entry) {
        $entry = $manifest.'dcc-mcp-cli'
    }
    if ($null -eq $entry) {
        throw "Official release manifest has no DCCMCP CLI entry."
    }
    if ([string]::IsNullOrWhiteSpace($entry.url) -or [string]::IsNullOrWhiteSpace($entry.sha256)) {
        throw "Official release manifest has an incomplete CLI entry."
    }
    $expectedPattern = '^https://github\.com/dcc-mcp/dcc-mcp-cli/releases/download/.*/' + [regex]::Escape($asset) + '$'
    if ($entry.url -notmatch $expectedPattern) {
        throw "Official release manifest contains a non-official CLI asset URL."
    }
    if ($entry.sha256 -notmatch '^[0-9a-fA-F]{64}$') {
        throw "Official release manifest contains an invalid SHA-256."
    }

    Write-Host "Downloading $($entry.url)"
    Invoke-WebRequest -Uri $entry.url -OutFile $tmp
    $actual = (Get-FileHash -Algorithm SHA256 -Path $tmp).Hash
    if ($actual -ine $entry.sha256) {
        throw "SHA-256 mismatch. Expected $($entry.sha256), got $actual."
    }

    Move-Item -Force -Path $tmp -Destination $target
    Copy-Item -Force -Path $target -Destination $compatibilityTarget
} finally {
    foreach ($path in @($manifestPath, $tmp)) {
        if (Test-Path -LiteralPath $path) {
            Remove-Item -Force -LiteralPath $path
        }
    }
}

Write-Host "Installed dccmcp to $target"
Write-Host "Installed compatibility alias to $compatibilityTarget"

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$pathParts = if ($userPath) { $userPath -split ";" } else { @() }
if ($pathParts -notcontains $InstallDir) {
    $newPath = if ($userPath) { "$userPath;$InstallDir" } else { $InstallDir }
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    Write-Host "Added $InstallDir to the user PATH. Open a new terminal before running dccmcp."
}
