param(
    [string] $Version = $env:DCC_MCP_VERSION,
    [string] $InstallDir = $env:DCC_MCP_INSTALL_DIR
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$officialReleases = "https://github.com/dcc-mcp/dcc-mcp-core/releases"
$platform = "windows-x86_64"
$asset = "dcc-mcp-cli-windows-x86_64.exe"

if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = "latest"
}
if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    $InstallDir = Join-Path $env:LOCALAPPDATA "dcc-mcp\bin"
}

if ($Version -eq "latest") {
    $requestedVersion = $null
    $manifestUrl = "$officialReleases/latest/download/dcc-mcp-update-manifest-$platform.json"
} else {
    if ($Version -notmatch '^v?([0-9]+\.[0-9]+\.[0-9]+([.+-][0-9A-Za-z.-]+)?)$') {
        throw "Invalid release version: $Version"
    }
    $requestedVersion = $Matches[1]
    $manifestUrl = "$officialReleases/download/v$requestedVersion/dcc-mcp-update-manifest-$platform.json"
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$target = Join-Path $InstallDir "dcc-mcp-cli.exe"
$manifestTmp = Join-Path $InstallDir (".dcc-mcp-manifest-" + [System.Guid]::NewGuid() + ".json")
$binaryTmp = Join-Path $InstallDir (".dcc-mcp-cli-" + [System.Guid]::NewGuid() + ".exe")

try {
    Write-Host "Downloading $manifestUrl"
    Invoke-WebRequest -Uri $manifestUrl -OutFile $manifestTmp

    $manifestText = Get-Content -LiteralPath $manifestTmp -Raw
    if ([regex]::Matches($manifestText, '"dcc-mcp-cli"\s*:').Count -ne 1) {
        throw "Official release manifest must contain one dcc-mcp-cli entry"
    }
    try {
        $manifest = $manifestText | ConvertFrom-Json
    } catch {
        throw "Official release manifest is not valid JSON: $($_.Exception.Message)"
    }
    $entry = $manifest.'dcc-mcp-cli'
    if ($null -eq $entry) {
        throw "Official release manifest is missing dcc-mcp-cli"
    }

    $manifestVersion = [string] $entry.version
    $assetUrl = [string] $entry.url
    $expectedSha256 = [string] $entry.sha256
    if ($manifestVersion -notmatch '^[0-9]+\.[0-9]+\.[0-9]+([.+-][0-9A-Za-z.-]+)?$') {
        throw "Official release manifest contains an invalid dcc-mcp-cli version"
    }
    if ($Version -ne "latest" -and $manifestVersion -ne $requestedVersion) {
        throw "Official release manifest version does not match requested version $requestedVersion"
    }

    $expectedUrl = "$officialReleases/download/v$manifestVersion/$asset"
    if ($assetUrl -cne $expectedUrl) {
        throw "Official release manifest contains a non-official dcc-mcp-cli URL"
    }
    if ($expectedSha256 -notmatch '^[0-9A-Fa-f]{64}$') {
        throw "Official release manifest contains an invalid SHA-256"
    }

    Write-Host "Downloading $assetUrl"
    Invoke-WebRequest -Uri $assetUrl -OutFile $binaryTmp
    $actualSha256 = (Get-FileHash -LiteralPath $binaryTmp -Algorithm SHA256).Hash
    if ($actualSha256 -ine $expectedSha256) {
        throw "dcc-mcp-cli SHA-256 does not match the official release manifest"
    }

    if (Test-Path -LiteralPath $target) {
        [System.IO.File]::Replace($binaryTmp, $target, $null)
    } else {
        [System.IO.File]::Move($binaryTmp, $target)
    }
} finally {
    foreach ($path in @($manifestTmp, $binaryTmp)) {
        if (Test-Path -LiteralPath $path) {
            Remove-Item -LiteralPath $path -Force
        }
    }
}

Write-Host "Installed verified dcc-mcp-cli $manifestVersion to $target"

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
