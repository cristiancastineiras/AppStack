param([string]$Executable = (Join-Path $PSScriptRoot '..\target\debug\AppStackClassic.exe'), [switch]$HelpersOnly, [switch]$EditorOnly, [ValidateSet('System','Light','Dark')][string]$Theme = 'System')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName Accessibility
if (-not ('AppStackList' -as [type])) {
    Add-Type -ReferencedAssemblies ([Accessibility.IAccessible].Assembly.Location) -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class AppStackList {
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr window, uint message, IntPtr first, IntPtr second);
    [DllImport("oleacc.dll")] static extern int AccessibleObjectFromWindow(IntPtr window, uint objectId, ref Guid identifier, [MarshalAs(UnmanagedType.Interface)] out object value);
    static Accessibility.IAccessible Open(IntPtr window) {
        Guid identifier = typeof(Accessibility.IAccessible).GUID; object value;
        Marshal.ThrowExceptionForHR(AccessibleObjectFromWindow(window, 0xFFFFFFFC, ref identifier, out value));
        return (Accessibility.IAccessible)value;
    }
    public static int Find(IntPtr window, string name) {
        Accessibility.IAccessible value = Open(window);
        try { for (int child = 1; child <= value.accChildCount; child++) { if (value.get_accName(child) == name) return child; } return -1; }
        finally { Marshal.ReleaseComObject(value); }
    }
    public static void Invoke(IntPtr window, string name) {
        int child = Find(window, name); if (child < 0) throw new Exception("Accessible application was not found: " + name);
        Accessibility.IAccessible value = Open(window);
        try { value.accSelect(3, child); PostMessage(window, 0x100, new IntPtr(13), IntPtr.Zero); }
        finally { Marshal.ReleaseComObject(value); }
    }
}
'@
}
if (-not ('AppStackAccessibility' -as [type])) {
    Add-Type -ReferencedAssemblies ([Accessibility.IAccessible].Assembly.Location) -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class AppStackAccessibility {
    [DllImport("oleacc.dll")] static extern int AccessibleObjectFromWindow(IntPtr window, uint objectId, ref Guid identifier, [MarshalAs(UnmanagedType.Interface)] out object value);
    public static void Invoke(IntPtr window) {
        Guid identifier = typeof(Accessibility.IAccessible).GUID;
        object value;
        Marshal.ThrowExceptionForHR(AccessibleObjectFromWindow(window, 0xFFFFFFFC, ref identifier, out value));
        try { ((Accessibility.IAccessible)value).accDoDefaultAction(0); }
        finally { Marshal.ReleaseComObject(value); }
    }
}
'@
}
if (-not ('AppStackDpi' -as [type])) {
    Add-Type 'using System; using System.Runtime.InteropServices; public static class AppStackDpi { [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value); }'
}
[AppStackDpi]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
if (-not ('AppStackInput' -as [type])) {
    Add-Type 'using System; using System.Runtime.InteropServices; public static class AppStackInput { [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr first, string text); [DllImport("user32.dll", EntryPoint="SendMessageW")] static extern IntPtr SendPointer(IntPtr window, uint message, IntPtr first, IntPtr text); public static void SetText(IntPtr window, string text) { SendMessage(window, 12, IntPtr.Zero, text); IntPtr buffer = Marshal.AllocHGlobal(16384); try { SendPointer(window, 13, new IntPtr(8192), buffer); if (Marshal.PtrToStringUni(buffer) != text) throw new Exception("Native text edit did not accept the requested value"); } finally { Marshal.FreeHGlobal(buffer); } } }'
}
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class AppStackSmokeV2 {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] struct Message { public IntPtr Window; public uint Code; public UIntPtr First; public IntPtr Second; public uint Time; public Point Position; public uint Private; }
    delegate void WinEvent(IntPtr hook, uint code, IntPtr window, int objectId, int childId, uint threadId, uint time);
    delegate bool EnumWindow(IntPtr window, IntPtr data);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string className, string title);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr parent, IntPtr previous, string className, string title);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rectangle);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point point);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint code, IntPtr first, IntPtr second);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern bool SetWindowText(IntPtr window, string text);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr window, StringBuilder text, int length);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window, StringBuilder text, int length);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, EnumWindow callback, IntPtr data);
    [DllImport("user32.dll")] static extern IntPtr SetWinEventHook(uint minimum, uint maximum, IntPtr module, WinEvent callback, uint process, uint thread, uint flags);
    [DllImport("user32.dll")] static extern bool UnhookWinEvent(IntPtr hook);
    [DllImport("user32.dll")] static extern uint MsgWaitForMultipleObjectsEx(uint count, IntPtr handles, uint timeout, uint mask, uint flags);
    [DllImport("user32.dll")] static extern bool PeekMessage(out Message message, IntPtr window, uint minimum, uint maximum, uint remove);
    [DllImport("user32.dll")] static extern bool TranslateMessage(ref Message message);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessage(ref Message message);
    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);

    public static void WaitFor(Func<bool> condition, int process, int milliseconds) {
        WinEvent callback = delegate { };
        IntPtr hook = SetWinEventHook(1, 0x80FF, IntPtr.Zero, callback, (uint)process, 0, 0);
        Stopwatch timer = Stopwatch.StartNew();
        try {
            while (!condition()) {
                int remaining = milliseconds - (int)timer.ElapsedMilliseconds;
                if (remaining <= 0) throw new Exception("Timed out waiting for a native window event");
                MsgWaitForMultipleObjectsEx(0, IntPtr.Zero, (uint)remaining, 0x04FF, 4);
                Message message;
                while (PeekMessage(out message, IntPtr.Zero, 0, 0, 1)) { TranslateMessage(ref message); DispatchMessage(ref message); }
            }
        } finally { UnhookWinEvent(hook); GC.KeepAlive(callback); }
    }

    public static IntPtr WaitWindow(string className, string title, int process) {
        IntPtr found = IntPtr.Zero;
        WaitFor(delegate {
            IntPtr candidate = IntPtr.Zero;
            while ((candidate = FindWindowEx(IntPtr.Zero, candidate, className, title)) != IntPtr.Zero) {
                uint owner; GetWindowThreadProcessId(candidate, out owner);
                if (owner == process && IsWindowVisible(candidate)) { found = candidate; return true; }
            }
            return false;
        }, process, 10000);
        return found;
    }

    public static void WaitClosed(IntPtr window, int process) {
        WaitFor(delegate { return !IsWindowVisible(window); }, process, 10000);
    }

    public static void WaitForContent(string path, string value) {
        using (FileSystemWatcher watcher = new FileSystemWatcher(Path.GetDirectoryName(path), Path.GetFileName(path)))
        using (AutoResetEvent changed = new AutoResetEvent(false)) {
            watcher.NotifyFilter = NotifyFilters.FileName | NotifyFilters.LastWrite;
            watcher.Changed += delegate { changed.Set(); }; watcher.Created += delegate { changed.Set(); }; watcher.Renamed += delegate { changed.Set(); };
            watcher.EnableRaisingEvents = true;
            Stopwatch timer = Stopwatch.StartNew();
            while (true) {
                try { if (File.Exists(path) && File.ReadAllText(path).Contains(value)) return; } catch (IOException) { }
                int remaining = 10000 - (int)timer.ElapsedMilliseconds;
                if (remaining <= 0 || !changed.WaitOne(remaining)) throw new Exception("Timed out waiting for an atomic configuration save");
            }
        }
    }

    public static int CountButtons(IntPtr window) {
        int count = 0;
        EnumChildWindows(window, delegate(IntPtr child, IntPtr unused) { StringBuilder name = new StringBuilder(128); GetClassName(child, name, name.Capacity); if (name.ToString() == "Button") count++; return true; }, IntPtr.Zero);
        return count;
    }

    public static void Screenshot(IntPtr window, string path) {
        Rect bounds; GetWindowRect(window, out bounds);
        using (Bitmap image = new Bitmap(bounds.Right - bounds.Left, bounds.Bottom - bounds.Top)) {
            using (Graphics graphics = Graphics.FromImage(image)) {
                IntPtr dc = graphics.GetHdc();
                try { if (!PrintWindow(window, dc, 2)) throw new Exception("PrintWindow failed"); }
                finally { graphics.ReleaseHdc(dc); }
            }
            image.Save(path, ImageFormat.Png);
        }
    }
}
'@

