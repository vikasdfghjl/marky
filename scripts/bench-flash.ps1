<#
.SYNOPSIS
  Times how long the window shows white before the UI paints (the launch "flash" in dark mode).
.DESCRIPTION
  Launches the release build repeatedly and samples one pixel inside the window as fast as it can;
  a pixel read costs microseconds, so the app is not slowed. Needs a visible desktop (not for CI).
  With Windows in dark mode the expected result is "white for 0 ms": window.rs creates the window
  with the theme's background colour. A config-created window showed about 650 ms of white.
  Run it with the theme setting on System (or Dark) and Windows in dark mode.
.EXAMPLE
  npx tauri build --no-bundle; ./scripts/bench-flash.ps1
#>
param(
  [string]$File = (Join-Path (Split-Path $PSScriptRoot -Parent) 'README.md'),
  [int]$Runs = 5,
  [string]$Exe = (Join-Path (Split-Path $PSScriptRoot -Parent) 'src-tauri\target\release\marky.exe')
)
if (Get-Process marky -ErrorAction SilentlyContinue) { throw 'Close all Marky windows first.' }
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class Px {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr h);
  [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr h, IntPtr dc);
  [DllImport("gdi32.dll")] static extern uint GetPixel(IntPtr dc, int x, int y);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  static RECT last; static bool have;
  public static bool Window(uint pid, out RECT rect) {
    have = false;
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p != pid || !IsWindowVisible(h)) return true;
      RECT r; GetWindowRect(h, out r);
      if (r.R - r.L < 200 || r.B - r.T < 200) return true;
      last = r; have = true; return false;
    }, IntPtr.Zero);
    rect = last; return have;
  }
  /// 0 = no window, else 0xRRGGBB of a point inside the content area.
  public static int Sample(uint pid) {
    RECT r; if (!Window(pid, out r)) return -1;
    IntPtr dc = GetDC(IntPtr.Zero);
    uint c = GetPixel(dc, r.L + 400, r.T + 400); ReleaseDC(IntPtr.Zero, dc);
    return (int)((c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16 & 0xFF));
  }
}
'@
function Classify([int]$rgb) {
  if ($rgb -lt 0) { return 'no-window' }
  $r = ($rgb -shr 16) -band 255; $g = ($rgb -shr 8) -band 255; $b = $rgb -band 255
  if ($r -gt 235 -and $g -gt 235 -and $b -gt 235) { return 'WHITE' }
  if ($r -lt 60 -and $g -lt 60 -and $b -lt 60) { return 'dark' }
  return ('other {0:X6}' -f $rgb)
}
for ($run = 1; $run -le $Runs; $run++) {
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $app = Start-Process $Exe -ArgumentList "`"$File`"" -PassThru
  $events = @(); $prev = ''; $whiteStart = $null; $whiteMs = 0; $firstWindow = $null
  while ($sw.ElapsedMilliseconds -lt 2500) {
    $cls = Classify ([Px]::Sample([uint32]$app.Id))
    if ($cls -ne $prev) {
      $t = $sw.ElapsedMilliseconds
      $events += "{0}ms {1}" -f $t, $cls
      if ($cls -ne 'no-window' -and $null -eq $firstWindow) { $firstWindow = $t }
      if ($cls -eq 'WHITE') { $whiteStart = $t } elseif ($whiteStart) { $whiteMs += $t - $whiteStart; $whiteStart = $null }
      $prev = $cls
    }
  }
  Get-Process marky -ErrorAction SilentlyContinue | ForEach-Object { Stop-Process -Id $_.Id -Force }
  Start-Sleep -Seconds 2
  "run {0}: window first visible at {1} ms, white for {2} ms | {3}" -f $run, $firstWindow, $whiteMs, ($events -join ' > ')
}
