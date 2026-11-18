#Requires -Version 7.0
[CmdletBinding()]
param(
    [switch]$Refresh,
    [string]$Cargo = "$env:USERPROFILE\.cargo\bin\cargo.exe"
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$evidence = Join-Path $root 'artifacts\m0a'
New-Item -ItemType Directory -Path $evidence -Force | Out-Null
$oldHttp = $env:HTTP_PROXY
$oldHttps = $env:HTTPS_PROXY
$oldCargoProxy = $env:CARGO_HTTP_PROXY
Push-Location (Join-Path $root 'rust')
try {
    $raw = & $Cargo metadata --locked --offline --format-version 1 --filter-platform x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed' }
    $metadata = $raw | ConvertFrom-Json
    $inventory = foreach ($package in ($metadata.packages | Where-Object source | Sort-Object name,version)) {
        $directory = Split-Path -Parent $package.manifest_path
        $notices = Get-ChildItem -LiteralPath $directory -File | Where-Object { $_.Name -match '^(LICENSE|COPYING|COPYRIGHT|NOTICE)' }
        [pscustomobject]@{
            name = $package.name
            version = $package.version
            license = $package.license
            repository = $package.repository
            source = $package.source
            build_script = @($package.targets | Where-Object { $_.kind -contains 'custom-build' } | ForEach-Object { Split-Path -Leaf $_.src_path })
            proc_macro = @($package.targets | Where-Object { $_.kind -contains 'proc-macro' } | ForEach-Object name)
            notices = @($notices | ForEach-Object { [pscustomobject]@{ file = $_.Name; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash } })
        }
    }
    $inventory | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $root 'docs\dependency-inventory.json') -Encoding utf8
    # Network transport is isolated from all vault code; compare its reviewed closure.
    foreach ($package in @('wallet-network')) {
        $tree = & $Cargo tree -p $package -e normal,build --locked --offline --prefix none
        if ($LASTEXITCODE -ne 0) { throw "Dependency tree failed: $package" }
        $actual = @($tree | ForEach-Object { ($_ -split ' ')[0..1] -join ' ' } | Sort-Object -Unique)
        $baseline = @(Get-Content -LiteralPath (Join-Path $root 'docs\network-dependencies.txt'))
        if (Compare-Object $baseline $actual) { throw 'Network dependency closure changed; review before updating baseline' }
        if ($actual -match '^wallet-secrets ') { throw 'Network must not depend on vault code' }
    }
    $tree = & $Cargo tree -p wallet-signer -e normal,build --locked --offline --prefix none
    if ($LASTEXITCODE -ne 0) { throw 'Signer dependency tree failed' }
    $actual = @($tree | ForEach-Object { ($_ -split ' ')[0..1] -join ' ' } | Sort-Object -Unique)
    $baseline = @(Get-Content -LiteralPath (Join-Path $root 'docs\signer-dependencies.txt'))
    if (Compare-Object $baseline $actual) { throw 'Signer dependency closure changed; review before updating baseline' }
    $tree = & $Cargo tree -p wallet-ffi -e normal,build --locked --offline --prefix none
    if ($LASTEXITCODE -ne 0) { throw 'FFI dependency tree failed' }
    $actual = @($tree | ForEach-Object { ($_ -split ' ')[0..1] -join ' ' } | Sort-Object -Unique)
    $baseline = @(Get-Content -LiteralPath (Join-Path $root 'docs\ffi-dependencies.txt'))
    if (Compare-Object $baseline $actual) { throw 'FFI dependency closure changed; review before updating baseline' }
    $arguments = @('audit','--json')
    if ($Refresh) {
        $env:HTTP_PROXY = 'http://127.0.0.1:1080'
        $env:HTTPS_PROXY = 'http://127.0.0.1:1080'
        $env:CARGO_HTTP_PROXY = 'http://127.0.0.1:1080'
    } else { $arguments += '--no-fetch' }
    $audit = & $Cargo @arguments
    $auditExit = $LASTEXITCODE
    $audit | Set-Content -LiteralPath (Join-Path $evidence 'dependency-audit.json') -Encoding utf8
    if ($auditExit -ne 0) { throw "cargo audit failed ($auditExit); inspect artifacts/m0a/dependency-audit.json" }
    $report = $audit | ConvertFrom-Json
    Write-Output "Audit passed: $($report.vulnerabilities.count) advisories; $(@($inventory).Count) registry packages inventoried."
} finally {
    Pop-Location
    $env:HTTP_PROXY = $oldHttp
    $env:HTTPS_PROXY = $oldHttps
    $env:CARGO_HTTP_PROXY = $oldCargoProxy
}
