<#
.SYNOPSIS
  Bumps the version, commits, and creates the annotated tag vX.Y.Z. Does not push.
.DESCRIPTION
  package.json is the version source (tauri.conf.json points at it); Cargo.toml is kept
  in sync. Pushing the tag triggers .github/workflows/release.yml, which builds and
  publishes the GitHub Release. 0.x versions are published as pre-releases (beta).
.EXAMPLE
  ./scripts/release.ps1 -Version 0.2.0
  git push origin main --follow-tags
#>
param([Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
  if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must be plain X.Y.Z (got '$Version'); pre-release status comes from 0.x." }
  if (git status --porcelain) { throw 'Working tree is not clean; commit or stash first.' }
  $tag = "v$Version"
  if (git tag --list $tag) { throw "Tag $tag already exists." }

  $current = (Get-Content package.json -Raw | ConvertFrom-Json).version
  if ([version]$Version -le [version]$current) { throw "Version $Version must be greater than the current $current." }

  npm version $Version --no-git-tag-version | Out-Null
  $cargoToml = 'src-tauri/Cargo.toml'
  $cargo = Get-Content $cargoToml -Raw
  $cargo = [regex]::Replace($cargo, '(?m)^version = "[^"]+"', "version = `"$Version`"", 1)
  Set-Content $cargoToml $cargo -NoNewline
  cargo update --manifest-path $cargoToml --package marky --offline --quiet
  if ($LASTEXITCODE -ne 0) { throw 'cargo update failed' }

  git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock
  git commit --quiet -m "Release $tag"
  git tag -a $tag -m "Marky $Version"
  Write-Host "Created $tag. Publish it with: git push origin HEAD --follow-tags"
}
finally { Pop-Location }
