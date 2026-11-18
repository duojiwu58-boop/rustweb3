#Requires -Version 7.0
[CmdletBinding()]
param([string]$Cargo = "$env:USERPROFILE\.cargo\bin\cargo.exe")
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$evidence = Join-Path $root 'artifacts\isolation'
New-Item -ItemType Directory -Path $evidence -Force | Out-Null
Push-Location (Join-Path $root 'rust')
try {
    & $Cargo build -p wallet-sandbox -p wallet-signer --release --locked --offline
    if ($LASTEXITCODE -ne 0) { throw 'Isolation Release build failed' }
    $probe = Join-Path $root 'rust\target\release\sandbox-probe.exe'
    $result = & $probe $evidence
    $code = $LASTEXITCODE
    $result | Set-Content -LiteralPath (Join-Path $evidence 'probe.txt') -Encoding utf8
    $result
    if ($code -ne 0) { throw "Isolation probe failed ($code)" }
    $probe = Join-Path $root 'rust\target\release\vault-session-probe.exe'
    $result = & $probe (Join-Path $root 'rust\target\release\wallet-signer.exe') (Join-Path $root 'artifacts\vault-session')
    $code = $LASTEXITCODE
    $result | Set-Content -LiteralPath (Join-Path $evidence 'vault-session.txt') -Encoding utf8
    $result
    if ($code -ne 0) { throw "Public vault session probe failed ($code)" }
} finally { Pop-Location }
