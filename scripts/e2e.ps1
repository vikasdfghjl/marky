<#
.SYNOPSIS
  End-to-end smoke test of Marky's release build.
.DESCRIPTION
  GPUI has no DevTools protocol, so this drives the window the way assistive technology does:
  through Windows UI Automation, which reads the AccessKit tree Marky builds. Controls are found by
  role and name and pressed through their Invoke, Toggle or Expand patterns; text is typed with
  window messages and read back through the editor's TextPattern. Nothing needs the mouse, the
  keyboard or the window in front, so it can run while you work. Files are checked byte for byte on
  disk.

  Every run uses a temporary APPDATA and LOCALAPPDATA, so no user settings, recent list, recovery
  snapshots or log are read or changed. Prints one PASS, FAIL or SKIP line per check and exits with
  the number of failures.
.EXAMPLE
  cargo build --release --workspace; ./scripts/e2e.ps1
#>
param([string]$Exe = '')

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$exe = if ($Exe) { $Exe } else { Join-Path $root 'target\release\marky.exe' }
if (-not (Test-Path $exe)) { throw 'Release build not found. Run: cargo build --release --workspace' }
. (Join-Path $root 'scripts\lib.ps1')

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -Namespace E2e -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
[DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
[DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
[DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, System.Text.StringBuilder name, int max);
[DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
public delegate bool EnumProc(IntPtr h, IntPtr l);
public static IntPtr VisibleDialog(uint processId) {
  IntPtr found = IntPtr.Zero;
  EnumWindows((h, l) => {
    uint pid; GetWindowThreadProcessId(h, out pid);
    var name = new System.Text.StringBuilder(64); GetClassName(h, name, 64);
    if (pid == processId && IsWindowVisible(h) && name.ToString() == "#32770") { found = h; return false; }
    return true;
  }, IntPtr.Zero);
  return found;
}
public static void Close(IntPtr window) { PostMessage(window, 0x0010, IntPtr.Zero, IntPtr.Zero); }
/// A key press as window messages. Modifiers cannot be sent this way (GPUI reads their state from
/// the keyboard), so the tests use only plain keys and reach Ctrl shortcuts through the menus.
public static void Key(IntPtr window, int vk, int scan) {
  PostMessage(window, 0x0100, (IntPtr)vk, (IntPtr)(1 | (scan << 16)));
  PostMessage(window, 0x0101, (IntPtr)vk, (IntPtr)unchecked((int)(0xC0000001 | (uint)(scan << 16))));
}
public static void Type(IntPtr window, string text) {
  foreach (char ch in text) { PostMessage(window, 0x0102, (IntPtr)ch, (IntPtr)1); }
}
'@

$A = [System.Windows.Automation.AutomationElement]
$T = [System.Windows.Automation.ControlType]
$Scope = [System.Windows.Automation.TreeScope]

# ----- The fixture: a UTF-8 file with a BOM and CRLF endings, as in the app's test -----

$dir = Join-Path $env:TEMP 'marky-e2e'
Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $dir | Out-Null
$appData = Join-Path $dir 'appdata'
New-Item -ItemType Directory -Force $appData | Out-Null
$fence = '```'
$text = @(
  '# Fixture', '',
  'Body text with a [link to the second file](second.md) and [a PDF](notes.pdf).', '',
  'Prices like costs $5 and $10 stay text, and a note[^1].', '',
  '## Second heading', '',
  "${fence}js", 'const answer = 42;', $fence, '',
  "${fence}mermaid", 'flowchart LR', '  A --> B', $fence, '',
  'H~2~O and 19^th^ are scripts.', '',
  '[^1]: The footnote text.', ''
) -join "`n"
$fixture = Join-Path $dir 'fixture.md'
function Write-Fixture([string]$Content) {
  [IO.File]::WriteAllBytes($fixture, [byte[]](0xEF, 0xBB, 0xBF) + [Text.Encoding]::UTF8.GetBytes($Content.Replace("`n", "`r`n")))
}
Write-Fixture $text
[IO.File]::WriteAllText((Join-Path $dir 'second.md'), "# Second file`n")
[IO.File]::WriteAllText((Join-Path $dir 'notes.pdf'), 'not a document')

# ----- Helpers -----

$script:failures = 0
function Write-Check([string]$Name, [bool]$Ok, [string]$Detail = '') {
  if (-not $Ok) { $script:failures++ }
  "$(if ($Ok) { 'PASS' } else { 'FAIL' })  $Name$(if ($Detail) { "  ($Detail)" })"
}
function Write-Skip([string]$Name, [string]$Why) { "SKIP  $Name  ($Why)" }

$script:launched = [Collections.Generic.List[System.Diagnostics.Process]]::new()
function Start-Marky([string]$Document) {
  # A temporary APPDATA and LOCALAPPDATA: Marky reads and keeps its settings, recent list, recovery
  # snapshots and log under them.
  $savedAppData = $env:APPDATA
  $savedLocalAppData = $env:LOCALAPPDATA
  $env:APPDATA = $appData
  $env:LOCALAPPDATA = $appData
  try { $process = Start-Process $exe -ArgumentList "`"$Document`"" -PassThru } finally {
    $env:APPDATA = $savedAppData
    $env:LOCALAPPDATA = $savedLocalAppData
  }
  $script:launched.Add($process)
  $process
}

function Window-Of($Process) {
  Wait-Until { $Process.Refresh(); if ($Process.HasExited) { return $null }; if ($Process.MainWindowHandle -ne 0) { $Process.MainWindowHandle } } 60
}

function Title-Of($Process) { $Process.Refresh(); $Process.MainWindowTitle }

function Condition($Type, [string]$Name) {
  $parts = @()
  if ($Type) { $parts += New-Object System.Windows.Automation.PropertyCondition($A::ControlTypeProperty, $Type) }
  if ($Name) { $parts += New-Object System.Windows.Automation.PropertyCondition($A::NameProperty, $Name) }
  if ($parts.Count -eq 1) { return $parts[0] }
  New-Object System.Windows.Automation.AndCondition($parts)
}

# The first query switches AccessKit on; GPUI then redraws, so elements are waited for, not assumed.
function Find($Type, [string]$Name, [int]$TimeoutSeconds = 10) {
  Wait-Until { $script:ui.FindFirst($Scope::Descendants, (Condition $Type $Name)) } $TimeoutSeconds
}
function Find-Now($Type, [string]$Name) { $script:ui.FindFirst($Scope::Descendants, (Condition $Type $Name)) }
function Find-Like([string]$Pattern, [int]$TimeoutSeconds = 10) {
  Wait-Until {
    $all = $script:ui.FindAll($Scope::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($element in $all) { if ($element.Current.Name -match $Pattern) { return $element } }
  } $TimeoutSeconds
}

function Press($Element) {
  if (-not $Element) { return $false }
  foreach ($pattern in @([System.Windows.Automation.InvokePattern]::Pattern, [System.Windows.Automation.TogglePattern]::Pattern, [System.Windows.Automation.SelectionItemPattern]::Pattern)) {
    $found = $null
    if ($Element.TryGetCurrentPattern($pattern, [ref]$found)) {
      switch ($found.GetType().Name) {
        'InvokePattern' { $found.Invoke() }
        'TogglePattern' { $found.Toggle() }
        'SelectionItemPattern' { $found.Select() }
      }
      return $true
    }
  }
  $false
}

# A menu item, through the menu: Expand opens it, as a screen reader would.
function Run-Menu([string]$Menu, [string]$Item) {
  $button = Find $T::MenuItem $Menu
  if (-not $button) { return $false }
  $button.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
  Press (Find $null $Item)
}

function Editor-Text {
  $editor = Find $T::Edit 'Editor'
  if (-not $editor) { return $null }
  $editor.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern).DocumentRange.GetText(-1)
}

# The editor's caret line, as a screen reader reads it.
function Caret-Line {
  $editor = Find $T::Edit 'Editor'
  $range = $editor.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern).GetSelection()[0].Clone()
  $range.ExpandToEnclosingUnit([System.Windows.Automation.Text.TextUnit]::Line)
  $range.GetText(-1).TrimEnd()
}

function Focused-Name { $focused = $A::FocusedElement; "$($focused.Current.ControlType.ProgrammaticName.Replace('ControlType.', '')) '$($focused.Current.Name)'" }

# The selection's rectangles, in physical pixels: what a screen magnifier follows. .NET returns
# one System.Windows.Rect per range rectangle.
function Caret-Rectangles {
  $editor = Find $T::Edit 'Editor'
  if (-not $editor) { return $null }
  # .NET reports a degenerate UIA range as empty, so take the character after the caret: its
  # rectangle is the caret's position and height, which is what a magnifier follows.
  $range = $editor.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern).GetSelection()[0].Clone()
  [void]$range.MoveEndpointByUnit(1, [System.Windows.Automation.Text.TextUnit]::Character, 1)
  $range.GetBoundingRectangles()
}

function Preview-Text {
  $preview = Find $T::Document 'Preview'
  if (-not $preview) { return $null }
  $preview.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern).DocumentRange.GetText(-1)
}

function Settings-File { Join-Path $appData 'dev.marky.editor\settings.json' }

try {
  # ----- Run A: one window, everything that can be driven -----

  $app = Start-Marky $fixture
  $window = Window-Of $app
  if (-not $window) { throw "Marky did not open a window (exit code $($app.ExitCode))." }
  $script:ui = $A::FromHandle($window)
  Write-Check 'startup file opened' ((Wait-Until { (Title-Of $app) -eq 'fixture.md - Marky' } 30) -as [bool]) (Title-Of $app)
  $shown = Wait-Until { $value = Editor-Text; if ($value -eq $text) { $value } } 30
  Write-Check 'editor holds the text without BOM, with LF' ([bool]$shown) "$((Editor-Text).Length) of $($text.Length) chars"
  Write-Check 'preview rendered' ([bool](Find $T::Text 'Fixture' 30))
  $previewText = Wait-Until { $value = Preview-Text; if ($value -like '*Body text*') { $value } } 15
  Write-Check 'the preview exposes a text pattern that can be reviewed' ([bool]$previewText) "$("$previewText".Length) chars"
  Write-Check 'status bar shows CRLF and UTF-8 BOM' ([bool](Find $null 'CRLF') -and [bool](Find $null 'UTF-8 BOM'))
  Write-Check 'prices with $ stay plain text' ([bool](Find-Like 'costs \$5 and \$10 stay text'))
  Write-Check 'footnotes are gathered at the end' ([bool](Find-Like '^Footnote 1: The footnote text'))
  $diagram = Wait-Until { Find $T::Image 'Mermaid diagram' } 30
  Write-Check 'mermaid diagram is drawn in the preview' ([bool]$diagram)

  [E2e.Win]::Type($window, 'unsaved ')
  Write-Check 'dirty marker after typing' ((Wait-Until { (Title-Of $app) -eq 'fixture.md • - Marky' }) -as [bool]) (Title-Of $app)
  Write-Check 'typed text reaches the editor' ((Wait-Until { (Editor-Text) -eq "unsaved $text" } 10) -as [bool])

  Press (Find $T::Button 'Save') | Out-Null
  Write-Check 'clean after Save' ((Wait-Until { (Title-Of $app) -eq 'fixture.md - Marky' }) -as [bool]) (Title-Of $app)
  $expected = [byte[]](0xEF, 0xBB, 0xBF) + [Text.Encoding]::UTF8.GetBytes("unsaved $text".Replace("`n", "`r`n"))
  $onDisk = [IO.File]::ReadAllBytes($fixture)
  Write-Check 'saved bytes keep BOM and CRLF' ([Convert]::ToBase64String($onDisk) -eq [Convert]::ToBase64String($expected)) "$($onDisk.Length) bytes, expected $($expected.Length)"

  Run-Menu 'Edit' 'Undo' | Out-Null
  $undone = Wait-Until { (Editor-Text) -eq $text }
  Write-Check 'Edit > Undo restores the text and marks it unsaved' ([bool]$undone -and (Title-Of $app) -like '*•*') (Title-Of $app)
  Run-Menu 'Edit' 'Redo' | Out-Null
  Write-Check 'Edit > Redo returns to the saved text' ((Wait-Until { (Title-Of $app) -eq 'fixture.md - Marky' }) -as [bool]) (Title-Of $app)

  Run-Menu 'Edit' 'Find' | Out-Null
  Wait-Until { Find-Now $T::Group 'Find and replace' } 10 | Out-Null
  [E2e.Win]::Type($window, 'body')
  $count = Find-Like '^1 of 1$'
  Write-Check 'find counts matches without case' ([bool]$count)
  [E2e.Win]::Key($window, 0x1B, 0x01)
  Write-Check 'Esc closes find and returns to the editor' ((Wait-Until { -not (Find-Now $T::Group 'Find and replace') -and (Focused-Name) -eq "Edit 'Editor'" } 10) -as [bool]) (Focused-Name)

  Press (Find $T::Button 'Preview') | Out-Null
  Write-Check 'toolbar toggles the preview off' ((Wait-Until { -not (Find-Now $T::Document 'Preview') } 10) -as [bool])
  Press (Find $T::Button 'Preview') | Out-Null
  Press (Find $T::Button 'Editor') | Out-Null
  $readingView = Wait-Until { -not (Find-Now $T::Edit 'Editor') -and (Find-Now $T::Document 'Preview') } 10
  Write-Check 'hiding the editor leaves the preview' ([bool]$readingView)
  Press (Find $T::Button 'Editor') | Out-Null
  Find $T::Edit 'Editor' | Out-Null

  Press (Find $T::Button 'Outline') | Out-Null
  Write-Check 'outline lists the document headings' ([bool](Find $T::ListItem 'Second heading'))
  Press (Find $T::Button 'Outline') | Out-Null

  Run-Menu 'File' 'Settings…' | Out-Null
  Write-Check 'File > Settings opens the settings' ([bool](Find $T::Group 'Settings'))
  $stayedInside = $true
  for ($tab = 0; $tab -lt 6; $tab++) {
    [E2e.Win]::Key($window, 0x09, 0x0F)
    $focused = Focused-Name
    if ($focused -eq "Edit 'Editor'" -or $focused -eq "Document 'Preview'") { $stayedInside = $false }
  }
  Write-Check 'Tab stays among the settings controls' $stayedInside (Focused-Name)
  Press (Find $T::CheckBox 'Word wrap') | Out-Null
  $saved = Wait-Until { try { $json = Get-Content (Settings-File) -Raw | ConvertFrom-Json; if ($json.wordWrap -eq $false) { $json } } catch { $null } } 10
  Write-Check 'settings persist to settings.json' ([bool]$saved) (Settings-File)
  Press (Find $T::Button 'Close settings') | Out-Null
  Write-Check 'Close settings puts the panel away' ((Wait-Until { -not (Find-Now $T::Group 'Settings') } 10) -as [bool])

  Press (Find $T::Hyperlink 'link to the second file') | Out-Null
  Write-Check 'relative .md link opens in a new tab' ((Wait-Until { (Title-Of $app) -eq 'second.md - Marky' } 10) -as [bool]) (Title-Of $app)
  Press (Find $T::Button 'Close second.md') | Out-Null
  Write-Check 'closing a clean tab removes it' ((Wait-Until { -not (Find-Now $T::TabItem 'second.md') -and (Title-Of $app) -eq 'fixture.md - Marky' } 10) -as [bool]) (Title-Of $app)

  Press (Find $T::Hyperlink 'a PDF') | Out-Null
  Write-Check 'links to non-Markdown files are refused' ([bool](Find-Like '^Not opened: notes\.pdf'))

  # Pressing a link leaves the focus on it, as in a browser. A screen reader moves it back with
  # UI Automation's SetFocus, which AccessKit passes on as a focus request.
  try { (Find $T::Edit 'Editor').SetFocus() } catch { }
  Write-Check 'assistive technology can focus the editor' ((Wait-Until { (Focused-Name) -eq "Edit 'Editor'" } 10) -as [bool]) (Focused-Name)
  # Find left the caret on the "Body text" line; two lines down is the "Prices" line.
  $before = Caret-Line
  [E2e.Win]::Key($window, 0x28, 0x50); [E2e.Win]::Key($window, 0x28, 0x50)
  $line = Wait-Until { $current = Caret-Line; if ($current -like 'Prices like*') { $current } } 10
  Write-Check 'screen readers follow the caret (TextPattern)' ([bool]$line -and $before -like 'Body text*') "from '$before' to '$(Caret-Line)'"
  $rects = Caret-Rectangles
  Write-Check 'the caret has a screen rectangle for a magnifier' ([bool]$rects -and $rects.Count -ge 1 -and $rects[0].Width -gt 0 -and $rects[0].Height -gt 0) "$($rects[0])"

  [E2e.Win]::Type($window, 'x')
  Write-Check 'dirty again before the close test' ((Wait-Until { (Title-Of $app) -like '*•*' }) -as [bool])

  [E2e.Win]::Key($window, 0x1B, 0x01); [E2e.Win]::Key($window, 0x09, 0x0F)
  Write-Check 'Esc then Tab leaves the editor' ((Wait-Until { (Focused-Name) -eq "Document 'Preview'" } 10) -as [bool]) (Focused-Name)

  # The preview is one tab stop; the right arrow selects its first link and Enter follows it.
  [E2e.Win]::Key($window, 0x27, 0x4D); [E2e.Win]::Key($window, 0x0D, 0x1C)
  Write-Check 'arrow keys reach a preview link and Enter opens it' ((Wait-Until { (Title-Of $app) -eq 'second.md - Marky' } 10) -as [bool]) (Title-Of $app)
  Press (Find $T::Button 'Close second.md') | Out-Null
  Wait-Until { (Title-Of $app) -like 'fixture.md*' } 10 | Out-Null

  [E2e.Win]::Close($window)
  # A negative check, so a fixed wait: the window must still be open 2 s after the close request.
  Write-Check 'close with unsaved changes is held for the prompt' (-not $app.WaitForExit(2000))
  Stop-ProcessTree $app.Id

  # ----- Run B: a clean window closes at once -----

  $app = Start-Marky $fixture
  $window = Window-Of $app
  $ready = Wait-Until { (Title-Of $app) -eq 'fixture.md - Marky' } 30
  if ($ready) { [E2e.Win]::Close($window) }
  Write-Check 'clean window closes without a prompt' ($ready -and $app.WaitForExit(10000))

  # ----- Run C: files over the size limit are refused before they are read -----

  $huge = Join-Path $dir 'huge.md'
  $stream = [IO.File]::Create($huge); $stream.SetLength(129MB); $stream.Close()
  $app = Start-Marky $huge
  $dialog = Wait-Until { $found = [E2e.Win]::VisibleDialog([uint32]$app.Id); if ($found -ne [IntPtr]::Zero) { $found } } 30
  if ($dialog) { [E2e.Win]::Close($dialog) }
  Write-Check 'files over the size limit are refused' ([bool]$dialog -and $app.WaitForExit(10000) -and $app.ExitCode -eq 1) "exit code $(if ($app.HasExited) { $app.ExitCode } else { 'running' })"
  Remove-Item $huge -ErrorAction SilentlyContinue

  # ----- Run D: tabs, external changes, recent files, single instance and auto-save -----

  Write-Fixture $text
  $app = Start-Marky $fixture
  $window = Window-Of $app
  $script:ui = $A::FromHandle($window)
  $ready = Wait-Until { (Title-Of $app) -eq 'fixture.md - Marky' } 30
  if ($ready) {
    (Find $T::MenuItem 'File').GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    [E2e.Win]::Key($window, 0x4E, 0x31)
    Write-Check 'the underlined menu key N opens a New Tab' ((Wait-Until { (Title-Of $app) -eq 'Untitled - Marky' } 10) -as [bool]) (Title-Of $app)
    Press (Find $T::Button 'Close Untitled') | Out-Null
    Write-Check 'closing a clean Untitled tab returns to the first tab' ((Wait-Until { (Title-Of $app) -eq 'fixture.md - Marky' } 10) -as [bool]) (Title-Of $app)

    # Another program changes the file while the buffer is clean: the watcher reloads it.
    Write-Fixture "# Changed on disk`n"
    Write-Check 'a clean document reloads an external change' ((Wait-Until { (Editor-Text) -eq "# Changed on disk`n" } 15) -as [bool]) "$((Editor-Text).Length) chars"

    # A second launch forwards its path instead of opening a second window.
    $second = Join-Path $dir 'second.md'
    [IO.File]::WriteAllText($second, "# Second file`n")
    Start-Marky $second | Out-Null
    Write-Check 'a second launch opens its file as a tab' ((Wait-Until { (Title-Of $app) -eq 'second.md - Marky' } 15) -as [bool]) (Title-Of $app)

    Run-Menu 'File' 'Open Recent…' | Out-Null
    Write-Check 'Open Recent lists the file just opened' ([bool](Find $T::Group 'Recent files') -and [bool](Find-Like 'second\.md'))
    Press (Find $T::Button 'Close recent files') | Out-Null
    Press (Find $T::TabItem 'fixture.md') | Out-Null

    Run-Menu 'File' 'Settings…' | Out-Null
    Press (Find $T::CheckBox 'Auto save') | Out-Null
    Press (Find $T::Button 'Close settings') | Out-Null
    # Window messages go to the focused control; put the focus back in the editor first.
    try { (Find $T::Edit 'Editor').SetFocus() } catch { }
    Wait-Until { (Focused-Name) -eq "Edit 'Editor'" } 10 | Out-Null
    [E2e.Win]::Type($window, 'auto ')
    $autosaved = Wait-Until { try { if ([IO.File]::ReadAllText($fixture) -like '*auto*') { $true } } catch { $null } } 10
    Write-Check 'auto save writes about a second after typing stops' ([bool]$autosaved)
    Write-Check 'the auto-saved tab is clean' ((Wait-Until { (Title-Of $app) -eq 'fixture.md - Marky' } 10) -as [bool]) (Title-Of $app)
    [E2e.Win]::Close($window)
    $app.WaitForExit(10000) | Out-Null
    Stop-ProcessTree $app.Id
  } else {
    Write-Check 'feature run started' $false (Title-Of $app)
    Stop-ProcessTree $app.Id
  }

  # ----- Flows this harness cannot drive, kept as explicit skips -----

  Write-Skip 'recovery snapshot and relaunch after a crash' 'restoring asks in a native dialog, which this harness does not drive; store.rs tests the snapshots'
  Write-Skip 'Save As' 'the file dialog is native, which this harness does not drive'
  Write-Skip 'drop overlay' 'the harness cannot start an OS drag session'
  Write-Skip 'IME composition' 'composition cannot be sent as window messages; the IME path is unit-tested in buffer.rs'
}
finally {
  foreach ($process in $script:launched) {
    if (-not $process.HasExited) { Stop-ProcessTree $process.Id }
  }
  Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
}
exit $script:failures
