<#
.SYNOPSIS
  Release build with code signing, then verifies every signature.
.DESCRIPTION
  Signs marky.exe before the installer, and the installer after it.
  Requires MARKY_SIGN_COMMAND; see scripts/sign.ps1.
  -SkipVerify is for dry runs with a dummy command that does not really sign.
#>
param([switch]$SkipVerify)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
if (-not $env:MARKY_SIGN_COMMAND) { throw 'MARKY_SIGN_COMMAND is not set; see scripts/sign.ps1.' }

Push-Location $root
try {
  cargo build --release --workspace
  if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
  & (Join-Path $PSScriptRoot 'sign.ps1') (Join-Path $root 'target\release\marky.exe')
  $installer = & (Join-Path $PSScriptRoot 'build-installer.ps1')
  & (Join-Path $PSScriptRoot 'sign.ps1') $installer
}
finally { Pop-Location }

if ($SkipVerify) { return }
$version = [regex]::Match((Get-Content (Join-Path $root 'Cargo.toml') -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
$artifacts = @(
  (Join-Path $root 'target\release\marky.exe'),
  (Join-Path $root "dist\Marky_${version}_x64-setup.exe")
)
$unsigned = 0
foreach ($artifact in $artifacts) {
  $signature = Get-AuthenticodeSignature $artifact
  $ok = $signature.Status -eq 'Valid'
  if (-not $ok) { $unsigned++ }
  "{0}  {1}: {2} ({3})" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), (Split-Path $artifact -Leaf), $signature.Status, $signature.SignerCertificate.Subject
}
exit $unsigned
