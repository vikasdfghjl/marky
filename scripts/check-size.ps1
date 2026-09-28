<#
.SYNOPSIS
  Fails when the release artifacts exceed the PRD hard limits (release-blocking budgets).
.EXAMPLE
  npx tauri build; ./scripts/check-size.ps1
#>
$ErrorActionPreference = 'Stop'
$release = Join-Path (Split-Path $PSScriptRoot -Parent) 'src-tauri\target\release'

$budgets = @(
  @{ Name = 'App exe'; Path = Join-Path $release 'marky.exe'; LimitMb = 12 },
  @{ Name = 'Installer'; Path = (Get-ChildItem (Join-Path $release 'bundle\nsis\*.exe') | Select-Object -First 1).FullName; LimitMb = 10 }
)

$overBudget = 0
foreach ($budget in $budgets) {
  if (-not $budget.Path -or -not (Test-Path $budget.Path)) { throw "$($budget.Name) not found; run a full 'npx tauri build' first." }
  $sizeMb = (Get-Item $budget.Path).Length / 1MB
  $ok = $sizeMb -le $budget.LimitMb
  if (-not $ok) { $overBudget++ }
  "{0}  {1}: {2:N2} MB (limit {3} MB)" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $budget.Name, $sizeMb, $budget.LimitMb
}
exit $overBudget
