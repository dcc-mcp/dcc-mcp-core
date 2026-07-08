# Debug gateway + per-DCC REST/MCP using worktree-built dcc-mcp-cli / dcc-mcp-server.
# Usage:
#   # A) Live Maya + gateway (default 9765) — after relinking dcc-mcp-core into Maya
#   .\scripts\cli-gateway-debug.ps1 -Mode gateway
#
#   # B) Standalone server (no embedded DCC) — CI/automation smoke on example skills
#   .\scripts\cli-gateway-debug.ps1 -Mode standalone -McpPort 18765
#
param(
    [ValidateSet("gateway", "standalone")]
    [string]$Mode = "gateway",
    [string]$BaseUrl = "http://127.0.0.1:9765",
    [int]$McpPort = 18765,
    [string]$RepoRoot = ""
)

$ErrorActionPreference = "Stop"

function Quote-WindowsArgument {
    param([string]$Argument)

    if ([string]::IsNullOrEmpty($Argument)) {
        return '""'
    }
    if ($Argument -notmatch '[\s"]') {
        return $Argument
    }

    $quoted = '"'
    $backslashes = 0
    foreach ($char in $Argument.ToCharArray()) {
        if ($char -eq '\') {
            $backslashes++
            continue
        }
        if ($char -eq '"') {
            $quoted += ('\' * ($backslashes * 2 + 1)) + '"'
            $backslashes = 0
            continue
        }
        if ($backslashes -gt 0) {
            $quoted += ('\' * $backslashes)
            $backslashes = 0
        }
        $quoted += $char
    }
    if ($backslashes -gt 0) {
        $quoted += ('\' * ($backslashes * 2))
    }
    $quoted + '"'
}

function Join-WindowsArgumentList {
    param([string[]]$Arguments)

    ($Arguments | ForEach-Object { Quote-WindowsArgument $_ }) -join ' '
}

if (-not $RepoRoot) {
    $RepoRoot = (git -C $PSScriptRoot rev-parse --show-toplevel 2>$null)
    if (-not $RepoRoot) { $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path }
}

$Cli = Join-Path $RepoRoot "target\release\dcc-mcp-cli.exe"
$Server = Join-Path $RepoRoot "target\release\dcc-mcp-server.exe"

if (-not (Test-Path $Cli)) {
    Write-Host "Building dcc-mcp-cli + dcc-mcp-server..."
    Push-Location $RepoRoot
    cargo build -p dcc-mcp-cli -p dcc-mcp-server --release
    Pop-Location
}

$serverProc = $null
try {
    if ($Mode -eq "standalone") {
        $BaseUrl = "http://127.0.0.1:$McpPort"
        $skillPaths = Join-Path $RepoRoot "examples\skills"
        Write-Host "Starting standalone dcc-mcp-server on $BaseUrl (gateway disabled)..."
        # ponytail: Start-Process on Windows PowerShell 5.1 mangles array
        # ArgumentList values that contain quotes. Pass one pre-quoted command
        # line so JSON payloads survive intact.
        $serverProc = Start-Process -FilePath $Server -ArgumentList (
            Join-WindowsArgumentList @(
            "--app", "maya",
            "--gateway-port", "0",
            "--mcp-port", "$McpPort",
            "--skill-paths", $skillPaths
            )
        ) -PassThru -WindowStyle Hidden
        Start-Sleep -Seconds 2
    }

    $env:DCC_MCP_BASE_URL = $BaseUrl
    Write-Host "=== health ==="
    & $Cli health
    if ($Mode -eq "gateway") {
        Write-Host "=== list instances ==="
        & $Cli list
    }
    Write-Host "=== smoke (MCP + REST search) ==="
    & $Cli smoke
    Write-Host "=== search (scene) ==="
    & $Cli search --query "get_scene_info" --dcc-type maya 2>$null
    if ($LASTEXITCODE -ne 0) {
        & $Cli search --query "thread_probe" 2>$null
    }
}
finally {
    if ($serverProc) {
        Stop-Process -Id $serverProc.Id -Force -ErrorAction SilentlyContinue
    }
}
