<#
.SYNOPSIS
  Generates winget manifests (schema 1.12.0, multi-file) for a published release.
.DESCRIPTION
  Hashes the local NSIS installer, which must be byte-identical to the one uploaded at
  -InstallerUrl, and writes manifests\<letter>\<Publisher>\<Package>\<Version>\ ready
  for a pull request to microsoft/winget-pkgs. Validates them with `winget validate`.
.EXAMPLE
  ./scripts/winget-manifest.ps1 -InstallerUrl https://github.com/<owner>/marky/releases/download/v0.1.0/Marky_0.1.0_x64-setup.exe
#>
param(
  [Parameter(Mandatory)][string]$InstallerUrl,
  [string]$License = 'MIT',
  [string]$PackageIdentifier = 'VikasSingh.Marky',
  [string]$Publisher = 'Vikas Singh',
  [string]$PackageUrl = '',
  [string]$OutDir = (Join-Path (Split-Path $PSScriptRoot -Parent) 'dist-winget')
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$schema = '1.12.0'

$version = (Get-Content (Join-Path $root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version
$installer = Get-ChildItem (Join-Path $root "src-tauri\target\release\bundle\nsis\*_${version}_x64-setup.exe") | Select-Object -First 1
if (-not $installer) { throw "No NSIS installer for version $version; run a full 'npx tauri build' first." }
$sha256 = (Get-FileHash $installer.FullName -Algorithm SHA256).Hash

$publisherPart, $packagePart = $PackageIdentifier.Split('.', 2)
$dir = Join-Path $OutDir "manifests\$($publisherPart.Substring(0,1).ToLower())\$publisherPart\$packagePart\$version"
New-Item -ItemType Directory -Force $dir | Out-Null

$header = { param($type) "# yaml-language-server: `$schema=https://aka.ms/winget-manifest.$type.$schema.schema.json`n" }

Set-Content (Join-Path $dir "$PackageIdentifier.yaml") ((& $header 'version') + @"
PackageIdentifier: $PackageIdentifier
PackageVersion: $version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $schema
"@)

# Tauri's NSIS installer is per-user (no admin prompt) and supports /S for silent installs.
Set-Content (Join-Path $dir "$PackageIdentifier.installer.yaml") ((& $header 'installer') + @"
PackageIdentifier: $PackageIdentifier
PackageVersion: $version
InstallerType: nullsoft
Scope: user
InstallModes:
- interactive
- silent
UpgradeBehavior: install
FileExtensions:
- md
- markdown
Installers:
- Architecture: x64
  InstallerUrl: $InstallerUrl
  InstallerSha256: $sha256
ManifestType: installer
ManifestVersion: $schema
"@)

$packageUrlLine = if ($PackageUrl) { "PackageUrl: $PackageUrl`n" } else { '' }
Set-Content (Join-Path $dir "$PackageIdentifier.locale.en-US.yaml") ((& $header 'defaultLocale') + @"
PackageIdentifier: $PackageIdentifier
PackageVersion: $version
PackageLocale: en-US
Publisher: $Publisher
PackageName: Marky
$($packageUrlLine)License: $License
ShortDescription: A fast, lightweight Markdown editor with live preview.
Description: Marky opens Markdown files in well under a second, edits them with syntax highlighting and a live side-by-side preview, and idles around 100 MB of RAM by reusing the WebView2 runtime built into Windows.
Tags:
- markdown
- editor
- text-editor
- tauri
ManifestType: defaultLocale
ManifestVersion: $schema
"@)

Write-Host "Manifests written to $dir"
winget validate --manifest $dir
if ($LASTEXITCODE -ne 0) { throw "winget validate failed ($LASTEXITCODE)" }
