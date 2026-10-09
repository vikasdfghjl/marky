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
  $ids = Get-ProcessTree $RootId
  $ids | ForEach-Object { Stop-Process -Id $_ -Force -ErrorAction SilentlyContinue }
  # Return only once the tree is gone: a relaunch while it lingers would be forwarded to it (single instance).
  Wait-Until { -not (Get-Process -Id $ids -ErrorAction SilentlyContinue) } 10 | Out-Null
}

# Polls $Condition until it returns something truthy or $TimeoutSeconds pass, and returns its last
# result. Wait for state, not time: a cold CI runner starts WebView2 many times slower than a warm laptop.
function Wait-Until([scriptblock]$Condition, [int]$TimeoutSeconds = 60) {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while ($true) {
    $result = & $Condition
    if ($result -or (Get-Date) -ge $deadline) { return $result }
    Start-Sleep -Milliseconds 200
  }
}
