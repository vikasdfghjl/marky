<#
.SYNOPSIS
  End-to-end smoke test of the release app. e2e.ts drives the page over the WebView2 DevTools
  protocol; this script adds what only Windows can see: the native menu, the close guard and
  crash recovery across relaunches. Native dialogs block automation, so their flows are not covered.
.EXAMPLE
  npx tauri build --no-bundle; ./scripts/e2e.ps1
#>
$ErrorActionPreference = 'Stop'
$exe = Join-Path (Split-Path $PSScriptRoot -Parent) 'src-tauri\target\release\marky.exe'
if (-not (Test-Path $exe)) { throw 'Release build not found. Run: npx tauri build --no-bundle' }
# Single-instance mode would forward every launch below into an already-open window.
if (Get-Process marky -ErrorAction SilentlyContinue) { throw 'Close all Marky windows first (single-instance mode would route the test into them).' }

$dir = Join-Path $env:TEMP 'marky-e2e'
Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $dir 'img') | Out-Null
$fixture = Join-Path $dir 'fixture.md'
[IO.File]::WriteAllBytes($fixture, [byte[]](0xEF, 0xBB, 0xBF) + [Text.Encoding]::UTF8.GetBytes("# Fixture`r`n`r`n![dot](img/dot.png)`r`n`r`nBody text`r`n"))
[IO.File]::WriteAllText((Join-Path $dir 'second.md'), "# Second file`n")
[IO.File]::WriteAllBytes((Join-Path $dir 'img\dot.png'), [Convert]::FromBase64String('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII='))

# Settings persist between launches and are shared with an installed Marky: every run
# starts from defaults, and the user's own settings come back afterwards.
$settingsFile = Join-Path $env:APPDATA 'dev.marky.editor\settings.json'
$settingsBackup = Join-Path $dir 'settings.backup.json'
if (Test-Path $settingsFile) { Move-Item $settingsFile $settingsBackup }

$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222'

. (Join-Path $PSScriptRoot 'lib.ps1')

$script:failures = 0

function Write-Check($name, [bool]$ok, $detail = '') {
  if (-not $ok) { $script:failures++ }
  $status = if ($ok) { 'PASS' } else { 'FAIL' }
  "$status  $name$(if ($detail) { "  ($detail)" })"
}

