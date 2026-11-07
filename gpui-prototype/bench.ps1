<#
.SYNOPSIS
  Measures the GPUI prototype the way scripts/bench.ps1 measures Marky, for the gate in docs/gpui-migration.md.
.EXAMPLE
  cargo build --manifest-path gpui-prototype/Cargo.toml --release; ./gpui-prototype/bench.ps1
.NOTES
  Same fixture (scripts/fixtures/typical.md), same clock (startup_ms counts from the top of main to the end of
  the first painted frame), same RAM figure (private working set of the process tree, 10 s after launch).
  The release profile matches src-tauri's, so "Program" compares with the README's number.
  Typing: 60 keystrokes after the RAM sample, sent with SendKeys, so they take the real keyboard path.
  type_ms is the time from the edit arriving to the end of the frame that shows it, like the app's
  rAF-after-paint measure in bench-large.ts. SendKeys types into whatever window is in front, and the
  script brings the prototype there: leave the machine alone while it runs, or pass -Keys 0.
  -Via Messages posts the key presses to the window instead, so it can run while you work, but its
  numbers are not comparable: GPUI draws the frame after a posted key later than after a real one
  (7 Nov 2026, typical.md: p50 2.5-5.8 ms and p95 16-18 ms, against 1.6 and 2.6 ms with SendKeys;
  bringing the window to the front or holding the key longer made no difference).
  Preview: preview_ms is the time from the last keystroke to the re-parsed preview being applied.
  The edits are never saved. Compare with Marky only within one session (see AGENTS.md).
#>
param([int]$Runs = 5, [string]$File = '', [int]$Keys = 60, [string]$Exe = '', [ValidateSet('SendKeys', 'Messages')][string]$Via = 'SendKeys')

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
if (-not ('BenchKeys' -as [type])) { Add-Type @'
using System; using System.Runtime.InteropServices;
public static class BenchKeys {
  [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  /// WM_KEYDOWN then WM_KEYUP, as a key press arrives; GPUI's message loop translates the key-down
  /// into WM_CHAR, so the character takes the keyboard's path (key bindings first, then text input).
  public static void Press(IntPtr window, int vk, int scan) {
    PostMessage(window, 0x0100, (IntPtr)vk, (IntPtr)(1 | (scan << 16)));
    PostMessage(window, 0x0101, (IntPtr)vk, (IntPtr)unchecked((int)(0xC0000001 | (uint)(scan << 16))));
  }
}
'@ }
$ramMb = @()
$previewMs = @()
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

  # SendKeys needs the window in front; window messages do not.
  $canType = $Keys -gt 0 -and $(if ($Via -eq 'SendKeys') { $shell.AppActivate($app.Id) } else { $app.Refresh(); $app.MainWindowHandle -ne 0 })
  if ($canType) {
    $typedBefore = Get-LogCount 'type_ms='
    for ($k = 0; $k -lt $Keys; $k++) {
      if ($Via -eq 'SendKeys') { $shell.SendKeys('a') } else { [BenchKeys]::Press($app.MainWindowHandle, 0x41, 0x1E) }
      # Paced like a typist, so each keystroke gets its own frame.
      Start-Sleep -Milliseconds 50
    }
    Wait-Until { (Get-LogCount 'type_ms=') -ge $typedBefore + $Keys } 10 | Out-Null
    # The preview catches up once typing stops: wait for a preview_ms line after the last keystroke.
    $caughtUp = Wait-Until {
      $lines = @(Get-Content $log)
      $lastType = ($lines | Select-String 'type_ms=' | Select-Object -Last 1).LineNumber
      $lastPreview = $lines | Select-String 'preview_ms=(\d+)' | Select-Object -Last 1
      if ($lastPreview -and $lastPreview.LineNumber -gt $lastType) { $lastPreview }
    } 30
    if ($caughtUp) { $previewMs += [double]$caughtUp.Matches[0].Groups[1].Value }
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
  "Typing         p50 {0:N1} ms  p95 {1:N1} ms  ({2} keystrokes, {3})" -f (Get-Median $typeMs), (Get-Percentile $typeMs 0.95), $typeMs.Count, $(if ($Via -eq 'SendKeys') { 'SendKeys' } else { 'window messages, not comparable with SendKeys' })
} elseif ($Keys -gt 0) {
  "Typing         not measured: $(if ($Via -eq 'SendKeys') { 'the window could not be brought to the front for SendKeys' } else { 'no keystroke reached the editor' })"
}
if ($previewMs.Count) {
  "Preview        median {0} ms after typing stops  [{1}]" -f (Get-Median $previewMs), ($previewMs -join ', ')
}
"Program        {0:N2} MB" -f ((Get-Item $exe).Length / 1MB)
