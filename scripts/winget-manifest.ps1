<#
.SYNOPSIS
  Generates winget manifests (schema 1.12.0, multi-file) for a published release.
.DESCRIPTION
  Hashes the local NSIS installer, which must be byte-identical to the one uploaded at
  -InstallerUrl, and writes manifests\<letter>\<Publisher>\<Package>\<Version>\ ready
  for a pull request to microsoft/winget-pkgs. Validates them with `winget validate`.
.EXAMPLE
  ./scripts/winget-manifest.ps1 -InstallerUrl https://github.com/<owner>/marky/releases/download/v0.3.3/Marky_0.3.3_x64-setup.exe
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

$version = [regex]::Match((Get-Content (Join-Path $root 'Cargo.toml') -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
$installer = Join-Path $root "dist\Marky_${version}_x64-setup.exe"
if (-not (Test-Path $installer)) { throw "No installer for version $version; run scripts/build-installer.ps1 first." }
$sha256 = (Get-FileHash $installer -Algorithm SHA256).Hash

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

# The NSIS installer is per-user (no admin prompt) and supports /S for silent installs.
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
Description: Marky opens Markdown files in well under a second, edits them with syntax highlighting and a live side-by-side preview, and uses about 16 MB of RAM.
Tags:
- markdown
- editor
- text-editor
- rust
ManifestType: defaultLocale
ManifestVersion: $schema
"@)

Write-Host "Manifests written to $dir"
winget validate --manifest $dir
if ($LASTEXITCODE -ne 0) { throw "winget validate failed ($LASTEXITCODE)" }
