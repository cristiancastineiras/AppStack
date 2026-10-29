param(
    [string]$Executable = (Join-Path $PSScriptRoot '..\target\release\AppStackClassic.exe'),
    [string]$BaselineExecutable,
    [ValidateRange(3,30)][int]$Samples = 9,
    [ValidateRange(1,10000)][int]$Applications = 8,
    [switch]$Resident,
    [switch]$MoveCursor = $true,
    [string]$PinnedShortcut,
    [switch]$Legacy,
    [switch]$AllowAnchorMismatch,
    [switch]$HelpersOnly
)
$ErrorActionPreference = 'Stop'
if (-not ('AppStackStartupV5' -as [type])) {
    Add-Type @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class AppStackStartupV5 {
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] struct Monitor { public uint Size; public Rect Bounds, Work; public uint Flags; }
    [StructLayout(LayoutKind.Sequential)] struct Message { public IntPtr Window; public uint Code; public UIntPtr First; public IntPtr Second; public uint Time; public Point Position; public uint Private; }
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct ShellInfo {
        public int Size; public uint Mask; public IntPtr Window;
        [MarshalAs(UnmanagedType.LPWStr)] public string Verb, File, Parameters, Directory;
        public int Show; public IntPtr Instance, IdList;
        [MarshalAs(UnmanagedType.LPWStr)] public string Class;
        public IntPtr ClassKey; public uint HotKey; public IntPtr Icon, Process;
    }
    [StructLayout(LayoutKind.Sequential)] struct MouseInput { public int X, Y; public uint Data, Flags, Time; public UIntPtr Extra; }
    [StructLayout(LayoutKind.Sequential)] struct Input { public uint Type; public MouseInput Mouse; }
    public sealed class Sample {
        public int process;
        public double shell_ms, visible_ms, painted_ms, milliseconds;
        public bool anchored;
        public Point origin, moved;
        public Rect popup;
    }
    delegate void WinEvent(IntPtr hook, uint code, IntPtr window, int objectId, int childId, uint threadId, uint time);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr FindWindowEx(IntPtr parent, IntPtr previous, string className, string title);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr GetProp(IntPtr window, string name);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr window, out Rect bounds);
    [DllImport("user32.dll")] static extern bool GetCursorPos(out Point point);
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern uint SendInput(uint count, Input[] input, int size);
    [DllImport("user32.dll")] static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll")] static extern IntPtr MonitorFromPoint(Point point, uint flags);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern bool GetMonitorInfo(IntPtr monitor, ref Monitor info);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr window, uint message, IntPtr first, IntPtr second);
    [DllImport("user32.dll")] static extern IntPtr SendMessageTimeout(IntPtr window, uint message, IntPtr first, IntPtr second, uint flags, uint timeout, out UIntPtr result);
    [DllImport("shell32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool ShellExecuteEx(ref ShellInfo info);
    [DllImport("shell32.dll", CharSet=CharSet.Unicode)] static extern IntPtr CommandLineToArgvW(string command, out int count);
    [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr memory);
    [DllImport("kernel32.dll")] static extern uint GetProcessId(IntPtr process);
    [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr handle, uint timeout);
    [DllImport("kernel32.dll")] static extern bool GetExitCodeProcess(IntPtr process, out uint code);
    [DllImport("kernel32.dll")] static extern bool TerminateProcess(IntPtr process, uint code);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("dwmapi.dll")] static extern int DwmFlush();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] static extern IntPtr SetWinEventHook(uint minimum, uint maximum, IntPtr module, WinEvent callback, uint process, uint thread, uint flags);
    [DllImport("user32.dll")] static extern bool UnhookWinEvent(IntPtr hook);
    [DllImport("user32.dll")] static extern uint MsgWaitForMultipleObjectsEx(uint count, IntPtr handles, uint timeout, uint mask, uint flags);
    [DllImport("user32.dll")] static extern bool PeekMessage(out Message message, IntPtr window, uint minimum, uint maximum, uint remove);
    [DllImport("user32.dll")] static extern bool TranslateMessage(ref Message message);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessage(ref Message message);
    public static Point Cursor() { Point point; GetCursorPos(out point); return point; }
    public static string[] Arguments(string command) {
        int count; IntPtr memory=CommandLineToArgvW("app " + command,out count);
        if (memory == IntPtr.Zero) throw new Exception("Shortcut arguments are invalid");
        try { string[] arguments=new string[count-1]; for (int index=1; index<count; index++) arguments[index-1]=Marshal.PtrToStringUni(Marshal.ReadIntPtr(memory,index*IntPtr.Size)); return arguments; }
        finally { LocalFree(memory); }
    }
    public static void RestoreCursor(Point point) { SetCursorPos(point.X, point.Y); }
    static void MovePointer(Point point) {
        SetCursorPos(point.X, point.Y);
        Input input = new Input { Mouse = new MouseInput {
            X=(int)Math.Round((point.X - GetSystemMetrics(76)) * 65535.0 / (GetSystemMetrics(78) - 1)),
            Y=(int)Math.Round((point.Y - GetSystemMetrics(77)) * 65535.0 / (GetSystemMetrics(79) - 1)),
            Flags=0xC001
        }};
        if (SendInput(1, new Input[] { input }, Marshal.SizeOf(typeof(Input))) != 1) throw new Exception("Mouse input injection failed");
    }
    static IntPtr Launch(string shortcut) {
        ShellInfo info = new ShellInfo { Size=Marshal.SizeOf(typeof(ShellInfo)), Mask=0x540, Verb="open", File=shortcut, Show=1 };
        if (!ShellExecuteEx(ref info)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        if (info.Process == IntPtr.Zero) throw new Exception("Shell did not return a launch process");
        return info.Process;
    }
    static IntPtr FindOwned(string className, string title, int process, bool visible) {
        IntPtr window = IntPtr.Zero;
        while ((window = FindWindowEx(IntPtr.Zero, window, className, title)) != IntPtr.Zero) {
            uint owner; GetWindowThreadProcessId(window, out owner);
            if (owner == process && IsWindowVisible(window) == visible) return window;
        }
        return IntPtr.Zero;
    }
    static void WaitFor(Func<bool> condition, int process) {
        WinEvent callback = delegate { };
        IntPtr hook = SetWinEventHook(0x8000, 0x800F, IntPtr.Zero, callback, (uint)process, 0, 0);
        if (hook == IntPtr.Zero) throw new Exception("Could not observe native window events");
        Stopwatch timer = Stopwatch.StartNew();
        try {
            while (!condition()) {
                int remaining = 10000 - (int)timer.ElapsedMilliseconds;
                if (remaining <= 0) throw new Exception("Timed out waiting for the popup's first paint or visibility change");
                MsgWaitForMultipleObjectsEx(0, IntPtr.Zero, (uint)remaining, 0x04FF, 4);
                Message message;
                while (PeekMessage(out message, IntPtr.Zero, 0, 0, 1)) { TranslateMessage(ref message); DispatchMessage(ref message); }
            }
        } finally { UnhookWinEvent(hook); GC.KeepAlive(callback); }
    }
    public static int StartResident(string shortcut) {
        return StartResident(shortcut,true);
    }
    public static int StartResident(string shortcut, bool requireReady) {
        IntPtr process = Launch(shortcut);
        try {
            int identifier = (int)GetProcessId(process);
            IntPtr host = IntPtr.Zero;
            WaitFor(delegate { host=FindOwned(null,"AppStackClassic dispatcher",identifier,false); return host != IntPtr.Zero && (!requireReady || GetProp(host,"AppStack.DispatcherReady") != IntPtr.Zero); },identifier);
            UIntPtr result;
            if (SendMessageTimeout(host,0,IntPtr.Zero,IntPtr.Zero,2,10000,out result) == IntPtr.Zero) throw new Exception("Dispatcher is not ready");
            return identifier;
        } catch { TerminateProcess(process,1); throw; }
        finally { CloseHandle(process); }
    }
    public static void Hide(string title, int process) {
        IntPtr window = FindOwned("AppStack.Popup",title,process,true);
        if (window == IntPtr.Zero) return;
        PostMessage(window,0x100,new IntPtr(27),IntPtr.Zero);
        WaitFor(delegate { return FindOwned("AppStack.Popup",title,process,false) != IntPtr.Zero; },process);
    }
    static int Clamp(int value, int minimum, int maximum) { return Math.Min(Math.Max(value, minimum), maximum); }
    static bool AtOrigin(IntPtr window, Point point, Rect bounds) {
        Monitor monitor = new Monitor { Size=(uint)Marshal.SizeOf(typeof(Monitor)) };
        if (!GetMonitorInfo(MonitorFromPoint(point,2),ref monitor)) throw new Exception("Monitor work area is unavailable");
        Rect work = monitor.Work;
        int width=bounds.Right-bounds.Left, height=bounds.Bottom-bounds.Top;
        int gap=(8 * (int)GetDpiForWindow(window) + 48) / 96;
        int left=point.X < work.Left ? work.Left+gap : point.X >= work.Right ? work.Right-width-gap : point.X-width/2;
        int top=point.Y >= work.Bottom ? work.Bottom-height-gap : point.Y < work.Top ? work.Top+gap : point.Y-height-gap;
        left=Clamp(left,work.Left,Math.Max(work.Left,work.Right-width));
        top=Clamp(top,work.Top,Math.Max(work.Top,work.Bottom-height));
        return Math.Abs(bounds.Left-left) <= 2 && Math.Abs(bounds.Top-top) <= 2;
    }
    public static Sample Click(string title, int owner, int left, int top, bool moveCursor, bool requirePaint) {
        IntPtr dpi=SetThreadDpiAwarenessContext(new IntPtr(-4));
        try {
            Point origin=new Point { X=left, Y=top };
            MovePointer(origin);
            Monitor monitor=new Monitor { Size=(uint)Marshal.SizeOf(typeof(Monitor)) };
            GetMonitorInfo(MonitorFromPoint(origin,2),ref monitor);
            Point moved=new Point { X=monitor.Work.Left+(monitor.Work.Right-monitor.Work.Left)*3/4, Y=monitor.Work.Top+(monitor.Work.Bottom-monitor.Work.Top)/4 };
            Input[] click={new Input { Mouse=new MouseInput { Flags=2 } },new Input { Mouse=new MouseInput { Flags=4 } }};
            Stopwatch timer=Stopwatch.StartNew();
            if (SendInput(2,click,Marshal.SizeOf(typeof(Input))) != 2) throw new Exception("Taskbar click injection failed");
            if (moveCursor) MovePointer(moved);
            IntPtr window=IntPtr.Zero;
            WaitFor(delegate { window=FindOwned("AppStack.Popup",title,owner,true); return window != IntPtr.Zero; },owner);
            double visible=timer.Elapsed.TotalMilliseconds;
            if (requirePaint) WaitFor(delegate { return IsWindowVisible(window) && GetProp(window,"AppStack.PopupReady") != IntPtr.Zero; },owner);
            else { UIntPtr result; if (SendMessageTimeout(window,0,IntPtr.Zero,IntPtr.Zero,2,10000,out result) == IntPtr.Zero) throw new Exception("Popup is not responsive"); }
            double painted=timer.Elapsed.TotalMilliseconds;
            if (DwmFlush() < 0) throw new Exception("Desktop composition is unavailable");
            double elapsed=timer.Elapsed.TotalMilliseconds;
            Rect bounds;
            if (!GetWindowRect(window,out bounds)) throw new Exception("Popup bounds are unavailable");
            return new Sample { process=owner, visible_ms=visible, painted_ms=painted, milliseconds=elapsed,
                origin=origin, moved=moveCursor ? moved : origin, popup=bounds, anchored=AtOrigin(window,origin,bounds) };
        } finally { SetThreadDpiAwarenessContext(dpi); }
    }
    public static Sample Open(string shortcut, string title, int resident, bool moveCursor) {
        IntPtr dpi = SetThreadDpiAwarenessContext(new IntPtr(-4));
        IntPtr launched = IntPtr.Zero;
        try {
            Rect taskbar;
            IntPtr taskbarWindow=FindWindowEx(IntPtr.Zero,IntPtr.Zero,"Shell_TrayWnd",null);
            if (taskbarWindow == IntPtr.Zero || !GetWindowRect(taskbarWindow,out taskbar)) throw new Exception("The desktop taskbar is unavailable");
            MovePointer(new Point { X=(taskbar.Left+taskbar.Right)/2, Y=(taskbar.Top+taskbar.Bottom)/2 });
            Point origin=Cursor();
            Monitor monitor=new Monitor { Size=(uint)Marshal.SizeOf(typeof(Monitor)) };
            GetMonitorInfo(MonitorFromPoint(origin,2),ref monitor);
            Point moved=new Point { X=monitor.Work.Left+(monitor.Work.Right-monitor.Work.Left)/4, Y=monitor.Work.Top+(monitor.Work.Bottom-monitor.Work.Top)/4 };
            Stopwatch timer=Stopwatch.StartNew();
            launched=Launch(shortcut);
            double shell=timer.Elapsed.TotalMilliseconds;
            int owner=resident == 0 ? (int)GetProcessId(launched) : resident;
            if (moveCursor) MovePointer(moved);
            IntPtr window=IntPtr.Zero;
            WaitFor(delegate { window=FindOwned("AppStack.Popup",title,owner,true); return window != IntPtr.Zero; },owner);
            double visible=timer.Elapsed.TotalMilliseconds;
            WaitFor(delegate { return IsWindowVisible(window) && GetProp(window,"AppStack.PopupReady") != IntPtr.Zero; },owner);
            double painted=timer.Elapsed.TotalMilliseconds;
            if (DwmFlush() < 0) throw new Exception("Desktop composition is unavailable");
            double elapsed=timer.Elapsed.TotalMilliseconds;
            Rect bounds;
            if (!GetWindowRect(window,out bounds)) throw new Exception("Popup bounds are unavailable");
            Sample sample=new Sample { process=owner, shell_ms=shell, visible_ms=visible, painted_ms=painted, milliseconds=elapsed,
                origin=origin, moved=moveCursor ? moved : origin, popup=bounds, anchored=AtOrigin(window,origin,bounds) };
            if (resident != 0) {
                uint exit;
                if (WaitForSingleObject(launched,5000) != 0 || !GetExitCodeProcess(launched,out exit) || exit != 0) throw new Exception("Shortcut forwarding failed");
            }
            return sample;
        } catch { if (resident == 0 && launched != IntPtr.Zero) { TerminateProcess(launched,1); WaitForSingleObject(launched,5000); } throw; }
        finally { if (launched != IntPtr.Zero) CloseHandle(launched); SetThreadDpiAwarenessContext(dpi); }
    }
}
'@
}
$native = 'AppStackStartupV5' -as [type]
if ($HelpersOnly) { return }
if (-not $PinnedShortcut -and -not $PSBoundParameters.ContainsKey('MoveCursor')) { $MoveCursor = [switch]$false }
& {
    if ($PinnedShortcut) {
        if (-not $Resident) { throw 'Use -Resident for a non-destructive test of an existing taskbar profile' }
        $PinnedShortcut = (Resolve-Path $PinnedShortcut).Path
        $root = Join-Path (Join-Path $PSScriptRoot '..\target') ('taskbar-' + [guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $root | Out-Null
        $root = (Resolve-Path $root).Path
        $shell = New-Object -ComObject WScript.Shell
        $link = $shell.CreateShortcut($PinnedShortcut)
        $target = $link.TargetPath
        $arguments = $native::Arguments($link.Arguments)
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($link)
        $groupIndex = [array]::IndexOf($arguments,'--group')
        $dataIndex = [array]::IndexOf($arguments,'--data-dir')
        if ($groupIndex -lt 0 -or $dataIndex -lt 0) { throw 'The pinned shortcut must specify --group and --data-dir' }
        $groupId = $arguments[$groupIndex + 1]
        $data = $arguments[$dataIndex + 1]
        if ($data.StartsWith('\\?\UNC\',[StringComparison]::OrdinalIgnoreCase)) { $data = '\\' + $data.Substring(8) }
        elseif ($data.StartsWith('\\?\',[StringComparison]::Ordinal)) { $data = $data.Substring(4) }
        $config = Get-Content (Join-Path $data 'appstack.json') -Raw | ConvertFrom-Json
        $group = @($config.groups | Where-Object { $_.id -eq $groupId -or $_.name -eq $groupId })
        if ($group.Count -ne 1) { throw 'Pinned group is ambiguous or missing' }
        Add-Type -AssemblyName UIAutomationClient
        Add-Type -AssemblyName UIAutomationTypes
        $buttonCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty,('Appid: AppStackClassic.Group.' + $group[0].id))
        $barCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Shell_TrayWnd')
        $bar = [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children,$barCondition)
        $button = $bar.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$buttonCondition)
        if ($null -eq $button) { throw 'The shortcut is not present on the primary taskbar' }
        $owners = @(Get-CimInstance Win32_Process -Filter "Name='AppStackClassic.exe'" | Where-Object { $_.ExecutablePath -eq $target -and $_.CommandLine.IndexOf($data,[StringComparison]::OrdinalIgnoreCase) -ge 0 })
        if ($owners.Count -gt 1) { throw 'Multiple profile owners were found' }
        $owned = $null
        $residentPreparationMs = 0
        $clickSamples = @()
        $cursor = $native::Cursor()
        try {
            if ($owners.Count -eq 1) { $identifier = [int]$owners[0].ProcessId }
            else {
                $silentPath = Join-Path $root 'silent.lnk'
                $silent = $shell.CreateShortcut($silentPath)
                $silent.TargetPath = $target
                $silent.Arguments = '--silent --data-dir "' + $data + '"'
                $silent.Save()
                [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($silent)
                $preparation = [Diagnostics.Stopwatch]::StartNew()
                $identifier = $native::StartResident($silentPath,(-not $Legacy))
                $residentPreparationMs = [math]::Round($preparation.Elapsed.TotalMilliseconds,3)
                $owned = [Diagnostics.Process]::GetProcessById($identifier)
            }
            $native::Hide($group[0].name,$identifier)
            foreach ($iteration in 0..$Samples) {
                $bounds = $button.Current.BoundingRectangle
                $sample = $native::Click($group[0].name,$identifier,[int]($bounds.Left+$bounds.Width/2),[int]($bounds.Top+$bounds.Height/2),$MoveCursor.IsPresent,(-not $Legacy))
                $clickSamples += [pscustomobject]@{sample=$iteration;milliseconds=[math]::Round($sample.milliseconds,3);visible_ms=[math]::Round($sample.visible_ms,3);painted_ms=[math]::Round($sample.painted_ms,3);anchored=$sample.anchored;origin=$sample.origin;moved=$sample.moved;popup=$sample.popup}
                $native::Hide($group[0].name,$identifier)
                if (-not $sample.anchored -and -not $AllowAnchorMismatch) { throw 'The popup did not stay anchored to its taskbar button' }
            }
            $times = @($clickSamples | Where-Object sample -gt 0 | Select-Object -ExpandProperty milliseconds | Sort-Object)
            $summary = [pscustomobject]@{mode='physical_taskbar';applications=$group[0].entries.Count;samples=$times.Count;resident_preparation_ms=$residentPreparationMs;first_popup_ms=$clickSamples[0].milliseconds;median_ms=$times[[int][math]::Floor($times.Count/2)];p95_ms=$times[[int][math]::Ceiling($times.Count*0.95)-1];anchor_failures=@($clickSamples | Where-Object { -not $_.anchored }).Count;first_paint_verified=(-not $Legacy)}
            $summary | Format-List
        } finally {
            $native::RestoreCursor($cursor)
            [IO.File]::WriteAllText((Join-Path $root 'results.json'),(@{summary=$summary;measurements=$clickSamples;shortcut=$PinnedShortcut;definition='Physical SendInput click on the existing pinned taskbar button, optional immediate cursor movement, and next DWM presentation. Legacy mode cannot verify the first-paint marker. Sample zero is retained but excluded from steady-state statistics.'} | ConvertTo-Json -Depth 8))
            [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell)
            if ($owned -and -not $owned.HasExited) { $quit = Start-Process $target -ArgumentList @('--quit','--data-dir',('"' + $data + '"')) -PassThru; if (-not $quit.WaitForExit(5000) -or -not $owned.WaitForExit(10000)) { Stop-Process -Id $owned.Id -Force } }
            Write-Output ('Results: ' + $root)
        }
        return
    }
    $Executable = (Resolve-Path $Executable).Path
    $binaries = @{ optimized = $Executable }
    if ($BaselineExecutable) { $binaries.baseline = (Resolve-Path $BaselineExecutable).Path }
    $root = Join-Path (Join-Path $PSScriptRoot '..\target') ('startup-' + [guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $root | Out-Null
    $root = (Resolve-Path $root).Path
    $group = [guid]::NewGuid().ToString()
    $title = 'Tools'
    $applicationPaths = @(
        'notepad.exe','calc.exe','cmd.exe','control.exe','mstsc.exe','Taskmgr.exe',
        'charmap.exe','regedt32.exe','mmc.exe','msinfo32.exe','perfmon.exe',
        'WindowsPowerShell\v1.0\powershell.exe'
    ) | ForEach-Object { Join-Path (Join-Path $env:WINDIR 'System32') $_ } | Where-Object { Test-Path $_ }
    $applicationPaths = @((Join-Path $env:WINDIR 'explorer.exe')) + @($applicationPaths)
    $entries = @(1..$Applications | ForEach-Object {
        $path = $applicationPaths[($_ - 1) % $applicationPaths.Count]
        @{id=[guid]::NewGuid().ToString();name=[IO.Path]::GetFileNameWithoutExtension($path);target=@{kind='Application';value=$path}}
    })
    $configuration = @{schema_version=2;settings=@{show_tray=$false;check_updates=$false};groups=@(@{id=$group;name=$title;appearance=@{columns=4;show_labels=$true};entries=$entries})}
    $profiles = @{}
    $shortcuts = @{}
    $silentShortcuts = @{}
    $residents = @{}
    $firstPopups = @{}
    $shell = New-Object -ComObject WScript.Shell
    foreach ($variant in $binaries.Keys) {
        $profile = Join-Path $root $variant
        New-Item -ItemType Directory -Path $profile | Out-Null
        $profiles[$variant] = $profile
        [IO.File]::WriteAllText((Join-Path $profile 'appstack.json'),($configuration | ConvertTo-Json -Depth 12),(New-Object Text.UTF8Encoding($false)))
        foreach ($mode in @('group','silent')) {
            $path = Join-Path $profile ($mode + '.lnk')
            $link = $shell.CreateShortcut($path)
            $link.TargetPath = $binaries[$variant]
            $link.Arguments = $(if ($mode -eq 'group') { '--group ' + $group + ' ' } else { '--silent ' }) + '--data-dir "' + $profile + '"'
            $link.WorkingDirectory = Split-Path $binaries[$variant]
            $link.Save()
            [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($link)
            if ($mode -eq 'group') { $shortcuts[$variant] = $path } else { $silentShortcuts[$variant] = $path }
        }
    }
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell)
    $measurements = @()
    $previousTrace = $env:APPSTACK_STARTUP_TRACE
    $cursor = $native::Cursor()
    Remove-Item Env:\APPSTACK_STARTUP_TRACE -ErrorAction SilentlyContinue
    try {
        if ($Resident) {
            foreach ($variant in $binaries.Keys) {
                $identifier = $native::StartResident($silentShortcuts[$variant])
                $residents[$variant] = [Diagnostics.Process]::GetProcessById($identifier)
                $first = $native::Open($shortcuts[$variant],$title,$identifier,$MoveCursor.IsPresent)
                $firstPopups[$variant] = $first
                if (-not $first.anchored) { throw ('First popup moved away from its launch position: ' + $variant) }
                $native::Hide($title,$identifier)
            }
        }
        foreach ($iteration in 0..$Samples) {
            $order = @($binaries.Keys | Sort-Object)
            if ($iteration % 2 -eq 1) { [array]::Reverse($order) }
            foreach ($variant in $order) {
                $process = $null
                try {
                    $identifier = if ($Resident) { $residents[$variant].Id } else { 0 }
                    $sample = $native::Open($shortcuts[$variant],$title,$identifier,$MoveCursor.IsPresent)
                    if (-not $Resident) { $process = [Diagnostics.Process]::GetProcessById($sample.process) }
                    $measurements += [pscustomobject]@{variant=$variant;sample=$iteration;
                        milliseconds=[math]::Round($sample.milliseconds,3);shell_ms=[math]::Round($sample.shell_ms,3);
                        visible_ms=[math]::Round($sample.visible_ms,3);painted_ms=[math]::Round($sample.painted_ms,3);
                        anchored=$sample.anchored;origin=$sample.origin;moved=$sample.moved;popup=$sample.popup}
                    if (-not $sample.anchored) { throw ('Popup moved away from its launch position: ' + $variant) }
                    if ($Resident) { $native::Hide($title,$identifier) }
                } finally {
                    if ($process -and -not $process.HasExited) {
                        $quit = Start-Process $binaries[$variant] -ArgumentList @('--quit','--data-dir',('"' + $profiles[$variant] + '"')) -PassThru
                        if (-not $quit.WaitForExit(5000)) { throw 'Quit command timed out' }
                        if (-not $process.WaitForExit(10000)) { Stop-Process -Id $process.Id -Force; throw 'AppStack did not shut down' }
                        if ($process.ExitCode -ne 0) { throw ('AppStack exited with code ' + $process.ExitCode) }
                    }
                }
            }
        }
    } finally {
        foreach ($variant in $residents.Keys) {
            $process = $residents[$variant]
            if (-not $process.HasExited) {
                $quit = Start-Process $binaries[$variant] -ArgumentList @('--quit','--data-dir',('"' + $profiles[$variant] + '"')) -PassThru
                if (-not $quit.WaitForExit(5000) -or -not $process.WaitForExit(10000)) { Stop-Process -Id $process.Id -Force }
            }
        }
        $native::RestoreCursor($cursor)
        $env:APPSTACK_STARTUP_TRACE = $previousTrace
        [IO.File]::WriteAllText((Join-Path $root 'samples.json'),($measurements | ConvertTo-Json -Depth 8))
        Write-Output ('Results: ' + $root)
    }
    $summary = @($measurements | Where-Object sample -gt 0 | Group-Object variant | ForEach-Object {
        $times = @($_.Group.milliseconds | Sort-Object)
        $shellTimes = @($_.Group.shell_ms | Sort-Object)
        $paintTimes = @($_.Group.painted_ms | Sort-Object)
        $variant = $_.Name
        $first = if ($Resident) { $firstPopups[$variant].milliseconds } else { ($measurements | Where-Object { $_.variant -eq $variant -and $_.sample -eq 0 }).milliseconds }
        [pscustomobject]@{variant=$_.Name;mode=$(if ($Resident) { 'resident' } else { 'cold' });applications=$Applications;samples=$times.Count;
            first_popup_ms=[math]::Round($first,3);shell_median_ms=$shellTimes[[int][math]::Floor($times.Count / 2)];paint_median_ms=$paintTimes[[int][math]::Floor($times.Count / 2)];
            anchor_failures=@($_.Group | Where-Object { -not $_.anchored }).Count;
            median_ms=$times[[int][math]::Floor($times.Count / 2)];
            p95_ms=$times[[int][math]::Ceiling($times.Count * 0.95) - 1];
            minimum_ms=$times[0];maximum_ms=$times[-1]}
    })
    $definition = 'Native ShellExecuteEx of a .lnk without --position, to the popup first paint and next DWM presentation. This is the shortcut route, not an automated physical Explorer taskbar-button click. OS caches are retained. Sample zero is retained but excluded from steady-state statistics. Resident instances stay alive across samples.'
    [IO.File]::WriteAllText((Join-Path $root 'results.json'),(@{summary=$summary;measurements=$measurements;first_popups=$firstPopups;move_cursor=$MoveCursor.IsPresent;definition=$definition} | ConvertTo-Json -Depth 8))
    $summary | Format-List
}