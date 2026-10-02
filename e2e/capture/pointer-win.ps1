# The real Windows pointer for video captures (e2e/capture/_capture.ts).
# Reads one command per line on stdin, answers one line on stdout:
#   find <window title>     -> ok | err      (remembers the window)
#   size <x> <y> <w> <h>    -> ok            (outer top-left at x,y; CLIENT area w x h; on top)
#   rect                    -> <x> <y> <w> <h>  (client area on screen, physical pixels)
#   screen                  -> <w> <h>       (primary screen, physical pixels)
#   glide <x> <y> <ms>      -> ok            (eased, slightly arced move of the real cursor; ms 0: by distance)
#   down | up               -> ok            (left button)
#   wheel <n>               -> ok            (n notches, + = up)
#   front                   -> ok
# Per-monitor DPI aware, so every figure is in physical pixels - the same
# pixels ffmpeg's gdigrab records.
$ErrorActionPreference = 'Stop'
Add-Type @"
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
public static class P {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hh, uint f);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, int d, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("winmm.dll")] public static extern uint timeBeginPeriod(uint p);
  public static void Glide(int x, int y, int ms) {
    POINT s; GetCursorPos(out s);
    double dx = x - s.X, dy = y - s.Y, dist = Math.Sqrt(dx * dx + dy * dy);
    // ms 0: a hand's pace, longer for longer moves
    if (ms <= 0) ms = (int)Math.Min(1300, Math.Max(450, 350 + dist * 0.45));
    // a slight arc, as a wrist moves, never a ruler line
    double bow = Math.Min(60, dist * 0.08);
    double nx = dist > 0 ? -dy / dist : 0, ny = dist > 0 ? dx / dist : 0;
    var sw = Stopwatch.StartNew();
    while (true) {
      double t = Math.Min(1.0, sw.Elapsed.TotalMilliseconds / Math.Max(1, ms));
      // ease in-out cubic: a hand's start and stop
      double e = t < 0.5 ? 4 * t * t * t : 1 - Math.Pow(-2 * t + 2, 3) / 2;
      double arc = Math.Sin(Math.PI * e) * bow;
      SetCursorPos((int)Math.Round(s.X + dx * e + nx * arc), (int)Math.Round(s.Y + dy * e + ny * arc));
      if (t >= 1.0) break;
      Thread.Sleep(4);
    }
  }
}
"@
# -4 = DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
if (-not [P]::SetProcessDpiAwarenessContext([IntPtr](-4))) { [void][P]::SetProcessDPIAware() }
[void][P]::timeBeginPeriod(1)
$hwnd = [IntPtr]::Zero

function Say($s) { [Console]::Out.WriteLine($s); [Console]::Out.Flush() }

while ($null -ne ($line = [Console]::In.ReadLine())) {
  $a = $line.Trim().Split(' ')
  try {
    switch ($a[0]) {
      'find' {
        # [NullString]: PowerShell passes $null to a string parameter as "", a class name no window has.
        $hwnd = [P]::FindWindow([NullString]::Value, ($a[1..($a.Length - 1)] -join ' '))
        if ($hwnd -eq [IntPtr]::Zero) { Say 'err no window' } else { Say 'ok' }
      }
      'size' {
        [void][P]::ShowWindow($hwnd, 9) # SW_RESTORE: a maximized window ignores sizes
        $w = New-Object P+RECT; $c = New-Object P+RECT
        [void][P]::GetWindowRect($hwnd, [ref]$w); [void][P]::GetClientRect($hwnd, [ref]$c)
        $dw = ($w.R - $w.L) - ($c.R - $c.L); $dh = ($w.B - $w.T) - ($c.B - $c.T)
        # HWND_TOPMOST: Windows may refuse SetForegroundWindow to a background
        # process, and a window left on top would be recorded and clicked.
        [void][P]::SetWindowPos($hwnd, [IntPtr](-1), [int]$a[1], [int]$a[2], [int]$a[3] + $dw, [int]$a[4] + $dh, 0x0040) # SWP_SHOWWINDOW
        [void][P]::SetForegroundWindow($hwnd)
        Say 'ok'
      }
      'rect' {
        $c = New-Object P+RECT; [void][P]::GetClientRect($hwnd, [ref]$c)
        $p = New-Object P+POINT; [void][P]::ClientToScreen($hwnd, [ref]$p)
        Say "$($p.X) $($p.Y) $($c.R - $c.L) $($c.B - $c.T)"
      }
      'screen' { Say "$([P]::GetSystemMetrics(0)) $([P]::GetSystemMetrics(1))" }
      'glide' { [P]::Glide([int]$a[1], [int]$a[2], [int]$a[3]); Say 'ok' }
      'down' { [P]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero); Say 'ok' }
      'up' { [P]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero); Say 'ok' }
      'wheel' { [P]::mouse_event(0x0800, 0, 0, 120 * [int]$a[1], [UIntPtr]::Zero); Say 'ok' }
      'front' { [void][P]::SetForegroundWindow($hwnd); Say 'ok' }
      default { Say "err unknown $($a[0])" }
    }
  } catch { Say "err $($_.Exception.Message)" }
}
