# Видимые окна процесса и окна Проводника с их координатами (проверка панели у Проводника).
#   .\scripts\win-rects.ps1 -ProcessId 1234 [-ExplorerTitle "A Общая"]
param([int]$ProcessId, [string]$ExplorerTitle)
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices; using System.Collections.Generic;
public static class Wn {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  public struct RECT { public int L, T, R, B; }
  public static List<string> List(uint want, string title) {
    var res = new List<string>(); var fg = GetForegroundWindow();
    EnumWindows((h, l) => {
      if (!IsWindowVisible(h)) return true;
      uint pid; GetWindowThreadProcessId(h, out pid);
      var t = new StringBuilder(256); GetWindowText(h, t, 256);
      var c = new StringBuilder(256); GetClassName(h, c, 256);
      bool mine = pid == want, exp = c.ToString() == "CabinetWClass" && title != null && t.ToString().Contains(title);
      if (mine || exp) { RECT r; GetWindowRect(h, out r);
        res.Add(String.Format("{0} [{1}] '{2}' {3},{4} {5}x{6}{7}", mine ? "ПАНЕЛЬ" : "ПРОВОДНИК", c, t, r.L, r.T, r.R - r.L, r.B - r.T, h == fg ? " (активно)" : "")); }
      return true; }, IntPtr.Zero);
    return res; } }
"@
[Wn]::SetProcessDPIAware() | Out-Null
[Wn]::List([uint32]$ProcessId, $ExplorerTitle)
