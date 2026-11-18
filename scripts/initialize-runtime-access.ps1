#Requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Directory)
$ErrorActionPreference = 'Stop'
$directoryPath = (Resolve-Path -LiteralPath $Directory).Path
# Provision the destination, not the build directory: Copy-Item does not carry
# the source AppContainer ACLs to newly created destination files.
$offlineFiles = @('SecureWallet.exe', 'web3rust.dll', 'wallet-signer.exe')
$onlineFiles = @('wallet-network.exe')
foreach ($file in ($offlineFiles + $onlineFiles)) {
    if (-not (Test-Path -LiteralPath (Join-Path $directoryPath $file) -PathType Leaf)) {
        throw "Missing runtime file: $file"
    }
}
if (-not ('Web3Rust.RuntimeAccess' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Security.Principal;
namespace Web3Rust {
    public static class RuntimeAccess {
        [DllImport("userenv.dll", CharSet = CharSet.Unicode)]
        static extern int DeriveAppContainerSidFromAppContainerName(string name, out IntPtr sid);
        [DllImport("advapi32.dll")]
        static extern IntPtr FreeSid(IntPtr sid);
        public static SecurityIdentifier Sid(string name) {
            IntPtr sid;
            Marshal.ThrowExceptionForHR(DeriveAppContainerSidFromAppContainerName(name, out sid));
            try { return new SecurityIdentifier(sid); }
            finally { FreeSid(sid); }
        }
    }
}
'@
}
foreach ($name in @('Web3Rust.Wallet.v1', 'Web3Rust.DesktopPublicTest.v2', 'Web3Rust.Network.v1', 'Web3Rust.ProxyNetwork.v1')) {
    $sid = [Web3Rust.RuntimeAccess]::Sid($name)
    $files = if ($name -in @('Web3Rust.Wallet.v1', 'Web3Rust.DesktopPublicTest.v2')) { $offlineFiles } else { $onlineFiles }
    foreach ($target in (@($directoryPath) + @($files | ForEach-Object { Join-Path $directoryPath $_ }))) {
        $acl = Get-Acl -LiteralPath $target
        $rule = [Security.AccessControl.FileSystemAccessRule]::new(
            $sid, [Security.AccessControl.FileSystemRights]::ReadAndExecute,
            [Security.AccessControl.AccessControlType]::Allow)
        # No inheritance and no write access; preserve existing owners and rules.
        $acl.AddAccessRule($rule)
        Set-Acl -LiteralPath $target -AclObject $acl
        $verified = (Get-Acl -LiteralPath $target).GetAccessRules($true, $false, [Security.Principal.SecurityIdentifier]) |
            Where-Object { $_.IdentityReference -eq $sid -and $_.AccessControlType -eq 'Allow' -and
                ($_.FileSystemRights -band $rule.FileSystemRights) -eq $rule.FileSystemRights }
        if (-not $verified) { throw "Runtime permission verification failed: $name / $target" }
    }
    Write-Output "Read/execute configured: $name"
}
