#Requires -Version 7.0
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$source = Join-Path $root 'artifacts\secure-desktop'
$destination = Join-Path $root 'artifacts\bnb-live'
foreach ($evidence in @('secure-local-tests.txt','secure-network-test.txt')) {
    if (-not (Test-Path -LiteralPath (Join-Path $source $evidence))) { throw "Missing verification: $evidence" }
}
New-Item -ItemType Directory -Path $destination -Force | Out-Null
if (-not (Test-Path -LiteralPath (Join-Path $destination 'network.ini'))) {
    Copy-Item -LiteralPath (Join-Path $root 'config/network.ini') -Destination (Join-Path $destination 'network.ini')
}
$files = @('wallet-launcher.exe','SecureWallet.exe','web3rust.dll','wallet-signer.exe','wallet-network.exe','wallet.ico')
foreach ($file in $files) {
    Copy-Item -LiteralPath (Join-Path $source $file) -Destination (Join-Path $destination $file)
}
& (Join-Path $PSScriptRoot 'initialize-runtime-access.ps1') -Directory $destination
Copy-Item -LiteralPath (Join-Path $root 'docs\real-bnb-test.md') -Destination (Join-Path $destination 'READ-ME.md')
Copy-Item -LiteralPath (Join-Path $root 'docs\tron.md') -Destination (Join-Path $destination 'TRON.md')
$manifest = foreach ($file in $files) {
    $hash = Get-FileHash -LiteralPath (Join-Path $destination $file) -Algorithm SHA256
    [pscustomobject]@{file=$file; sha256=$hash.Hash; bytes=(Get-Item -LiteralPath $hash.Path).Length}
}
$manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $destination 'SHA256.json') -Encoding utf8
$manifest
Write-Output "Ready: $destination\wallet-launcher.exe"
