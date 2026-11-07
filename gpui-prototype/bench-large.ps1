<#
.SYNOPSIS
  Benchmarks the GPUI prototype's release build on large Markdown files, the counterpart of
  scripts/bench-large.ps1: cold start with the file open, idle RAM, a typing burst, typing with the
  preview catching up after each key, main-thread stalls, and RAM after typing.
.EXAMPLE
  cargo build --manifest-path gpui-prototype/Cargo.toml --release --workspace; ./gpui-prototype/bench-large.ps1
.NOTES
  The files are the app's: the same docs-like generator as scripts/bench-large.ts (headings, links,
  lists, tables, four languages of code fences, quotes), ported here so no Node is needed.
  Measures, as in the app's script:
  - typing: type_ms, from the edit arriving to the end of the frame that shows it (the app times
    beforeinput to the frame after paint);
  - burst: 30 keys, 70 ms apart; with preview: 12 keys, each waiting for the preview to catch up;
  - preview: preview_ms, from the keystroke to the re-parsed preview applied, and the parse time;
  - stalls: frames over 50 ms and parse results that take over 50 ms to apply, both on the main
    thread (the app counts long tasks over 50 ms).
  Keys go in with SendKeys, the typing measure of record (see bench.ps1): the prototype is brought to
  the front, and the script checks before every key that it still is, so it never types elsewhere.
  Leave the machine alone while it types. The edits are never saved.
#>
param([int[]]$SizesKb = @(100, 1024, 4096, 8192), [int]$StartupRuns = 3, [string]$Exe = '')

$root = Split-Path $PSScriptRoot -Parent
$exe = if ($Exe) { $Exe } else { Join-Path $PSScriptRoot 'target\release\gpui-prototype.exe' }
if (-not (Test-Path $exe)) { throw 'Release build not found. Run: cargo build --manifest-path gpui-prototype/Cargo.toml --release --workspace' }
. (Join-Path $root 'scripts\lib.ps1')

$dir = Join-Path $env:TEMP 'marky-gpui-bench-large'
New-Item -ItemType Directory -Force $dir | Out-Null
$log = Join-Path $env:TEMP 'marky-gpui-bench-large.log'
$shell = New-Object -ComObject WScript.Shell
if (-not ('BenchLargeWin' -as [type])) {
  Add-Type -Name BenchLargeWin -Namespace '' -MemberDefinition '[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();'
}

# ----- The app's document generator (scripts/bench-large.ts), byte for byte -----

