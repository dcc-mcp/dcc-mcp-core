# Probe DCC-MCP gateway (Windows PowerShell entry point).
$ErrorActionPreference = "Stop"
$Dir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Py = if (Get-Command py -ErrorAction SilentlyContinue) { "py", "-3" } else { "python" }
& @Py (Join-Path $Dir "check_gateway.py") @args
exit $LASTEXITCODE
