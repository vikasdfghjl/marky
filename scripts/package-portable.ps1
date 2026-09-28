<#
.SYNOPSIS
  Packs the portable ZIP (marky.exe + LICENSE.txt + THIRD_PARTY_NOTICES.txt) from a release build.
.OUTPUTS
  Path of the ZIP: marky-<version>-portable-x64.zip in the repo root.
#>
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$version = (Get-Content (Join-Path $root 'package.json') -Raw | ConvertFrom-Json).version
$exe = Join-Path $root 'src-tauri\target\release\marky.exe'
if (-not (Test-Path $exe)) { throw 'Release build not found. Run: npx tauri build' }

$staging = Join-Path $env:TEMP "marky-portable-$version"
Remove-Item $staging -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $staging | Out-Null
Copy-Item $exe $staging
Copy-Item (Join-Path $root 'LICENSE') (Join-Path $staging 'LICENSE.txt')
Copy-Item (Join-Path $root 'THIRD_PARTY_NOTICES.txt') $staging

$zip = Join-Path $root "marky-$version-portable-x64.zip"
Compress-Archive -Path (Join-Path $staging '*') -DestinationPath $zip -Force
$zip
