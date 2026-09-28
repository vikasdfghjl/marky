<#
.SYNOPSIS
  Release build with code signing, then verifies every signature.
.DESCRIPTION
  Merges a signCommand into the Tauri config for this build only (plain `npx tauri build`
  stays unsigned for local work). Requires MARKY_SIGN_COMMAND; see scripts/sign.ps1.
  -SkipVerify is for dry runs with a dummy command that does not really sign.
#>
param([switch]$SkipVerify)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
if (-not $env:MARKY_SIGN_COMMAND) { throw 'MARKY_SIGN_COMMAND is not set; see scripts/sign.ps1.' }

$signHook = Join-Path $PSScriptRoot 'sign.ps1'
$signingConfig = @{
  bundle = @{ windows = @{ signCommand = @{ cmd = 'pwsh'; args = @('-NoProfile', '-File', $signHook, '%1') } } }
} | ConvertTo-Json -Depth 6 -Compress
$configFile = Join-Path $env:TEMP 'marky-signing.conf.json'
Set-Content $configFile $signingConfig

Push-Location $root
try {
  npx tauri build --config $configFile
  if ($LASTEXITCODE -ne 0) { throw "tauri build failed ($LASTEXITCODE)" }
}
finally { Pop-Location }

if ($SkipVerify) { return }
$release = Join-Path $root 'src-tauri\target\release'
$artifacts = @((Join-Path $release 'marky.exe')) + (Get-ChildItem (Join-Path $release 'bundle\nsis\*.exe')).FullName
$unsigned = 0
foreach ($artifact in $artifacts) {
  $signature = Get-AuthenticodeSignature $artifact
  $ok = $signature.Status -eq 'Valid'
  if (-not $ok) { $unsigned++ }
  "{0}  {1}: {2} ({3})" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), (Split-Path $artifact -Leaf), $signature.Status, $signature.SignerCertificate.Subject
}
exit $unsigned
