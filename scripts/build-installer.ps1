<#
.SYNOPSIS
  Builds the per-user NSIS installer for Marky.
.DESCRIPTION
  Uses makensis from PATH, or the standard NSIS install folders.
  CI installs NSIS before calling this. The release build must already exist:
  cargo build --release --workspace
.EXAMPLE
  ./scripts/build-installer.ps1
#>
param([string]$Version = '')
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$cargo = Get-Content (Join-Path $root 'Cargo.toml') -Raw
if (-not $Version) { $Version = [regex]::Match($cargo, '(?m)^version = "([^"]+)"').Groups[1].Value }
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Installer version must be X.Y.Z (got '$Version')" }

$release = Join-Path $root 'target\release'
if (-not (Test-Path (Join-Path $release 'marky.exe'))) { throw 'marky.exe not found; run: cargo build --release --workspace' }

$makensis = if ($env:MAKENSIS) { $env:MAKENSIS } else { (Get-Command makensis -ErrorAction SilentlyContinue).Source }
if (-not $makensis) {
  $makensis = @(
    (Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe'),
    (Join-Path $env:ProgramFiles 'NSIS\makensis.exe'),
    (Join-Path $env:LOCALAPPDATA 'Programs\NSIS\makensis.exe')
  ) + @(Get-Item "$env:LOCALAPPDATA\*\NSIS\makensis.exe", "$env:LOCALAPPDATA\*\NSIS\Bin\makensis.exe" -ErrorAction SilentlyContinue | ForEach-Object FullName) |
    Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
}
if (-not $makensis) { throw 'makensis not found; install NSIS (choco install nsis) or set it on PATH.' }

$out = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $out | Out-Null
$version4 = "$Version.0"
& $makensis /V2 "/DVERSION=$Version" "/DVERSION4=$version4" "/DSRCDIR=$root" "/DOUTDIR=$out" (Join-Path $root 'installer\marky.nsi') | Out-Null
if ($LASTEXITCODE -ne 0) { throw "makensis failed ($LASTEXITCODE)" }
$installer = Join-Path $out "Marky_${Version}_x64-setup.exe"
if (-not (Test-Path $installer)) { throw "Installer was not produced: $installer" }
$installer
