param(
    [string] $Version = $env:DCC_MCP_VERSION,
    [string] $InstallDir = $env:DCC_MCP_INSTALL_DIR,
    [string] $Repo = $env:DCC_MCP_REPO
)

# One-line install:
# powershell -c "irm https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-cli/main/scripts/install-cli.ps1 | iex"

$ErrorActionPreference = "Stop"
$allowUnsigned = $env:DCC_MCP_ALLOW_UNSIGNED_DOWNLOADS -match "^(1|true|yes)$"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = "latest"
}
if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    $InstallDir = Join-Path $env:LOCALAPPDATA "dcc-mcp\bin"
}
if ([string]::IsNullOrWhiteSpace($Repo)) {
    $Repo = "dcc-mcp/dcc-mcp-cli"
}

$asset = "dccmcp-windows-x86_64.exe"
$legacyAsset = "dcc-mcp-cli-windows-x86_64.exe"

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$target = Join-Path $InstallDir "dccmcp.exe"
$compatibilityTarget = Join-Path $InstallDir "dcc-mcp-cli.exe"
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("dccmcp-" + [System.Guid]::NewGuid() + ".exe")

try {
    $downloadedUrl = $null
    foreach ($candidate in @($asset, $legacyAsset)) {
        if ($Version -eq "latest") {
            $candidateUrl = "https://github.com/$Repo/releases/latest/download/$candidate"
        } else {
            $candidateUrl = "https://github.com/$Repo/releases/download/$Version/$candidate"
        }
        try {
            Write-Host "Downloading $candidateUrl"
            Invoke-WebRequest -Uri $candidateUrl -OutFile $tmp
            $downloadedUrl = $candidateUrl
            break
        } catch {
            Write-Warning "Asset unavailable: $candidateUrl"
        }
    }
    if (-not $downloadedUrl) {
        throw "No DCCMCP or dcc-mcp-cli release asset was found."
    }

    $checksumPath = "$tmp.sha256"
    try {
        $checksumAvailable = $false
        try {
            Invoke-WebRequest -Uri "$downloadedUrl.sha256" -OutFile $checksumPath
            $checksumAvailable = $true
        } catch {
            if (-not $allowUnsigned) {
                throw "Release checksum verification failed for $downloadedUrl. $($_.Exception.Message) Set DCC_MCP_ALLOW_UNSIGNED_DOWNLOADS=1 only for a trusted legacy mirror."
            }
            Write-Warning "Checksum sidecar unavailable or invalid. Continuing by explicit opt-out."
        }
        if ($checksumAvailable) {
            $expected = (Get-Content -Raw -Path $checksumPath).Trim() -split "\s+" | Select-Object -First 1
            if ($expected -notmatch "^[0-9a-fA-F]{64}$") {
                throw "Checksum sidecar does not contain a SHA-256 digest."
            }
            $actual = (Get-FileHash -Algorithm SHA256 -Path $tmp).Hash
            if ($actual -ine $expected) {
                throw "SHA-256 mismatch. Expected $expected, got $actual."
            }
        }
    } finally {
        if (Test-Path $checksumPath) {
            Remove-Item -Force $checksumPath
        }
    }

    Move-Item -Force -Path $tmp -Destination $target
    Copy-Item -Force -Path $target -Destination $compatibilityTarget
} finally {
    if (Test-Path $tmp) {
        Remove-Item -Force $tmp
    }
}

Write-Host "Installed dccmcp to $target"
Write-Host "Installed compatibility alias to $compatibilityTarget"

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$pathParts = @()
if ($userPath) {
    $pathParts = $userPath -split ";"
}

if ($pathParts -notcontains $InstallDir) {
    $newPath = if ($userPath) { "$userPath;$InstallDir" } else { $InstallDir }
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    Write-Host "Added $InstallDir to the user PATH. Open a new terminal before running dccmcp."
}
