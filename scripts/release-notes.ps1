<#
.SYNOPSIS
  Prints CHANGELOG.md's section for one version (or "Unreleased"): the text a GitHub Release
  uses as its notes. Fails when the section is missing or empty.
.EXAMPLE
  ./scripts/release-notes.ps1 0.3.0
#>
param([Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = 'Stop'
$changelog = Get-Content (Join-Path (Split-Path $PSScriptRoot -Parent) 'CHANGELOG.md') -Raw
$heading = [regex]::Escape($Version)
$section = [regex]::Match($changelog, "(?ms)^## \[$heading\][^\n]*\n(.*?)(?=^## \[|\z)").Groups[1].Value.Trim()
if (-not $section) { throw "CHANGELOG.md has nothing under ## [$Version]; describe the release there first." }
$section
