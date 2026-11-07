<#
.SYNOPSIS
  Fails when the release artifacts exceed the hard limits (release-blocking budgets).
.EXAMPLE
  cargo build --release --workspace; ./scripts/build-installer.ps1; ./scripts/check-size.ps1
#>
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$release = Join-Path $root 'target\release'
$version = [regex]::Match((Get-Content (Join-Path $root 'Cargo.toml') -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value

$budgets = @(
  @{ Name = 'Marky program'; Path = (Join-Path $release 'marky.exe'); LimitMb = 24 },
  @{ Name = 'Installer'; Path = (Join-Path $root "dist\Marky_${version}_x64-setup.exe"); LimitMb = 10 }
)

$overBudget = 0
foreach ($budget in $budgets) {
  if (-not (Test-Path $budget.Path)) { throw "$($budget.Name) not found; run a release build and scripts/build-installer.ps1 first." }
  $sizeMb = (Get-Item $budget.Path).Length / 1MB
  $ok = $sizeMb -le $budget.LimitMb
  if (-not $ok) { $overBudget++ }
  "{0}  {1}: {2:N2} MB (limit {3} MB)" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $budget.Name, $sizeMb, $budget.LimitMb
}
exit $overBudget
