<#
.SYNOPSIS
  End-to-end smoke test of the release app, driven over the WebView2 DevTools protocol.
  Covers: open from CLI arg, BOM/CRLF round trip, dirty marker, Ctrl+S, path allowlist,
  local images, recent files, external-change reload, recovery snapshot, close guard.
  Native dialogs block automation, so dialog-driven flows are not covered.
.EXAMPLE
  npx tauri build --no-bundle; ./scripts/e2e.ps1
#>
$ErrorActionPreference = 'Stop'
$exe = Join-Path (Split-Path $PSScriptRoot -Parent) 'src-tauri\target\release\marky.exe'
if (-not (Test-Path $exe)) { throw 'Release build not found. Run: npx tauri build --no-bundle' }

$dir = Join-Path $env:TEMP 'marky-e2e'
Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $dir 'img') | Out-Null
$fixture = Join-Path $dir 'fixture.md'
[IO.File]::WriteAllBytes($fixture, [byte[]](0xEF, 0xBB, 0xBF) + [Text.Encoding]::UTF8.GetBytes("# Fixture`r`n`r`n![dot](img/dot.png)`r`n`r`nBody text`r`n"))
[IO.File]::WriteAllText((Join-Path $dir 'second.md'), "# Second file`n")
[IO.File]::WriteAllBytes((Join-Path $dir 'img\dot.png'), [Convert]::FromBase64String('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII='))

$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222'

function Stop-Tree($id) {
  Get-CimInstance Win32_Process | Where-Object ParentProcessId -eq $id | ForEach-Object { Stop-Tree $_.ProcessId }
  Stop-Process -Id $id -Force -ErrorAction SilentlyContinue
}

$script:failures = 0

function Write-Check($name, [bool]$ok, $detail = '') {
  if (-not $ok) { $script:failures++ }
  $status = if ($ok) { 'PASS' } else { 'FAIL' }
  "$status  $name$(if ($detail) { "  ($detail)" })"
}

try {
  # Run A: everything scriptable, ending with unsaved changes for the close guard.
  $app = Start-Process $exe -ArgumentList "`"$fixture`"" -PassThru
  Start-Sleep -Seconds 4
  node (Join-Path $PSScriptRoot 'e2e.mjs') $fixture $exe (Join-Path $dir 'second.md')
  if ($LASTEXITCODE -ne 0) { $script:failures++ }

  Add-Type -Namespace Win32 -Name Menu -MemberDefinition @'
[DllImport("user32.dll")] public static extern IntPtr GetMenu(IntPtr hWnd);
[DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr hMenu);
'@
  $app.Refresh()
  $menuCount = [Win32.Menu]::GetMenuItemCount([Win32.Menu]::GetMenu($app.MainWindowHandle))
  Write-Check 'native menu bar has File, Edit, View, Help' ($menuCount -eq 4) "$menuCount top-level menus"

  $app.CloseMainWindow() | Out-Null
  Start-Sleep -Seconds 2
  Write-Check 'close with unsaved changes is held for the prompt' (-not $app.HasExited)
  Stop-Tree $app.Id
  Start-Sleep -Seconds 1

  # Run B: a clean window closes immediately.
  $app = Start-Process $exe -ArgumentList "`"$fixture`"" -PassThru
  Start-Sleep -Seconds 4
  $app.CloseMainWindow() | Out-Null
  Start-Sleep -Seconds 2
  Write-Check 'clean window closes without a prompt' $app.HasExited
  if (-not $app.HasExited) { Stop-Tree $app.Id }
}
finally {
  Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
}
exit $script:failures
