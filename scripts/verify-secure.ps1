#Requires -Version 7.0
[CmdletBinding()]
param([switch]$Live, [string]$Cargo = "$env:USERPROFILE\.cargo\bin\cargo.exe", [string]$DelphiRoot = 'C:\Program Files (x86)\Embarcadero\Studio\23.0')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$output = Join-Path $root 'artifacts\secure-desktop'
$units = Join-Path $root 'artifacts\secure-desktop\dcu'
New-Item -ItemType Directory -Path $output,$units -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $root 'desktop/assets/wallet.ico') -Destination $output
if (-not (Test-Path -LiteralPath (Join-Path $output 'network.ini'))) {
    Copy-Item -LiteralPath (Join-Path $root 'config/network.ini') -Destination (Join-Path $output 'network.ini')
}
Push-Location (Join-Path $root 'rust')
try {
    & $Cargo build -p wallet-ffi -p wallet-signer -p wallet-network -p wallet-sandbox --release --locked --offline
    if ($LASTEXITCODE -ne 0) { throw 'Secure Rust build failed' }
    foreach ($name in @('web3rust.dll','wallet-signer.exe','wallet-network.exe','wallet-launcher.exe')) {
        Copy-Item -LiteralPath (Join-Path $root "rust\target\release\$name") -Destination $output
    }
} finally { Pop-Location }
Push-Location (Join-Path $root 'desktop')
try {
    Push-Location (Join-Path $root 'desktop/assets')
    try {
        & (Join-Path $DelphiRoot 'bin/brcc32.exe') '-fowallet.res' 'wallet.rc'
        if ($LASTEXITCODE -ne 0) { throw 'Wallet icon resource build failed' }
    } finally { Pop-Location }
    foreach ($project in @('SecureAbiSmoke.dpr','SecureWallet.dpr')) {
        & (Join-Path $DelphiRoot 'bin\dcc64.exe') '-B' '-Q' "-E$output" "-N0$units" "-U$DelphiRoot\lib\win64\release" $project
        if ($LASTEXITCODE -ne 0) { throw "Secure Delphi build failed: $project" }
    }
} finally { Pop-Location }
$report = & (Join-Path $root 'rust\target\release\secure-desktop-probe.exe') $output
$code = $LASTEXITCODE
$report | Set-Content -LiteralPath (Join-Path $output 'secure-local-tests.txt') -Encoding utf8
$report
if ($code -ne 0) { throw "Contained ABI/VCL tests failed ($code)" }
if ($Live) {
    $launcher = Start-Process -FilePath (Join-Path $output 'wallet-launcher.exe') -ArgumentList '--self-test' -WindowStyle Hidden -PassThru
    if (-not $launcher.WaitForExit(120000)) { throw "Read-only self-test timeout, PID $($launcher.Id)" }
    if ($launcher.ExitCode -ne 0) { throw 'GUI/broker/mainnet read-only test failed' }
    'PASS: asynchronous isolated VCL/public broker/one-shot internet worker; mainnet read-only, no broadcast' |
        Tee-Object -FilePath (Join-Path $output 'secure-network-test.txt')
}
Get-ChildItem -LiteralPath $output -File | Where-Object Extension -in '.exe','.dll' |
    Get-FileHash -Algorithm SHA256 | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $output 'hashes.json') -Encoding utf8
