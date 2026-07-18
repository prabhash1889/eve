# Local-only: self-sign the Store MSIX and install it for sideload testing.
#
# This is NOT for the Microsoft Store - the Store re-signs on submission and
# discards this signature. The only coupling is identity: the cert SUBJECT must
# equal the MSIX manifest's Publisher, so pack the MSIX with a matching
# MSIX_PUBLISHER first (this script defaults to "CN=Eve Local Test").
#
# Trusting the cert and installing need ADMIN - run from an elevated PowerShell:
#   powershell -ExecutionPolicy Bypass -File packaging\local-sign-install.ps1
#
# Uninstall afterwards with:
#   Get-AppxPackage *Eve* | Remove-AppxPackage

param(
  [string]$Msix = "build/0.1.4.0/Eve-0.1.4.0-store.msix",
  [string]$Publisher = "CN=Eve Local Test",
  [string]$Password = "eve-local-test"
)

$ErrorActionPreference = "Stop"
$pfx = Join-Path $env:TEMP "eve-local-test.pfx"
$cer = Join-Path $env:TEMP "eve-local-test.cer"

if (-not (Test-Path $Msix)) { throw "MSIX not found: $Msix (run npm run build:msix first)" }

# 1. Self-signed code-signing cert whose subject matches the MSIX Publisher.
$cert = Get-ChildItem Cert:\CurrentUser\My |
  Where-Object { $_.Subject -eq $Publisher } | Select-Object -First 1
if (-not $cert) {
  Write-Host "Creating self-signed cert $Publisher"
  $cert = New-SelfSignedCertificate -Type Custom -Subject $Publisher `
    -KeyUsage DigitalSignature -FriendlyName "Eve Local Test" `
    -CertStoreLocation "Cert:\CurrentUser\My" `
    -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
}
$secure = ConvertTo-SecureString -String $Password -Force -AsPlainText
Export-PfxCertificate -Cert $cert -FilePath $pfx -Password $secure | Out-Null
Export-Certificate -Cert $cert -FilePath $cer | Out-Null

# 2. Sign the MSIX (signtool from the newest installed Windows SDK).
$signtool = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe" |
  Sort-Object FullName -Descending | Select-Object -First 1
if (-not $signtool) { throw "signtool.exe not found - install the Windows SDK." }
& $signtool.FullName sign /fd SHA256 /f $pfx /p $Password $Msix
if ($LASTEXITCODE -ne 0) { throw "signtool failed" }

# 3. Trust the cert (LocalMachine - needs admin) so Windows accepts the package.
Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople | Out-Null

# 4. Install for the current user.
Add-AppxPackage -Path $Msix
Write-Host "Installed. Launch Eve from the Start menu, or:"
Write-Host "  Get-AppxPackage *Eve* | Format-List Name, PackageFullName, InstallLocation"
