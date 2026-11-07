<#
.SYNOPSIS
  Packs the portable ZIP (marky.exe + marky-diagrams.exe + LICENSE + THIRD_PARTY_NOTICES) from a release build.
.OUTPUTS
  Path of the ZIP: marky-<version>-portable-x64.zip in the repo root.
#>
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$version = [regex]::Match((Get-Content (Join-Path $root 'Cargo.toml') -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
$release = Join-Path $root 'target\release'
foreach ($exe in 'marky.exe', 'marky-diagrams.exe') {
  if (-not (Test-Path (Join-Path $release $exe))) { throw "$exe not found; run: cargo build --release --workspace" }
}

$staging = Join-Path $env:TEMP "marky-portable-$version"
Remove-Item $staging -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $staging | Out-Null
Copy-Item (Join-Path $release 'marky.exe') $staging
Copy-Item (Join-Path $release 'marky-diagrams.exe') $staging
Copy-Item (Join-Path $root 'LICENSE') (Join-Path $staging 'LICENSE.txt')
Copy-Item (Join-Path $root 'THIRD_PARTY_NOTICES.txt') $staging

$zip = Join-Path $root "marky-$version-portable-x64.zip"
Compress-Archive -Path (Join-Path $staging '*') -DestinationPath $zip -Force
$zip
