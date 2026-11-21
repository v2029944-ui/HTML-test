# Runs on an isolated hosted Windows runner, using only its temporary test install.
param([Parameter(Mandatory=$true)][string]$Candidate,[Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or !$env:RUNNER_TEMP) {
    throw 'An isolated Windows Actions runner is required'
}
Add-Type -AssemblyName @('System.Drawing','UIAutomationClient','UIAutomationTypes','WindowsBase','System.Windows.Forms')
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Runtime.InteropServices;
using System.Text;
public static class AnalizIcon {
 [DllImport("shell32.dll", EntryPoint="ExtractIconExW", CharSet=CharSet.Unicode, ExactSpelling=true, SetLastError=true)]
 static extern uint ExtractIconEx(string file,int idx,out IntPtr big,IntPtr small,uint count);
 [DllImport("user32")] static extern bool DestroyIcon(IntPtr icon);
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)]
 struct ShellFileInfo {
   public IntPtr Icon;public int Index;public uint Attributes;
   [MarshalAs(UnmanagedType.ByValTStr,SizeConst=260)] public string DisplayName;
   [MarshalAs(UnmanagedType.ByValTStr,SizeConst=80)] public string TypeName;
 }
 [DllImport("shell32",CharSet=CharSet.Unicode)] static extern IntPtr SHGetFileInfo(string path,uint attrs,out ShellFileInfo info,uint size,uint flags);
 public static Bitmap FromShortcut(string path){
   ShellFileInfo info;
   if(SHGetFileInfo(path,0,out info,(uint)Marshal.SizeOf(typeof(ShellFileInfo)),0x100)==IntPtr.Zero||info.Icon==IntPtr.Zero)
     throw new Exception("Shell did not resolve the pinned shortcut icon");
   try{return Copy(info.Icon);}finally{DestroyIcon(info.Icon);}
 }
 [DllImport("user32")] static extern IntPtr SendMessageTimeout(IntPtr h,uint m,IntPtr w,IntPtr l,uint flags,uint ms,out IntPtr result);
 [DllImport("user32",CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls,string title);
 [DllImport("user32")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32")] static extern bool SetCursorPos(int x,int y);
 [DllImport("user32")] static extern void mouse_event(uint f,int x,int y,uint data,UIntPtr extra);
 public static void ContextMenu(int x,int y){SetCursorPos(x,y);mouse_event(8,0,0,0,UIntPtr.Zero);mouse_event(16,0,0,0,UIntPtr.Zero);}
 public static void PhysicalPixels(){SetThreadDpiAwarenessContext(new IntPtr(-4));}
 static Bitmap Copy(IntPtr h){using(var icon=Icon.FromHandle(h)){return icon.ToBitmap();}}
 public static Bitmap FromFile(string path){
   // ExtractIconEx counts handles. Request only the large icon at the default PE index.
   IntPtr icon;
   uint count=ExtractIconEx(path,0,out icon,IntPtr.Zero,1);
   int error=Marshal.GetLastWin32Error();
   try{
     if(count==UInt32.MaxValue)throw new InvalidOperationException("ExtractIconExW failed for "+path+" (Win32 "+error+")");
     if(count==0||icon==IntPtr.Zero)throw new Exception("No default PE icon in "+path+" (count="+count+", handle="+icon+")");
     return Copy(icon);
   }finally{if(icon!=IntPtr.Zero)DestroyIcon(icon);}
 }
 public static Bitmap FromWindow(IntPtr hwnd,int kind){
   for(int attempt=0;attempt<20;attempt++){
     IntPtr icon;
     if(SendMessageTimeout(hwnd,127,new IntPtr(kind),IntPtr.Zero,2,500,out icon)!=IntPtr.Zero && icon!=IntPtr.Zero)return Copy(icon);
     System.Threading.Thread.Sleep(250);
   }
   throw new Exception("WM_GETICON returned no "+(kind==1?"large taskbar":"small window")+" icon");
 }
 static bool Pixel(Bitmap b,int x,int y,int size,double u,double v,bool white){
   Color p=b.GetPixel(x+(int)(size*u),y+(int)(size*v));
   return white ? p.R>214&&p.G>214&&p.B>214 : p.B>p.R+65&&p.B>p.G+25;
 }
 public static bool Mark(Bitmap b,int x,int y,int s){
   return Pixel(b,x,y,s,.5,.18,false)&&Pixel(b,x,y,s,.255,.7,true)&&Pixel(b,x,y,s,.5,.57,true)&&
          Pixel(b,x,y,s,.74,.38,true)&&Pixel(b,x,y,s,.37,.70,false)&&Pixel(b,x,y,s,.255,.43,false);
 }
 public static bool TaskbarMark(Bitmap b){
   for(int s=15;s<=Math.Min(80,Math.Min(b.Width,b.Height));s++)
     for(int y=0;y<=b.Height-s;y++)for(int x=0;x<=b.Width-s;x++)
       if(Mark(b,x,y,s))return true;
   return false;
 }
}
'@
[AnalizIcon]::PhysicalPixels()
$Candidate=(Resolve-Path $Candidate).Path
$pinDir=Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar'
$out=(New-Item -ItemType Directory -Path $OutputDirectory -Force).FullName
$root=(New-Item -ItemType Directory -Path (Join-Path $env:RUNNER_TEMP ('analiz-icons-'+[guid]::NewGuid().ToString('N')))).FullName
$install=Join-Path $root 'app'
$work=Join-Path $root 'profile'
New-Item -ItemType Directory $work | Out-Null
# Both versions are production binaries. Redirect their user data into this desktop's test profile.
$env:APPDATA=Join-Path $work 'Roaming'
$env:LOCALAPPDATA=Join-Path $work 'Local'
New-Item -ItemType Directory $env:APPDATA,$env:LOCALAPPDATA | Out-Null
$shell=New-Object -ComObject WScript.Shell
$result=[ordered]@{sha=$env:REFERENCE_HEAD_SHA;clean=@{};upgrade=@{}}
function Save-Result { $result | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 (Join-Path $out 'reference-windows-icons.json') }
function Run-Setup($setup,[switch]$Update) {
    $mode=if($Update){'/P /UPDATE'}else{'/S'}
    $process=Start-Process $setup -ArgumentList ($mode+' /D='+$install) -PassThru
    if(!$process.WaitForExit(180000)){throw "Installation timed out"}
    if($process.ExitCode -ne 0){throw "Installer returned $($process.ExitCode)"}
    $exe=@(Get-ChildItem -LiteralPath $install -Filter '*.exe' -File | Where-Object {$_.Name -notmatch '^uninstall'})
    if($exe.Count -ne 1){throw 'Expected exactly one installed application'}
    return $exe[0].FullName
}
function Stop-Owned($path) {
    Get-Process -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq $path} | ForEach-Object {
        if($_.MainWindowHandle -ne 0){[void]$_.CloseMainWindow()}
        if(!$_.WaitForExit(8000)){Stop-Process -Id $_.Id -Force;[void]$_.WaitForExit(5000)}
    }
}
function Uninstall-Owned {
    $u=Join-Path $install 'uninstall.exe'
    if(Test-Path -LiteralPath $u){
        $p=Start-Process $u -ArgumentList ('/S _?='+$install) -Wait -PassThru
        if($p.ExitCode -ne 0){throw "Test uninstall returned $($p.ExitCode)"}
    }
    if(Test-Path -LiteralPath $install){Remove-Item -LiteralPath $install -Recurse -Force}
}
function Main-Window($path){
    $process=Get-Process -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq $path} | Select-Object -First 1
    if(!$process){$process=Start-Process $path -PassThru}
    for($i=0;$i -lt 100;$i++){Start-Sleep -Milliseconds 200;$process.Refresh();if($process.MainWindowHandle -ne 0){return $process.MainWindowHandle}}
    throw 'Installed application has no main window'
}
function Check-Bitmap($bitmap,$id){
    try{
        $bitmap.Save((Join-Path $out ("reference-icon-"+$id+".png")))
        if(![AnalizIcon]::Mark($bitmap,0,0,$bitmap.Width)){throw "Expected three white bars on blue: $id"}
    }finally{$bitmap.Dispose()}
}
function Taskbar-Button{
    $hwnd=[AnalizIcon]::FindWindow('Shell_TrayWnd',$null)
    if($hwnd -eq [IntPtr]::Zero){return}
    $taskbar=[System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
    $buttons=$taskbar.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
    foreach($b in $buttons){
        if($b.Current.Name -match 'Анализ.*работ|analiz[ .-]*rabot' -and !$b.Current.IsOffscreen){
            $r=$b.Current.BoundingRectangle
            if($r.Width -ge 16 -and $r.Width -le 600 -and $r.Height -ge 16 -and $r.Height -le 120){return $b}
        }
    }
}
function Capture-Taskbar($name,$record){
    for($i=0;$i -lt 20;$i++){
        Start-Sleep -Milliseconds 300
        $b=Taskbar-Button
        if(!$b){continue}
        $r=$b.Current.BoundingRectangle
        $image=New-Object Drawing.Bitmap ([int]$r.Width),([int]$r.Height)
        try{
            $g=[Drawing.Graphics]::FromImage($image)
            try{$g.CopyFromScreen([int]$r.X,[int]$r.Y,0,0,$image.Size)}finally{$g.Dispose()}
            $image.Save((Join-Path $out ("reference-taskbar-"+$name+".png")))
            $record.button=$b.Current.Name
            if([AnalizIcon]::TaskbarMark($image)){
                $record.taskbar='verified from visible Explorer pixels';return
            }
        }finally{$image.Dispose()}
    }
    if($record.button){throw "Explorer displays a different icon: $name"}
    $record.taskbar='unverified: no visible app button in Explorer accessibility tree'
}
function Find-Pin($path){
    if(Test-Path -LiteralPath $pinDir){
        foreach($link in Get-ChildItem -LiteralPath $pinDir -Filter '*.lnk' -File){
            if($shell.CreateShortcut($link.FullName).TargetPath -eq $path){return $link.FullName}
        }
    }
}
function Taskbar-Verb($names){
    $b=Taskbar-Button
    if(!$b){return $false}
    $r=$b.Current.BoundingRectangle
    [AnalizIcon]::ContextMenu([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2))
    try{
        for($i=0;$i -lt 5;$i++){Start-Sleep -Milliseconds 250
            foreach($name in $names){
                $condition=New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::NameProperty),$name
                $item=[System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
                if($item -and !$item.Current.IsOffscreen){
                    $pattern=$null
                    if($item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){
                        $pattern.Invoke();return $true
                    }
                }
            }
        }
        return $false
    }finally{[System.Windows.Forms.SendKeys]::SendWait('{ESC}')}
}
function Try-Pin($path){
    $folder=(New-Object -ComObject Shell.Application).NameSpace((Split-Path $path))
    $item=$folder.ParseName((Split-Path $path -Leaf))
    foreach($verb in $item.Verbs()){
        if($verb.Name.Replace('&','') -match '^(Pin to taskbar|Закрепить на панели задач)$'){
            $verb.DoIt();Start-Sleep -Seconds 1;break
        }
    }
    if(!(Find-Pin $path)){
        $null=Taskbar-Verb @('Pin to taskbar','Закрепить на панели задач')
        Start-Sleep -Milliseconds 350
    }
    return Find-Pin $path
}
$exe=$null
try{
    try{
        Check-Bitmap ([AnalizIcon]::FromFile($Candidate)) 'installer';$result.clean.installer='verified'
        $exe=Run-Setup $Candidate
        Check-Bitmap ([AnalizIcon]::FromFile($exe)) 'clean-exe';$result.clean.exe='verified'
        $hwnd=Main-Window $exe
        Check-Bitmap ([AnalizIcon]::FromWindow($hwnd,0)) 'clean-window'
        Check-Bitmap ([AnalizIcon]::FromWindow($hwnd,1)) 'clean-taskbar-hwnd'
        $result.clean.hwnd='verified small and large'
        Capture-Taskbar 'clean' $result.clean
    }catch{$result.clean.error=$_.Exception.Message}
    Save-Result
    if($exe){Stop-Owned $exe}
    Uninstall-Owned
    $old=Join-Path $root 'stable-1.10.0.exe'
    Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/v2029944-ui/HTML-test/releases/download/tauri-v1.10.0/Analiz-rabot-Tauri-Windows-x64-setup.exe' -OutFile $old
    $oldHash=(Get-FileHash $old -Algorithm SHA256).Hash
    if($oldHash -ne 'dcf1aeed22492007fe658795401d10a3730060699a88b2a903380bf5a1d67fd6'){throw 'Stable installer digest differs'}
    $oldExe=Run-Setup $old
    $oldExeHash=(Get-FileHash $oldExe -Algorithm SHA256).Hash
    $null=Main-Window $oldExe
    $pin=Try-Pin $oldExe
    $result.upgrade.pinBefore=if($pin){'verified real shell shortcut'}else{'unverified: Explorer did not expose or create the pin'}
    Stop-Owned $oldExe
    $exe=Run-Setup $Candidate -Update
    if($exe -ne $oldExe -or (Get-FileHash $exe -Algorithm SHA256).Hash -eq $oldExeHash){throw 'Existing executable was not replaced in place'}
    Check-Bitmap ([AnalizIcon]::FromFile($exe)) 'upgrade-exe';$result.upgrade.exe='verified in place over stable 1.10.0'
    if($pin){
        if((Find-Pin $exe) -ne $pin){throw 'The existing pinned shortcut was lost during upgrade'}
        $result.upgrade.pinned=@{target=$shell.CreateShortcut($pin).TargetPath;location=$shell.CreateShortcut($pin).IconLocation}
        Check-Bitmap ([AnalizIcon]::FromShortcut($pin)) 'existing-pin'
        $result.upgrade.pinned.resolvedIcon='verified with SHGetFileInfo'
        Capture-Taskbar 'existing-pin' $result.upgrade.pinned
    }
    $hwnd=Main-Window $exe
    Check-Bitmap ([AnalizIcon]::FromWindow($hwnd,0)) 'upgrade-window'
    Check-Bitmap ([AnalizIcon]::FromWindow($hwnd,1)) 'upgrade-taskbar-hwnd'
    $result.upgrade.hwnd='verified small and large'
    Capture-Taskbar 'upgrade' $result.upgrade
    if($result.clean.error){throw $result.clean.error}
}catch{$result.error=$_.Exception.Message;throw}
finally{
    Save-Result
    if($pin){$null=Taskbar-Verb @('Unpin from taskbar','Открепить от панели задач')}
    if($exe){Stop-Owned $exe}
    Uninstall-Owned
    Remove-Item -LiteralPath $root -Recurse -Force
}
