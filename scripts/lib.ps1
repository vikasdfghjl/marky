# Shared helpers for the release-build scripts. Dot-source it: . (Join-Path $PSScriptRoot 'lib.ps1')

# Marky runs as marky.exe plus several msedgewebview2.exe children; measuring or stopping
# the app means handling the whole tree.

function Get-ProcessTree([int]$RootId) {
  $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId
  $ids = @($RootId)
  while ($true) {
    $children = @($all | Where-Object { $ids -contains $_.ParentProcessId -and $ids -notcontains $_.ProcessId })
    if ($children.Count -eq 0) { return $ids }
    $ids += $children.ProcessId
  }
}

function Stop-ProcessTree([int]$RootId) {
  Get-ProcessTree $RootId | ForEach-Object { Stop-Process -Id $_ -Force -ErrorAction SilentlyContinue }
}