function New-Document([int]$TargetBytes) {
  $code = [ordered]@{
    js   = "export async function load(id) {`n  const res = await fetch(``/api/items/`${id}``);`n  if (!res.ok) throw new Error(res.statusText);`n  return res.json();`n}"
    rust = "pub fn parse(input: &str) -> Result<Config, Error> {`n    let value: toml::Value = input.parse()?;`n    Config::try_from(value)`n}"
    json = "{`n  `"name`": `"service`",`n  `"retries`": 3,`n  `"hosts`": [`"a.example.com`", `"b.example.com`"]`n}"
    bash = "export SERVICE_URL=https://example.com`ncurl -sf `"`$SERVICE_URL/health`" || exit 1`nsystemctl restart service"
  }
  $languages = @($code.Keys)
  $out = [Text.StringBuilder]::new("# API reference`n")
  for ($i = 1; $out.Length -lt $TargetBytes; $i++) {
    $lang = $languages[$i % $languages.Count]
    [void]$out.Append(@"

## Section ${i}: configuring the $lang client

The ``timeout$i`` option controls how requests are routed. See [the guide](https://example.com/docs/$i) and **remember** that _defaults_ differ per environment; the values below are examples, not recommendations.

- First item with ``inline code`` and more words to fill the line
- Second item with a [link to the section](#section-$i-configuring-the-$lang-client)
- Third item

| Option | Type | Default |
| --- | --- | --- |
| ``timeout`` | number | $i |
| ``retries`` | number | 3 |

``````$lang
$($code[$lang])
``````

> **Note**
> Changes take effect after the next restart.

"@.Replace("`r`n", "`n"))
  }
  $out.ToString()
}

# ----- Helpers -----

function Get-Median([double[]]$Values) { if (-not $Values) { return $null }; ($Values | Sort-Object)[[math]::Floor($Values.Count / 2)] }
function Get-Percentile([double[]]$Values, [double]$P) { if (-not $Values) { return $null }; $sorted = $Values | Sort-Object; $sorted[[math]::Min($sorted.Count - 1, [math]::Floor($sorted.Count * $P))] }

function Get-TreeRamMb([int]$RootId) {
  $tree = Get-ProcessTree $RootId
  $counters = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $tree -contains $_.IDProcess }
  [math]::Round(($counters | Measure-Object WorkingSetPrivate -Sum).Sum / 1MB, 1)
}

function Read-Log { if (Test-Path $log) { @(Get-Content $log) } else { @() } }

# Values of `name=` on log lines after line $From.
function Get-Values([string]$Name, [int]$From) {
  Read-Log | Select-Object -Skip $From | Select-String "$Name=([\d.]+)" | ForEach-Object { [double]$_.Matches[0].Groups[1].Value }
}

function Start-Prototype([string]$File) {
  $env:MARKY_PERF_LOG = $log
  try { Start-Process $exe -ArgumentList "`"$File`"" -PassThru } finally { Remove-Item Env:MARKY_PERF_LOG -ErrorAction SilentlyContinue }
}

# One key, only if the prototype is still the window in front.
function Send-Key($App, [string]$Key) {
  $App.Refresh()
  if ([BenchLargeWin]::GetForegroundWindow() -ne $App.MainWindowHandle) { return $false }
  $shell.SendKeys($Key)
  $true
}

foreach ($kb in $SizesKb) {
  $file = Join-Path $dir "doc-$kb.md"
  [IO.File]::WriteAllText($file, (New-Document ($kb * 1024)))
  $actualKb = [math]::Round((Get-Item $file).Length / 1KB)
  "== $actualKb KB ($([math]::Round($actualKb / 1024, 1)) MB)"

  # Cold start with the file as the startup document, and RAM 10 s after launch, untouched.
  $startups = @(); $idle = @()
  for ($i = 0; $i -lt $StartupRuns; $i++) {
    Remove-Item $log -ErrorAction SilentlyContinue
    $launched = Get-Date
    $app = Start-Prototype $file
    $line = Wait-Until { Read-Log | Select-String 'startup_ms=(\d+)' | Select-Object -First 1 } 60
    $remaining = 10 - ((Get-Date) - $launched).TotalSeconds
    if ($remaining -gt 0) { Start-Sleep -Milliseconds ([int]($remaining * 1000)) }
    $idle += Get-TreeRamMb $app.Id
    Stop-ProcessTree $app.Id
    if ($line) { $startups += [double]$line.Matches[0].Groups[1].Value }
  }
  "cold start median {0} ms [{1}]; idle RAM median {2} MB untouched" -f (Get-Median $startups), ($startups -join ', '), (Get-Median $idle)

  # Editing, in one window.
  Remove-Item $log -ErrorAction SilentlyContinue
  $app = Start-Prototype $file
  Wait-Until { Read-Log | Select-String 'startup_ms=' } 60 | Out-Null
  Start-Sleep -Seconds 8
  $untouched = Get-TreeRamMb $app.Id
  if (-not $shell.AppActivate($app.Id) -or -not (Wait-Until { $app.Refresh(); [BenchLargeWin]::GetForegroundWindow() -eq $app.MainWindowHandle } 5)) {
    "typing not measured: the prototype could not be brought to the front"
    Stop-ProcessTree $app.Id
    continue
  }
  Send-Key $app '^{HOME}' | Out-Null
  Start-Sleep -Seconds 6
  "idle RAM $untouched MB untouched, $(Get-TreeRamMb $app.Id) MB after waking"

  # Burst: 30 keys, 70 ms apart, as the app's script types.
  $from = (Read-Log).Count
  $sent = 0
  for ($k = 0; $k -lt 30; $k++) { if (Send-Key $app 'a') { $sent++ }; Start-Sleep -Milliseconds 70 }
  Wait-Until { @(Get-Values 'type_ms' $from).Count -ge $sent } 10 | Out-Null
  Start-Sleep -Milliseconds 1500
  $burst = @(Get-Values 'type_ms' $from)
  $burstStalls = @(Get-Values 'stall_ms' $from)

  # With preview: 12 keys, each waiting for the preview to catch up.
  $from = (Read-Log).Count
  for ($k = 0; $k -lt 12; $k++) {
    $before = @(Get-Values 'preview_ms' $from).Count
    if (-not (Send-Key $app 'b')) { break }
    Wait-Until { @(Get-Values 'preview_ms' $from).Count -gt $before } 10 | Out-Null
    Start-Sleep -Milliseconds 300
  }
  $paced = @(Get-Values 'type_ms' $from)
  $preview = @(Get-Values 'preview_ms' $from)
  $parse = @(Get-Values 'parse_ms' $from)
  $pacedStalls = @(Get-Values 'stall_ms' $from)
  $worst = @($burstStalls + $pacedStalls + 0 | Measure-Object -Maximum).Maximum

  "typing burst p50 {0} ms p95 {1} ms max {2} ms ({3} keys)" -f (Get-Median $burst), (Get-Percentile $burst 0.95), (@($burst | Measure-Object -Maximum).Maximum), $burst.Count
  "typing with preview p50 {0} ms p95 {1} ms max {2} ms ({3} keys)" -f (Get-Median $paced), (Get-Percentile $paced 0.95), (@($paced | Measure-Object -Maximum).Maximum), $paced.Count
  "preview after typing stops median {0} ms (parse {1} ms)" -f (Get-Median $preview), (Get-Median $parse)
  "stalls over 50 ms: {0} in the burst, {1} with preview, worst {2} ms" -f $burstStalls.Count, $pacedStalls.Count, $worst
  "RAM after typing $(Get-TreeRamMb $app.Id) MB"
  Stop-ProcessTree $app.Id
}
