#Requires -Version 7.0
[CmdletBinding()]
param(
    [string]$Cargo = "$env:USERPROFILE\.cargo\bin\cargo.exe",
    [string]$DelphiRoot = 'C:\Program Files (x86)\Embarcadero\Studio\23.0'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$rust = Join-Path $root 'rust'
$output = Join-Path $root 'artifacts\m0a\bin'
$units = Join-Path $root 'artifacts\m0a\dcu'
$compiler = Join-Path $DelphiRoot 'bin\dcc64.exe'
foreach ($tool in @($Cargo, $compiler)) {
    if (-not (Test-Path -LiteralPath $tool -PathType Leaf)) { throw "Missing tool: $tool" }
}
New-Item -ItemType Directory -Path $output, $units -Force | Out-Null
$env:PATH = "$(Split-Path -Parent $Cargo);$env:PATH"
Push-Location $rust
try {
    & $Cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { throw 'cargo fmt failed' }
    & $Cargo test --workspace --locked --offline
    if ($LASTEXITCODE -ne 0) { throw 'cargo test failed' }
    & $Cargo clippy --workspace --all-targets --locked --offline -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'cargo clippy failed' }
    foreach ($package in @('wallet-ffi', 'wallet-signer', 'wallet-network')) {
        & $Cargo build -p $package --release --locked --offline --target x86_64-pc-windows-msvc
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed: $package" }
        & $Cargo tree -p $package -e features --locked --offline
        if ($LASTEXITCODE -ne 0) { throw "cargo tree failed: $package" }
    }
    & $Cargo build -p wallet-core -p chain-evm -p chain-tron -p wallet-secrets --release --locked --offline --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Offline chain library Release build failed' }
    foreach ($name in @('web3rust.dll', 'wallet-signer.exe', 'wallet-network.exe')) {
        Copy-Item -LiteralPath (Join-Path $rust "target\x86_64-pc-windows-msvc\release\$name") -Destination $output
    }
    $faultPeer = Join-Path $root 'artifacts\m0a\mock-peer.exe'
    & (Join-Path (Split-Path -Parent $Cargo) 'rustc.exe') (Join-Path $root 'tests\fixtures\mock-peer.rs') '-o' $faultPeer
    if ($LASTEXITCODE -ne 0) { throw 'Fault peer build failed' }
    $previousPeer = $env:W3_TEST_FAULT_PEER
    try {
        $env:W3_TEST_FAULT_PEER = $faultPeer
        & $Cargo test -p wallet-ffi --test lifecycle --locked --offline -- --ignored
        if ($LASTEXITCODE -ne 0) { throw 'Process failure tests failed' }
    } finally { $env:W3_TEST_FAULT_PEER = $previousPeer }
} finally { Pop-Location }
Push-Location (Join-Path $root 'desktop')
try {
    & $compiler '-B' '-Q' "-E$output" "-N0$units" "-U$DelphiRoot\lib\win64\release" 'AbiSmoke.dpr'
    if ($LASTEXITCODE -ne 0) { throw 'Delphi ABI smoke build failed' }
    & $compiler '-B' '-Q' "-E$output" "-N0$units" "-U$DelphiRoot\lib\win64\release" 'Web3Rust.dpr'
    if ($LASTEXITCODE -ne 0) { throw 'Delphi GUI build failed' }
} finally { Pop-Location }
& (Join-Path $output 'AbiSmoke.exe')
if ($LASTEXITCODE -ne 0) { throw 'Delphi ABI smoke failed' }
$guiTest = Start-Process -FilePath (Join-Path $output 'Web3Rust.exe') -ArgumentList '--self-test' -WindowStyle Hidden -PassThru
if (-not $guiTest.WaitForExit(30000)) { throw "GUI self-test timed out, PID $($guiTest.Id)" }
if ($guiTest.ExitCode -ne 0) { throw "GUI self-test failed: exit $($guiTest.ExitCode), see artifacts/m0a/gui/self-test.txt" }
Get-Content -LiteralPath (Join-Path $root 'artifacts\m0a\gui\self-test.txt')
$leftovers = Get-Process -Name 'wallet-signer','wallet-network' -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and (Split-Path -Parent $_.Path) -eq $output }
if ($leftovers) { throw 'Task-owned child process remained after smoke test' }
$hashes = Get-ChildItem -LiteralPath $output -File | Get-FileHash -Algorithm SHA256
$hashes | Select-Object Algorithm, Hash, Path | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root 'artifacts\m0a\hashes.json') -Encoding utf8
$hashes