try {
  # Run A: everything scriptable, ending with unsaved changes for the close guard.
  $app = Start-Process $exe -ArgumentList "`"$fixture`"" -PassThru
  # Parenthesised: Invoke-RestMethod emits a JSON array as one object, and the filter needs its items.
  $devTools = Wait-Until { try { (Invoke-RestMethod 'http://127.0.0.1:9222/json' -TimeoutSec 2) | Where-Object type -eq 'page' } catch { $null } }
  if (-not $devTools) { throw 'Marky did not open its DevTools endpoint on port 9222 within 60 s.' }
  npx tsx (Join-Path $PSScriptRoot 'e2e.ts') $fixture $exe (Join-Path $dir 'second.md')
  if ($LASTEXITCODE -ne 0) { $script:failures++ }

  Add-Type -Namespace Win32 -Name Menu -MemberDefinition @'
[DllImport("user32.dll")] public static extern IntPtr GetMenu(IntPtr hWnd);
[DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr hMenu);
[DllImport("user32.dll")] public static extern IntPtr GetSubMenu(IntPtr hMenu, int pos);
[DllImport("user32.dll")] public static extern uint GetMenuState(IntPtr hMenu, uint pos, uint flags);
[DllImport("user32.dll")] static extern bool EnumWindows(EnumWindowsProc cb, IntPtr l);
[DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
[DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, System.Text.StringBuilder name, int max);
[DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
[DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, System.Text.StringBuilder text, int max);
public delegate bool EnumWindowsProc(IntPtr h, IntPtr l);
public static string Title(IntPtr window) { var text = new System.Text.StringBuilder(256); GetWindowText(window, text, 256); return text.ToString(); }
public static bool IsChecked(IntPtr bar, int menu, uint item) {
  const uint MF_BYPOSITION = 0x400, MF_CHECKED = 0x8;
  return (GetMenuState(GetSubMenu(bar, menu), item, MF_BYPOSITION) & MF_CHECKED) != 0;
}
// Process.MainWindowHandle can return Marky's hidden single-instance window instead of the real one.
public static IntPtr MainWindow(uint pid) {
  IntPtr found = IntPtr.Zero;
  EnumWindows((h, l) => {
    uint p; GetWindowThreadProcessId(h, out p);
    var name = new System.Text.StringBuilder(64); GetClassName(h, name, 64);
    if (p == pid && name.ToString() == "Tauri Window") { found = h; return false; }
    return true;
  }, IntPtr.Zero);
  return found;
}
public static void Close(IntPtr window) { PostMessage(window, 0x0010, IntPtr.Zero, IntPtr.Zero); } // WM_CLOSE
'@
  $window = [Win32.Menu]::MainWindow([uint32]$app.Id)
  Write-Check 'main window found' ($window -ne [IntPtr]::Zero)
  $bar = [Win32.Menu]::GetMenu($window)
  $menuCount = [Win32.Menu]::GetMenuItemCount($bar)
  Write-Check 'native menu bar has File, Edit, View, Help' ($menuCount -eq 4) "$menuCount top-level menus"
  # e2e.ts ends with editor and preview on, outline and auto-save off, and word wrap turned off by Alt+Z.
  $ticks = @{
    editor   = [Win32.Menu]::IsChecked($bar, 2, 0)
    preview  = [Win32.Menu]::IsChecked($bar, 2, 1)
    outline  = [Win32.Menu]::IsChecked($bar, 2, 2)
    wordWrap = [Win32.Menu]::IsChecked($bar, 2, 3)
    autoSave = [Win32.Menu]::IsChecked($bar, 0, 6)
  }
  $ticksMatch = $ticks.editor -and $ticks.preview -and -not ($ticks.outline -or $ticks.wordWrap -or $ticks.autoSave)
  Write-Check 'menu ticks follow the settings' $ticksMatch (($ticks.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ' ')

  [Win32.Menu]::Close($window)
  # A negative check, so a fixed wait: the window must still be open 2 s after the close request.
  Write-Check 'close with unsaved changes is held for the prompt' (-not $app.WaitForExit(2000))
  Stop-ProcessTree $app.Id

  # Run B: run A was killed with unsaved changes, so the relaunch must offer recovery (a native dialog).
  Add-Type -Namespace Win32 -Name Dialogs -MemberDefinition @'
public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, System.Text.StringBuilder name, int max);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
public static IntPtr VisibleDialog(uint processId) {
  IntPtr found = IntPtr.Zero;
  EnumWindows((h, _) => {
    uint pid; GetWindowThreadProcessId(h, out pid);
    var name = new System.Text.StringBuilder(64); GetClassName(h, name, 64);
    if (pid == processId && IsWindowVisible(h) && name.ToString() == "#32770") { found = h; return false; }
    return true;
  }, IntPtr.Zero);
  return found;
}
'@
  $recoveryFile = Join-Path $env:LOCALAPPDATA 'dev.marky.editor\recovery.json'
  $recoveryDialog = { $dialog = [Win32.Dialogs]::VisibleDialog([uint32]$app.Id); if ($dialog -ne [IntPtr]::Zero) { $dialog } }
  $app = Start-Process $exe -ArgumentList "`"$fixture`"" -PassThru
  $dialog = Wait-Until $recoveryDialog
  Write-Check 'relaunch after a crash offers recovery' ([bool]$dialog)

  # The title-bar X (WM_CLOSE, like Esc) answers "Not now": nothing is restored or deleted, and the
  # recovery survives closing the app (a clean window, so it closes natively).
  if ($dialog) { [Win32.Menu]::Close($dialog) }
  $dismissed = Wait-Until { -not (& $recoveryDialog) } 10
  [Win32.Menu]::Close([Win32.Menu]::MainWindow([uint32]$app.Id))
  $kept = $dismissed -and $app.WaitForExit(10000) -and "$(Get-Content $recoveryFile -Raw -ErrorAction SilentlyContinue)".Contains('unsaved ')
  Write-Check 'closing the recovery prompt keeps the unsaved work' $kept
  if (-not $app.HasExited) { Stop-ProcessTree $app.Id }

  $app = Start-Process $exe -ArgumentList "`"$fixture`"" -PassThru
  Write-Check 'postponed recovery is offered again at the next launch' ([bool](Wait-Until $recoveryDialog))
  Stop-ProcessTree $app.Id
  Remove-Item $recoveryFile -ErrorAction SilentlyContinue

  # Run C: a clean window closes immediately.
  $app = Start-Process $exe -ArgumentList "`"$fixture`"" -PassThru
  # Wait until the file is showing, so this closes a fully loaded window.
  $window = Wait-Until {
    $handle = [Win32.Menu]::MainWindow([uint32]$app.Id)
    if ($handle -ne [IntPtr]::Zero -and [Win32.Menu]::Title($handle) -eq "fixture.md $([char]0x2014) Marky") { $handle }
  }
  if ($window) { [Win32.Menu]::Close($window) }
  Write-Check 'clean window closes without a prompt' ($window -and $app.WaitForExit(10000))
  if (-not $app.HasExited) { Stop-ProcessTree $app.Id }
}
finally {
  Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
  Remove-Item $settingsFile -ErrorAction SilentlyContinue
  if (Test-Path $settingsBackup) { Move-Item $settingsBackup $settingsFile }
}
exit $script:failures
