
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class TupaEnergy {
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
    static int _cursorX, _cursorY; static bool _cursorSaved;
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder sb, int max);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
    public static void WinA() {
        keybd_event(0x5B,0,0,UIntPtr.Zero); keybd_event(0x41,0,0,UIntPtr.Zero);
        keybd_event(0x41,0,2,UIntPtr.Zero); keybd_event(0x5B,0,2,UIntPtr.Zero);
    }
    public static string FgClass() {
        var sb = new StringBuilder(256);
        GetClassName(GetForegroundWindow(), sb, 256);
        return sb.ToString();
    }
    public static void Click(int x, int y) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(120);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
    }
    public static void SaveCursor() {
        POINT p;
        if (GetCursorPos(out p)) { _cursorX = p.X; _cursorY = p.Y; _cursorSaved = true; }
    }
    public static void RestoreCursor() {
        if (!_cursorSaved) { return; }
        _cursorSaved = false;
        SetCursorPos(_cursorX, _cursorY);
    }
}
'@
[TupaEnergy]::SetProcessDPIAware() | Out-Null
$key = 'HKLM:\SYSTEM\CurrentControlSet\Control\Power'
function Get-State { (Get-ItemProperty $key -Name EnergySaverState).EnergySaverState }
function Is-Open { [TupaEnergy]::FgClass() -eq 'ControlCenterWindow' }
function Wait-Open {
    for ($i = 0; $i -lt 40; $i++) { if (Is-Open) { return $true }; Start-Sleep -Milliseconds 50 }
    return $false
}
function Wait-State([int]$want) {
    for ($i = 0; $i -lt 30; $i++) { if ((Get-State) -eq $want) { return $true }; Start-Sleep -Milliseconds 50 }
    return $false
}
# 快速设置里「节能模式」按钮的中心：相对屏幕右下角的逻辑像素偏移，按 DPI 缩放
function Click-Energy {
    $scale = [TupaEnergy]::GetDpiForSystem() / 96.0
    $x = [int]([TupaEnergy]::GetSystemMetrics(0) - 89 * $scale)
    $y = [int]([TupaEnergy]::GetSystemMetrics(1) - 311 * $scale)
    [TupaEnergy]::Click($x, $y)
}
