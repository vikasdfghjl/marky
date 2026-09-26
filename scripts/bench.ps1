<#
.SYNOPSIS
  Benchmarks a release build: cold start to editable, WebView2 ready time and idle RAM.
.EXAMPLE
  npx tauri build --no-bundle; ./scripts/bench.ps1
.NOTES
  RAM is the private working set summed over marky.exe and all its WebView2 child processes
  (what Task Manager's "Memory" column shows), sampled after 10 s idle with README.md open.
  -BrowserArgs overrides WebView2 flags for experiments without a rebuild.
#>
param([int]$Runs = 5, [string]$BrowserArgs = '', [switch]$Details)

$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root 'src-tauri\target\release\marky.exe'
$sample = Join-Path $root 'README.md'
if (-not (Test-Path $exe)) { throw "Release build not found. Run: npx tauri build --no-bundle" }

$log = Join-Path $env:TEMP 'marky-bench.log'
Remove-Item $log -ErrorAction SilentlyContinue
$env:MARKY_PERF_LOG = $log
if ($BrowserArgs) { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $BrowserArgs }

function Get-ProcessTree([int]$rootId) {
  $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId
  $ids = @($rootId)
  while ($true) {
    $children = @($all | Where-Object { $ids -contains $_.ParentProcessId -and $ids -notcontains $_.ProcessId })
    if ($children.Count -eq 0) { return $ids }
    $ids += $children.ProcessId
  }
}

function Get-Median([double[]]$values) {
  $sorted = $values | Sort-Object
  $sorted[[math]::Floor($sorted.Count / 2)]
}

$ramMb = @()
for ($i = 1; $i -le $Runs; $i++) {
  $app = Start-Process $exe -ArgumentList "`"$sample`"" -PassThru
  Start-Sleep -Seconds 10
  $tree = Get-ProcessTree $app.Id
  $counters = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $tree -contains $_.IDProcess }
  $ramMb += ($counters | Measure-Object WorkingSetPrivate -Sum).Sum / 1MB
  $tree | ForEach-Object { Stop-Process -Id $_ -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Seconds 2
}
Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS, Env:MARKY_PERF_LOG -ErrorAction SilentlyContinue

$lines = Get-Content $log
$startupMs = $lines | Select-String 'startup_ms=(\d+)' | ForEach-Object { [double]$_.Matches[0].Groups[1].Value }
$webviewMs = $lines | Select-String 'setup_ms=(\d+)' | ForEach-Object { [double]$_.Matches[0].Groups[1].Value }
$installer = Get-ChildItem (Join-Path $root 'src-tauri\target\release\bundle\nsis\*.exe') -ErrorAction SilentlyContinue | Select-Object -First 1

"CPU            {0}" -f (Get-CimInstance Win32_Processor).Name.Trim()
"Runs           $Runs"
"Cold start     median {0} ms  [{1}]" -f (Get-Median $startupMs), ($startupMs -join ', ')
"WebView2 ready median {0} ms" -f (Get-Median $webviewMs)
"Idle RAM       median {0:N1} MB  [{1}]" -f (Get-Median $ramMb), (($ramMb | ForEach-Object { '{0:N0}' -f $_ }) -join ', ')
"App exe        {0:N2} MB" -f ((Get-Item $exe).Length / 1MB)
if ($installer) { "Installer      {0:N2} MB" -f ($installer.Length / 1MB) }
if ($Details) { $lines | Select-String 'startup_ms' }
