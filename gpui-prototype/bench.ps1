<#
.SYNOPSIS
  Measures the GPUI prototype the way scripts/bench.ps1 measures Marky, for the gate in docs/gpui-migration.md.
.EXAMPLE
  cargo build --manifest-path gpui-prototype/Cargo.toml --release; ./gpui-prototype/bench.ps1
.NOTES
  Same fixture (scripts/fixtures/typical.md), same clock (startup_ms counts from the top of main to the end of
  the first painted frame), same RAM figure (private working set of the process tree, 10 s after launch).
  The release profile matches src-tauri's, so "Program" compares with the README's number.
  Typing: keystrokes are sent with SendKeys after the RAM sample, and type_ms is the time from the edit
  arriving to the end of the frame that shows it, like the app's rAF-after-paint measure in bench-large.ts.
  The edits are never saved. Compare with Marky only within one session (see AGENTS.md).
  SendKeys types into whatever window is in front: leave the machine alone while it runs, or pass -Keys 0.
#>
param([int]$Runs = 5, [string]$File = '', [int]$Keys = 60, [string]$Exe = '')

$root = Split-Path $PSScriptRoot -Parent
$exe = if ($Exe) { $Exe } else { Join-Path $PSScriptRoot 'target\release\gpui-prototype.exe' }
$sample = if ($File) { $File } else { Join-Path $root 'scripts\fixtures\typical.md' }
if (-not (Test-Path $exe)) { throw "Release build not found. Run: cargo build --manifest-path gpui-prototype/Cargo.toml --release" }

. (Join-Path $root 'scripts\lib.ps1')

$log = Join-Path $env:TEMP 'marky-gpui-bench.log'
Remove-Item $log -ErrorAction SilentlyContinue
$env:MARKY_PERF_LOG = $log

function Get-Median([double[]]$values) {
  $sorted = $values | Sort-Object
  $sorted[[math]::Floor($sorted.Count / 2)]
}

function Get-Percentile([double[]]$values, [double]$p) {
  $sorted = $values | Sort-Object
  $sorted[[math]::Min($sorted.Count - 1, [math]::Floor($sorted.Count * $p))]
}

function Get-TreeRamMb([int]$rootId) {
  $tree = Get-ProcessTree $rootId
  $counters = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $tree -contains $_.IDProcess }
  ($counters | Measure-Object WorkingSetPrivate -Sum).Sum / 1MB
}

function Get-LogCount([string]$pattern) {
  if (-not (Test-Path $log)) { return 0 }
  @(Select-String -Path $log -Pattern $pattern).Count
}

$shell = New-Object -ComObject WScript.Shell
$ramMb = @()
for ($i = 1; $i -le $Runs; $i++) {
  $startsBefore = Get-LogCount 'startup_ms='
  $launched = Get-Date
  $app = Start-Process $exe -ArgumentList "`"$sample`"" -PassThru
  if (-not (Wait-Until { (Get-LogCount 'startup_ms=') -gt $startsBefore } 60)) {
    Stop-ProcessTree $app.Id
    throw "Run ${i}: no startup_ms line within 60 s"
  }
  # Sampled at the same moment after launch as bench.ps1, not a wait for state.
  $remaining = 10 - ((Get-Date) - $launched).TotalSeconds
  if ($remaining -gt 0) { Start-Sleep -Milliseconds ([int]($remaining * 1000)) }
  $ramMb += Get-TreeRamMb $app.Id

  if ($Keys -gt 0 -and $shell.AppActivate($app.Id)) {
    for ($k = 0; $k -lt $Keys; $k++) {
      $shell.SendKeys('a')
      # Paced like a typist, so each keystroke gets its own frame.
      Start-Sleep -Milliseconds 50
    }
    $typedBefore = $Keys * ($i - 1)
    Wait-Until { (Get-LogCount 'type_ms=') -ge $typedBefore + $Keys } 10 | Out-Null
  }
  Stop-ProcessTree $app.Id
}
Remove-Item Env:MARKY_PERF_LOG -ErrorAction SilentlyContinue

$lines = Get-Content $log
$startupMs = $lines | Select-String 'startup_ms=(\d+)' | ForEach-Object { [double]$_.Matches[0].Groups[1].Value }
$typeMs = $lines | Select-String 'type_ms=([\d.]+)' | ForEach-Object { [double]$_.Matches[0].Groups[1].Value }

"CPU            {0}" -f (Get-CimInstance Win32_Processor).Name.Trim()
"Runs           $Runs"
"Cold start     median {0} ms  [{1}]" -f (Get-Median $startupMs), ($startupMs -join ', ')
"Idle RAM       median {0:N1} MB  [{1}]  (10 s after launch)" -f (Get-Median $ramMb), (($ramMb | ForEach-Object { '{0:N0}' -f $_ }) -join ', ')
if ($typeMs.Count) {
  "Typing         p50 {0:N1} ms  p95 {1:N1} ms  ({2} keystrokes)" -f (Get-Median $typeMs), (Get-Percentile $typeMs 0.95), $typeMs.Count
} else {
  "Typing         not measured: the window could not be brought to the front for SendKeys"
}
"Program        {0:N2} MB" -f ((Get-Item $exe).Length / 1MB)