if (-not ('AppStackLayoutV4' -as [type])) {
    Add-Type -ReferencedAssemblies ([Accessibility.IAccessible].Assembly.Location) -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class AppStackLayoutV4 {
    [StructLayout(LayoutKind.Sequential)] struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr window, out Rect bounds);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr window, uint message, IntPtr first, IntPtr second);
    [DllImport("user32.dll", EntryPoint="SendMessageW")] static extern IntPtr SendValue(IntPtr window, uint message, IntPtr first, IntPtr second);
    [DllImport("user32.dll", EntryPoint="SendMessageW", CharSet=CharSet.Unicode)] static extern IntPtr FindValue(IntPtr window, uint message, IntPtr first, string value);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr GetProp(IntPtr window, string name);
    public static long PreviewRevision(IntPtr form) { return GetProp(GetDlgItem(form,50),"AppStack.PreviewRevision").ToInt64(); }
    public static void ChoosePreset(IntPtr form, int id, string name) {
        IntPtr control=GetDlgItem(form,id);
        IntPtr index=FindValue(control,0x158,new IntPtr(-1),name);
        if (index.ToInt64() < 0) throw new Exception("Preset is missing: " + name);
        SendValue(control,0x14E,index,IntPtr.Zero);
        PostMessage(form,0x111,new IntPtr(id | 0x10000),control);
    }
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr window, int id);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] static extern int GetWindowRgn(IntPtr window, IntPtr region);
    [DllImport("gdi32.dll")] static extern IntPtr CreateRectRgn(int left, int top, int right, int bottom);
    [DllImport("gdi32.dll")] static extern bool PtInRegion(IntPtr region, int x, int y);
    [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr value);
    [DllImport("oleacc.dll")] static extern int AccessibleObjectFromWindow(IntPtr window, uint objectId, ref Guid identifier, [MarshalAs(UnmanagedType.Interface)] out object value);
    public static void SelectTab(IntPtr form, string name) {
        Guid identifier=typeof(Accessibility.IAccessible).GUID; object value;
        Marshal.ThrowExceptionForHR(AccessibleObjectFromWindow(GetDlgItem(form,40),0xFFFFFFFC,ref identifier,out value));
        Accessibility.IAccessible tabs=(Accessibility.IAccessible)value;
        try {
            for (int child=1; child<=tabs.accChildCount; child++) {
                if (tabs.get_accName(child) == name) {
                    int left, top, width, height; Rect bounds;
                    tabs.accLocation(out left,out top,out width,out height,child);
                    IntPtr tab=GetDlgItem(form,40);
                    if (!GetWindowRect(tab,out bounds)) throw new Exception("Tab bounds are unavailable");
                    int position=((left+width/2-bounds.Left) & 0xFFFF) | ((top+height/2-bounds.Top) << 16);
                    PostMessage(tab,0x201,new IntPtr(1),new IntPtr(position));
                    PostMessage(tab,0x202,IntPtr.Zero,new IntPtr(position));
                    return;
                }
            }
            throw new Exception("Accessible tab is missing: " + name);
        } finally { Marshal.ReleaseComObject(tabs); }
    }
    public static bool HasClippedCorners(IntPtr window) {
        IntPtr region=CreateRectRgn(0,0,1,1);
        try { return GetWindowRgn(window,region) != 0 && !PtInRegion(region,0,0); }
        finally { DeleteObject(region); }
    }
    public static bool HasPillCorners(IntPtr window) {
        Rect bounds; if (!GetWindowRect(window,out bounds)) return false;
        int width=bounds.Right-bounds.Left, height=bounds.Bottom-bounds.Top;
        IntPtr region=CreateRectRgn(0,0,1,1);
        try { return GetWindowRgn(window,region) != 0 && !PtInRegion(region,Math.Min(width,height)/8,1) && PtInRegion(region,width/2,height/2); }
        finally { DeleteObject(region); }
    }
}
'@
}
function Set-EditorPreset([IntPtr]$Window,[int]$Id,[string]$Name,[int]$OwnerProcess) {
    $revision = [AppStackLayoutV4]::PreviewRevision($Window)
    [AppStackLayoutV4]::ChoosePreset($Window,$Id,$Name)
    [AppStackSmokeV2]::WaitFor([Func[bool]]{ return [AppStackLayoutV4]::PreviewRevision($Window) -gt $revision },$OwnerProcess,10000)
}
if ($HelpersOnly) { return }

