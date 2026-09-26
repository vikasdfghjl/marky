<#
.SYNOPSIS
  End-to-end smoke test of the release app, driven over the WebView2 DevTools protocol.
  Covers: open from CLI arg, BOM/CRLF round trip, dirty marker, Ctrl+S, path allowlist, close guard.
.EXAMPLE
  npx tauri build --no-bundle; ./scripts/e2e.ps1
#>
$ErrorActionPreference = 'Stop'
$exe = Join-Path (Split-Path $PSScriptRoot -Parent) 'src-tauri\target\release\marky.exe'
if (-not (Test-Path $exe)) { throw 'Release build not found. Run: npx tauri build --no-bundle' }
$dir = Join-Path $env:TEMP 'marky-e2e'
New-Item -ItemType Directory -Force $dir | Out-Null
$fixture = Join-Path $dir 'fixture.md'
[IO.File]::WriteAllBytes($fixture, [byte[]](0xEF,0xBB,0xBF) + [Text.Encoding]::UTF8.GetBytes("# Fixture`r`n`r`nBody text`r`n"))

$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222'

function Stop-Tree($id) {
  Get-CimInstance Win32_Process | Where-Object ParentProcessId -eq $id | ForEach-Object { Stop-Tree $_.ProcessId }
  Stop-Process -Id $id -Force -ErrorAction SilentlyContinue
}

# Run A: open, edit, save, security check, then close with unsaved changes.
$app = Start-Process $exe -ArgumentList "`"$fixture`"" -PassThru
Start-Sleep -Seconds 4
node (Join-Path $PSScriptRoot 'e2e.mjs') edit-save

$bytes = [IO.File]::ReadAllBytes($fixture)
$text = [Text.Encoding]::UTF8.GetString($bytes, 3, $bytes.Length - 3)
$bomKept = $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF
$expected = "Added line`r`n# Fixture`r`n`r`nBody text`r`n"
"{0}  saved bytes: BOM kept, CRLF kept, edit present" -f $(if ($bomKept -and $text -eq $expected) { 'PASS' } else { "FAIL (bom=$bomKept text=$([regex]::Escape($text)))" })

$app.CloseMainWindow() | Out-Null
Start-Sleep -Seconds 2
"{0}  close with unsaved changes is held for the prompt" -f $(if (-not $app.HasExited) { 'PASS' } else { 'FAIL (window closed)' })
Stop-Tree $app.Id
Start-Sleep -Seconds 1

# Run B: a clean window closes immediately.
$app = Start-Process $exe -ArgumentList "`"$fixture`"" -PassThru
Start-Sleep -Seconds 4
$app.CloseMainWindow() | Out-Null
Start-Sleep -Seconds 2
"{0}  clean window closes without a prompt" -f $(if ($app.HasExited) { 'PASS' } else { 'FAIL (still open)' })
if (-not $app.HasExited) { Stop-Tree $app.Id }
Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
