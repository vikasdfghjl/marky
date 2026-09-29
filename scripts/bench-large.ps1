<#
.SYNOPSIS
  Benchmarks a release build on large Markdown files: cold start with the file open, typing latency,
  preview update after typing stops, main-thread stalls and idle RAM.
.EXAMPLE
  npx tauri build --no-bundle; ./scripts/bench-large.ps1
.NOTES
  Files are generated (docs-like: headings, tables, four languages of code fences) into %TEMP%.
  Above 5 MB the preview turns itself off, so 8 MB shows the editor-only path.
#>
param([int[]]$SizesKb = @(100, 1024, 4096, 8192), [int]$StartupRuns = 3)

$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root 'src-tauri\target\release\marky.exe'
if (-not (Test-Path $exe)) { throw 'Release build not found. Run: npx tauri build --no-bundle' }
if (Get-Process marky -ErrorAction SilentlyContinue) { throw 'Close all Marky windows first (single-instance mode would route the test into them).' }

. (Join-Path $PSScriptRoot 'lib.ps1')

$dir = Join-Path $env:TEMP 'marky-bench-large'
New-Item -ItemType Directory -Force $dir | Out-Null
$log = Join-Path $env:TEMP 'marky-bench-large.log'

function Get-Median([double[]]$values) {
  $sorted = $values | Sort-Object
  $sorted[[math]::Floor($sorted.Count / 2)]
}

function Get-TreeRamMb([int]$rootId) {
  $tree = Get-ProcessTree $rootId
  $counters = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $tree -contains $_.IDProcess }
  [math]::Round(($counters | Measure-Object WorkingSetPrivate -Sum).Sum / 1MB, 1)
}

foreach ($kb in $SizesKb) {
  $file = Join-Path $dir "doc-$kb.md"
  node (Join-Path $PSScriptRoot 'bench-large.mjs') gen $kb $file
  $actualKb = [math]::Round((Get-Item $file).Length / 1KB)
  "== $actualKb KB ($([math]::Round($actualKb / 1024, 1)) MB)"

  # Cold start with the file as the startup document (no debugging port: it would skew timing).
  $startups = @(); $loaded = @()
  $env:MARKY_PERF_LOG = $log
  for ($i = 0; $i -lt $StartupRuns; $i++) {
    Remove-Item $log -ErrorAction SilentlyContinue
    $app = Start-Process $exe -ArgumentList "`"$file`"" -PassThru
    Start-Sleep -Seconds 10
    Stop-ProcessTree $app.Id
    Start-Sleep -Seconds 2
    $line = Get-Content $log | Select-String 'startup_ms=(\d+)' | Select-Object -First 1
    if ($line) {
      $startups += [double]$line.Matches[0].Groups[1].Value
      if ($line.Line -match 'ipc-ready=(\d+).*?file-loaded=(\d+)') { $loaded += [double]($Matches[2] - $Matches[1]) }
    }
  }
  Remove-Item Env:MARKY_PERF_LOG -ErrorAction SilentlyContinue
  "cold start median {0} ms [{1}]; ipc-ready to file-loaded median {2} ms" -f (Get-Median $startups), ($startups -join ', '), (Get-Median $loaded)

  # Editing measurements over the DevTools protocol.
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222'
  $app = Start-Process $exe -ArgumentList "`"$file`"" -PassThru
  Start-Sleep -Seconds 8
  $untouched = Get-TreeRamMb $app.Id
  node (Join-Path $PSScriptRoot 'bench-large.mjs') wake
  Start-Sleep -Seconds 6
  "idle RAM $untouched MB untouched (Windows trims idle working sets), $(Get-TreeRamMb $app.Id) MB after waking"
  node (Join-Path $PSScriptRoot 'bench-large.mjs') measure "$actualKb KB"
  "RAM after typing $(Get-TreeRamMb $app.Id) MB"
  Stop-ProcessTree $app.Id
  Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 2
}