if ($EditorOnly) {
    & {
        $Executable = (Resolve-Path $Executable).Path
        $root = Join-Path (Join-Path $PSScriptRoot '..\target') ('preview-' + [guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $root | Out-Null
        $root = (Resolve-Path $root).Path
        $groupId = [guid]::NewGuid().ToString()
        $entries = @('notepad.exe','calc.exe','cmd.exe','control.exe') | ForEach-Object {
            @{id=[guid]::NewGuid().ToString();name=[IO.Path]::GetFileNameWithoutExtension($_);target=@{kind='Application';value=(Join-Path (Join-Path $env:WINDIR 'System32') $_)}}
        }
        $configPath = Join-Path $root 'appstack.json'
        $config = @{schema_version=2;settings=@{show_tray=$false;main_theme=$Theme;popup_theme=$Theme};groups=@(@{id=$groupId;name='Preview group';entries=@($entries)})}
        [IO.File]::WriteAllText($configPath,($config | ConvertTo-Json -Depth 12),(New-Object Text.UTF8Encoding($false)))
        $original = (Get-FileHash $configPath).Hash
        $process = $null
        $form = [IntPtr]::Zero
        try {
            $process = Start-Process $Executable -ArgumentList @('--data-dir',('"' + $root + '"')) -PassThru
            $manager = [AppStackSmokeV2]::WaitWindow('AppStack.Manager','AppStackClassic',$process.Id)
            foreach ($action in @('cancel','save')) {
                [AppStackSmokeV2]::PostMessage($manager,0x111,[IntPtr]102,[IntPtr]::Zero) | Out-Null
                $form = [AppStackSmokeV2]::WaitWindow('AppStack.Form','Edit group',$process.Id)
                [AppStackLayoutV4]::SelectTab($form,'Interface')
                [AppStackSmokeV2]::WaitFor([Func[bool]]{ return [AppStackSmokeV2]::IsWindowVisible([AppStackLayoutV4]::GetDlgItem($form,128)) },$process.Id,10000)
                $preview = [AppStackLayoutV4]::GetDlgItem($form,50)
                if ($preview -eq [IntPtr]::Zero) { throw 'Live preview is missing' }
                $before = Join-Path $root ('preview-before-' + $action + '.png')
                $after = Join-Path $root ('preview-after-' + $action + '.png')
                [AppStackSmokeV2]::Screenshot($preview,$before)
                foreach ($setting in @(@(108,'2xl'),@(109,'lg'),@(110,'md'),@(111,'xl'),@(112,'xl'),@(113,'full'),@(114,'full'),@(115,'Card'),@(128,'2xl'))) {
                    Set-EditorPreset $form $setting[0] $setting[1] $process.Id
                }
                [AppStackSmokeV2]::Screenshot($preview,$after)
                [AppStackSmokeV2]::Screenshot($form,(Join-Path $root ('editor-' + $action + '.png')))
                if ((Get-FileHash $before).Hash -eq (Get-FileHash $after).Hash) { throw 'Changing presets did not change the preview pixels' }
                if ((Get-FileHash $configPath).Hash -ne $original) { throw 'Preview changed the configuration before Save' }
                [AppStackSmokeV2]::PostMessage($form,0x111,[IntPtr]$(if ($action -eq 'cancel') { 2 } else { 1 }),[IntPtr]::Zero) | Out-Null
                [AppStackSmokeV2]::WaitClosed($form,$process.Id)
                if ($action -eq 'cancel' -and (Get-FileHash $configPath).Hash -ne $original) { throw 'Cancel persisted preview changes' }
            }
            [AppStackSmokeV2]::WaitForContent($configPath,'9999')
            $saved = Get-Content $configPath -Raw | ConvertFrom-Json
            $look = $saved.groups[0].appearance
            if ($look.icon_size -ne 64 -or $look.gap_horizontal -ne 16 -or $look.gap_vertical -ne 12 -or $look.corner_radius -ne 9999 -or $look.tile_radius -ne 9999 -or $look.minimum_width -ne 672) { throw 'Preset values did not persist correctly' }
            $forward = Start-Process $Executable -ArgumentList @('--group',$groupId,'--position','800,800','--data-dir',('"' + $root + '"')) -PassThru
            if (-not $forward.WaitForExit(10000)) { throw 'Popup forwarding timed out' }
            $popup = [AppStackSmokeV2]::WaitWindow('AppStack.Popup','Preview group',$process.Id)
            if (-not [AppStackLayoutV4]::HasPillCorners($popup)) { throw 'rounded-full is not a real pill-shaped region' }
            [AppStackSmokeV2]::Screenshot($popup,(Join-Path $root 'popup-full.png'))
            Write-Output 'PASS: named size selects, live preview pixel changes, Cancel isolation, Save persistence and real rounded-full popup'
            Write-Output ('Screenshots: ' + $root)
        } finally {
            if ($form -ne [IntPtr]::Zero) { [AppStackSmokeV2]::PostMessage($form,0x111,[IntPtr]2,[IntPtr]::Zero) | Out-Null }
            if ($process -and -not $process.HasExited) {
                $quit = Start-Process $Executable -ArgumentList @('--quit','--data-dir',('"' + $root + '"')) -PassThru
                if (-not $quit.WaitForExit(5000) -or -not $process.WaitForExit(10000)) { Stop-Process -Id $process.Id -Force }
            }
        }
    }
    return
}

& {
    $Executable = (Resolve-Path $Executable).Path
    $root = Join-Path (Split-Path (Split-Path $Executable)) ('smoke-' + [guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $root | Out-Null
    $groupId = [guid]::NewGuid().ToString()
    $childId = [guid]::NewGuid().ToString()
    $marker = Join-Path $root 'launch-marker.txt'
    $entries = @(
        @{ id=[guid]::NewGuid().ToString(); name='Notepad'; target=@{kind='Application';value=(Join-Path $env:WINDIR 'System32\notepad.exe')} },
        @{ id=[guid]::NewGuid().ToString(); name='Calculator'; target=@{kind='Application';value=(Join-Path $env:WINDIR 'System32\calc.exe')} },
        @{ id=[guid]::NewGuid().ToString(); name='Tools'; target=@{kind='Group';value=$childId} }
    )
    $entries += @(0..999 | ForEach-Object { @{id=[guid]::NewGuid().ToString();name=('Application ' + $_);target=@{kind='Application';value=(Join-Path $env:WINDIR 'System32\notepad.exe')}} })
    $configuration = @{schema_version=2;settings=@{show_tray=$false};groups=@(
        @{id=$groupId;name='Work';appearance=@{columns=4;show_labels=$true};entries=$entries},
        @{id=$childId;name='Tools';entries=@(
            @{id=[guid]::NewGuid().ToString();name='Explorer';target=@{kind='Application';value=(Join-Path $env:WINDIR 'explorer.exe')}},
            @{id=[guid]::NewGuid().ToString();name='Launch marker';target=@{kind='Application';value=$env:ComSpec};arguments=('/d /c echo native-launch>"' + $marker + '"')}
        )}
    )}
    $configPath = Join-Path $root 'appstack.json'
    [IO.File]::WriteAllText($configPath, ($configuration | ConvertTo-Json -Depth 12), (New-Object Text.UTF8Encoding($false)))
    $process = $null
    $cursor = New-Object AppStackSmokeV2+Point
    [AppStackSmokeV2]::GetCursorPos([ref]$cursor) | Out-Null
    try {
        $timer = [Diagnostics.Stopwatch]::StartNew()
        $process = Start-Process $Executable -ArgumentList @('--data-dir', ('"' + $root + '"')) -PassThru -RedirectStandardError (Join-Path $root 'stderr.log') -RedirectStandardOutput (Join-Path $root 'stdout.log')
        $manager = [AppStackSmokeV2]::WaitWindow('AppStack.Manager',[IO.Path]::GetFileNameWithoutExtension($Executable),$process.Id)
        $cold = $timer.ElapsedMilliseconds
        [AppStackSmokeV2]::Screenshot($manager, (Join-Path $root 'manager.png'))
        $timer.Restart()
        $forward = Start-Process $Executable -ArgumentList @('--group',$groupId,'--position','800,800','--data-dir',('"' + $root + '"')) -PassThru
        if (-not $forward.WaitForExit(10000) -or $forward.ExitCode -ne 0) { throw 'Single-instance forwarding failed' }
        $popup = [AppStackSmokeV2]::WaitWindow('AppStack.Popup','Work',$process.Id)
        $warm = $timer.ElapsedMilliseconds
        $before = New-Object AppStackSmokeV2+Rect
        $after = New-Object AppStackSmokeV2+Rect
        [AppStackSmokeV2]::GetWindowRect($popup,[ref]$before) | Out-Null
        [AppStackSmokeV2]::PostMessage($popup,0x200,[IntPtr]::Zero,[IntPtr]3276900) | Out-Null
        [AppStackSmokeV2]::GetWindowRect($popup,[ref]$after) | Out-Null
        if ($before.Left -ne $after.Left -or $before.Top -ne $after.Top) { throw 'Popup moved with the cursor' }
        $buttons = [AppStackSmokeV2]::CountButtons($popup)
        if ($buttons -gt 2) { throw ('Popup recreated per-application windows: ' + $buttons) }
        [AppStackSmokeV2]::Screenshot($popup, (Join-Path $root 'popup.png'))
        $viewport = [AppStackSmokeV2]::FindWindowEx($popup,[IntPtr]::Zero,'AppStack.Viewport','Applications')
        if ($viewport -eq [IntPtr]::Zero) { throw 'Popup viewport was not found' }
        $list = [AppStackSmokeV2]::FindWindowEx($viewport,[IntPtr]::Zero,'SysListView32','Applications')
        if ($list -eq [IntPtr]::Zero) { throw 'Native popup list was not found' }
        [AppStackSmokeV2]::PostMessage($list,0x20A,[IntPtr](-7864320),[IntPtr]::Zero) | Out-Null
        [AppStackSmokeV2]::WaitFor([Func[bool]]{ return ([AppStackList]::Find($list,'Notepad') -lt 1) },$process.Id,10000)
        [AppStackSmokeV2]::PostMessage($viewport,0x115,[IntPtr]7,[IntPtr]::Zero) | Out-Null
        [AppStackSmokeV2]::WaitFor([Func[bool]]{ return ([AppStackList]::Find($list,'Application 999') -gt 0) },$process.Id,10000)
        [AppStackSmokeV2]::PostMessage($viewport,0x115,[IntPtr]6,[IntPtr]::Zero) | Out-Null
        [AppStackSmokeV2]::WaitFor([Func[bool]]{ return ([AppStackList]::Find($list,'Tools') -gt 0) },$process.Id,10000)
        [AppStackList]::Invoke($list,'Tools')
        $child = [AppStackSmokeV2]::WaitWindow('AppStack.Popup','Tools',$process.Id)
        if ($child -ne $popup) { throw 'Subfolder opened a new window instead of navigating inside the popup' }
        [AppStackSmokeV2]::Screenshot($child, (Join-Path $root 'subgroup.png'))
        [AppStackSmokeV2]::PostMessage($child,0x100,[IntPtr]27,[IntPtr]::Zero) | Out-Null
        if ([AppStackSmokeV2]::WaitWindow('AppStack.Popup','Work',$process.Id) -ne $popup) { throw 'Esc did not return to the parent folder' }
        $forward = Start-Process $Executable -ArgumentList @('--group',$groupId,'--position','800,800','--data-dir',('"' + $root + '"')) -PassThru
        $forward.WaitForExit(10000) | Out-Null
        [AppStackSmokeV2]::WaitClosed($popup,$process.Id)
        $reopenings = @()
        foreach ($iteration in 1..5) {
            $timer.Restart()
            $forward = Start-Process $Executable -ArgumentList @('--group',$groupId,'--position','800,800','--data-dir',('"' + $root + '"')) -PassThru
            $forward.WaitForExit(10000) | Out-Null
            $reopened = [AppStackSmokeV2]::WaitWindow('AppStack.Popup','Work',$process.Id)
            $reopenings += $timer.ElapsedMilliseconds
            if ($reopened -ne $popup) { throw 'A warm popup was recreated instead of reused' }
            $forward = Start-Process $Executable -ArgumentList @('--group',$groupId,'--position','800,800','--data-dir',('"' + $root + '"')) -PassThru
            $forward.WaitForExit(10000) | Out-Null
            [AppStackSmokeV2]::WaitClosed($popup,$process.Id)
        }
        $forward = Start-Process $Executable -ArgumentList @('--group',$childId,'--position','800,800','--data-dir',('"' + $root + '"')) -PassThru
        $forward.WaitForExit(10000) | Out-Null
        $launchPopup = [AppStackSmokeV2]::WaitWindow('AppStack.Popup','Tools',$process.Id)
        $launchViewport = [AppStackSmokeV2]::FindWindowEx($launchPopup,[IntPtr]::Zero,'AppStack.Viewport','Applications')
        $launchList = [AppStackSmokeV2]::FindWindowEx($launchViewport,[IntPtr]::Zero,'SysListView32','Applications')
        $accessible = [System.Windows.Automation.AutomationElement]::FromHandle($launchList).FindFirst(
            [System.Windows.Automation.TreeScope]::Descendants,
            (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty,'Launch marker')))
        if ($null -ne $accessible -and $accessible.Current.Name -ne 'Launch marker') { throw 'Native accessibility name is missing' }
        if ([AppStackList]::Find($launchList,'Launch marker') -lt 1) { throw 'Native accessible application name is missing' }
        $pattern = $null
        if ($null -ne $accessible -and $accessible.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)) {
            ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
        } else { [AppStackList]::Invoke($launchList,'Launch marker') }
        [AppStackSmokeV2]::WaitForContent($marker,'native-launch')
        [AppStackSmokeV2]::WaitClosed($launchPopup,$process.Id)
        [AppStackSmokeV2]::PostMessage($manager,0x111,[IntPtr]101,[IntPtr]::Zero) | Out-Null
        $form = [AppStackSmokeV2]::WaitWindow('AppStack.Form','New group',$process.Id)
        [AppStackSmokeV2]::Screenshot($form, (Join-Path $root 'editor.png'))
        $name = [AppStackSmokeV2]::FindWindowEx($form,[IntPtr]::Zero,'Edit','New group')
        if ($name -eq [IntPtr]::Zero) { throw 'Group name field was not found' }
        [AppStackInput]::SetText($name,'Smoke created')
        $formElement = [System.Windows.Automation.AutomationElement]::FromHandle($form)
        $interface = $formElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
            (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty,'Interface')))
        if ($null -eq $interface) { [AppStackLayoutV4]::SelectTab($form,'Interface') }
        else { ([System.Windows.Automation.SelectionItemPattern]$interface.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern)).Select() }
        [AppStackSmokeV2]::WaitFor([Func[bool]]{ return [AppStackSmokeV2]::IsWindowVisible([AppStackLayoutV4]::GetDlgItem($form,128)) },$process.Id,10000)
        foreach ($setting in @(@(108,'2xl'),@(109,'lg'),@(110,'md'),@(111,'xl'),@(112,'xl'),@(113,'3xl'),@(114,'xl'),@(128,'2xl'))) {
            Set-EditorPreset $form $setting[0] $setting[1] $process.Id
        }
        foreach ($label in @('Save','Cancel')) {
            $footer = [AppStackSmokeV2]::FindWindowEx($form,[IntPtr]::Zero,'Button',$label)
            if (-not [AppStackSmokeV2]::IsWindowVisible($footer)) { throw ('The ' + $label + ' button is hidden on the Interface tab') }
        }
        [AppStackSmokeV2]::Screenshot($form,(Join-Path $root 'editor-interface.png'))
        $saveButton = [AppStackSmokeV2]::FindWindowEx($form,[IntPtr]::Zero,'Button','Save')
        if ($saveButton -eq [IntPtr]::Zero) { throw 'Save button was not found' }
        [AppStackSmokeV2]::PostMessage($saveButton,0xF5,[IntPtr]::Zero,[IntPtr]::Zero) | Out-Null
        [AppStackSmokeV2]::WaitForContent($configPath,'Smoke created')
        $saved = Get-Content $configPath -Raw | ConvertFrom-Json
        if ($saved.groups.Count -ne 3) { throw 'Group creation or atomic persistence failed' }
        $custom = @($saved.groups | Where-Object name -eq 'Smoke created')[0].appearance
        if ($custom.icon_size -ne 64 -or $custom.gap_horizontal -ne 16 -or $custom.gap_vertical -ne 12 -or
            $custom.padding_horizontal -ne 24 -or $custom.padding_vertical -ne 24 -or $custom.corner_radius -ne 24 -or $custom.tile_radius -ne 12 -or $custom.minimum_width -ne 672) {
            throw 'Interface settings did not persist from the group editor'
        }
        if (-not (Test-Path (Join-Path $root 'appstack.json.bak'))) { throw 'Configuration backup was not created' }
        $process.Refresh()
        $workingSet = [math]::Round($process.WorkingSet64 / 1MB,1)
        $quit = Start-Process $Executable -ArgumentList @('--quit','--data-dir',('"' + $root + '"')) -PassThru
        $quit.WaitForExit(5000) | Out-Null
        if (-not $process.WaitForExit(10000)) { throw 'AppStack did not shut down cleanly' }
        $timer.Restart()
        $process = Start-Process $Executable -ArgumentList @('--group',$groupId,'--position','800,800','--data-dir',('"' + $root + '"')) -PassThru
        $coldPopupWindow = [AppStackSmokeV2]::WaitWindow('AppStack.Popup','Work',$process.Id)
        $coldPopup = $timer.ElapsedMilliseconds
        $p95 = ($reopenings | Sort-Object)[[math]::Ceiling($reopenings.Count * 0.95) - 1]
        $metrics = @{cold_manager_ms=$cold;cold_popup_ms=$coldPopup;first_popup_ms=$warm;warm_popup_p95_ms=$p95;warm_samples_ms=$reopenings;native_buttons=$buttons;working_set_mib=$workingSet}
        [IO.File]::WriteAllText((Join-Path $root 'metrics.json'),($metrics | ConvertTo-Json))
        foreach ($layout in @('icons','right-labels','card','custom-rounded','custom-square','custom-left')) {
            $quit = Start-Process $Executable -ArgumentList @('--quit','--data-dir',('"' + $root + '"')) -PassThru
            $quit.WaitForExit(5000) | Out-Null
            if (-not $process.WaitForExit(10000)) { throw 'AppStack did not shut down before the layout test' }
            $saved.groups[0].entries = @($saved.groups[0].entries | Select-Object -First 8)
            $saved.groups[0].appearance.show_labels = ($layout -ne 'icons')
            $saved.groups[0].appearance.label_position = $(if ($layout -eq 'right-labels') { 'Right' } else { 'Bottom' })
            $saved.groups[0].appearance.card_layout = ($layout -eq 'card')
            $saved.groups[0].appearance.header_bottom = ($layout -eq 'card')
            $isCustom = $layout.StartsWith('custom-')
            $look = $saved.groups[0].appearance
            $look.minimum_width = $(if ($isCustom) { 720 } else { 0 })
            $look.icon_size = $(if ($isCustom) { 64 } else { 32 })
            $look.padding_horizontal = $(if ($isCustom) { 24 } else { 10 })
            $look.padding_vertical = $(if ($isCustom) { 20 } else { 10 })
            $look.gap_horizontal = $(if ($isCustom) { 18 } else { 0 })
            $look.gap_vertical = $(if ($isCustom) { 14 } else { 0 })
            $look.corner_radius = $(if ($layout -eq 'custom-square') { 0 } elseif ($isCustom) { 24 } else { 8 })
            $look.tile_radius = $(if ($layout -eq 'custom-square') { 0 } elseif ($isCustom) { 12 } else { 4 })
            $look.show_edit_button = ($layout -ne 'custom-square')
            $look.show_launch_all_button = (-not $isCustom -or $layout -eq 'custom-square')
            $look.button_alignment = $(if ($layout -eq 'custom-rounded') { 'Center' } elseif ($layout -eq 'custom-left') { 'Left' } else { 'Right' })
            if ($isCustom) { $look.card_layout = $true }
            [IO.File]::WriteAllText($configPath,($saved | ConvertTo-Json -Depth 14),(New-Object Text.UTF8Encoding($false)))
            $process = Start-Process $Executable -ArgumentList @('--group',$groupId,'--position','800,800','--data-dir',('"' + $root + '"')) -PassThru
            $layoutWindow = [AppStackSmokeV2]::WaitWindow('AppStack.Popup','Work',$process.Id)
            $layoutViewport = [AppStackSmokeV2]::FindWindowEx($layoutWindow,[IntPtr]::Zero,'AppStack.Viewport','Applications')
            $layoutList = [AppStackSmokeV2]::FindWindowEx($layoutViewport,[IntPtr]::Zero,'SysListView32','Applications')
            if ($layoutList -eq [IntPtr]::Zero -or [AppStackList]::Find($layoutList,'Notepad') -lt 1) { throw ('Application accessibility was lost in layout ' + $layout) }
            if ($isCustom) {
                if ([AppStackSmokeV2]::CountButtons($layoutWindow) -ne 1) { throw 'A single-action popup has the wrong number of buttons' }
                $label = if ($layout -eq 'custom-square') { 'Launch all' } else { 'Edit' }
                $button = [AppStackSmokeV2]::FindWindowEx($layoutWindow,[IntPtr]::Zero,'Button',$label)
                $frameBounds = New-Object AppStackSmokeV2+Rect
                $buttonBounds = New-Object AppStackSmokeV2+Rect
                [AppStackSmokeV2]::GetWindowRect($layoutWindow,[ref]$frameBounds) | Out-Null
                [AppStackSmokeV2]::GetWindowRect($button,[ref]$buttonBounds) | Out-Null
                $padding = [int][math]::Round(24 * [AppStackLayoutV4]::GetDpiForWindow($layoutWindow) / 96)
                if ($layout -eq 'custom-rounded' -and [math]::Abs($buttonBounds.Left+$buttonBounds.Right-$frameBounds.Left-$frameBounds.Right) -gt 2) { throw 'The single button is not centered' }
                if ($layout -eq 'custom-left' -and [math]::Abs($buttonBounds.Left-$frameBounds.Left-$padding) -gt 2) { throw 'The single button is not left-aligned' }
                if ($layout -eq 'custom-square' -and [math]::Abs($frameBounds.Right-$buttonBounds.Right-$padding) -gt 2) { throw 'The single button is not right-aligned' }
                if ([AppStackLayoutV4]::HasClippedCorners($layoutWindow) -ne ($layout -ne 'custom-square')) { throw 'Custom popup corner clipping is incorrect' }
            }
            [AppStackSmokeV2]::Screenshot($layoutWindow,(Join-Path $root ($layout + '.png')))
        }
        Write-Output ('PASS: native UI, single instance, fixed popup, virtualization, scroll, subgroups, popup reuse, UI Automation, real launch, group creation, atomic persistence')
        Write-Output 'PASS: icons-only, right labels and card layouts retain accessible application names'
        Write-Output 'PASS: General/Interface tabs, custom settings persistence, icon sizes, gaps, corner regions and single-button alignment'
        Write-Output ('Diagnostic only: cold manager (1003 items)=' + $cold + 'ms; cold popup=' + $coldPopup + 'ms; first popup=' + $warm + 'ms; warm p95 (5 samples)=' + $p95 + 'ms; native buttons=' + $buttons + '; working set=' + $metrics.working_set_mib + 'MiB')
        Write-Output ('Screenshots: ' + $root)
    } catch {
        Write-Output ('Failed run artifacts: ' + $root)
        if (Test-Path (Join-Path $root 'stderr.log')) { Get-Content (Join-Path $root 'stderr.log') | Write-Output }
        if (Test-Path (Join-Path $root 'errors.log')) { Get-Content (Join-Path $root 'errors.log') | Write-Output }
        throw
    } finally {
        if ($process -and -not $process.HasExited) {
            $quit = Start-Process $Executable -ArgumentList @('--quit','--data-dir',('"' + $root + '"')) -PassThru
            $quit.WaitForExit(5000) | Out-Null
            if (-not $process.WaitForExit(10000)) { Stop-Process -Id $process.Id -Force }
        }
    }
}