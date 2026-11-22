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
 // Read the persisted Unicode shell link itself. WScript.Shell can return an empty
 // newly-created object for an existing shortcut with a non-ANSI filename.
 [ComImport,Guid("00021401-0000-0000-C000-000000000046")] class ShellLink {}
 [ComImport,Guid("000214F9-0000-0000-C000-000000000046"),InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
 interface IShellLinkW {
   [PreserveSig] int GetPath([Out,MarshalAs(UnmanagedType.LPWStr)] StringBuilder path,int chars,IntPtr fileData,uint flags);
   void GetIDList(out IntPtr list); void SetIDList(IntPtr list);
   void GetDescription(IntPtr text,int chars); void SetDescription(IntPtr text);
   void GetWorkingDirectory(IntPtr text,int chars); void SetWorkingDirectory(IntPtr text);
   void GetArguments(IntPtr text,int chars); void SetArguments(IntPtr text);
   void GetHotkey(out short key); void SetHotkey(short key);
   void GetShowCmd(out int cmd); void SetShowCmd(int cmd);
   [PreserveSig] int GetIconLocation([Out,MarshalAs(UnmanagedType.LPWStr)] StringBuilder path,int chars,out int index);
 }
 public sealed class LinkInfo {
   public string Target,RawTarget,Icon;
   public int IconIndex;
 }
 public static LinkInfo ReadLink(string file){
   object obj=new ShellLink();
   try{
     ((System.Runtime.InteropServices.ComTypes.IPersistFile)obj).Load(file,0);
     var link=(IShellLinkW)obj;
     var path=new StringBuilder(32768);
     Marshal.ThrowExceptionForHR(link.GetPath(path,path.Capacity,IntPtr.Zero,4));
     var result=new LinkInfo();result.RawTarget=path.ToString();
     path.Length=0;Marshal.ThrowExceptionForHR(link.GetPath(path,path.Capacity,IntPtr.Zero,0));
     result.Target=path.Length==0?Environment.ExpandEnvironmentVariables(result.RawTarget):path.ToString();
     path.Length=0;int index;
     Marshal.ThrowExceptionForHR(link.GetIconLocation(path,path.Capacity,out index));
     result.Icon=path.ToString();result.IconIndex=index;
     return result;
   }finally{Marshal.FinalReleaseComObject(obj);}
 }
 [StructLayout(LayoutKind.Sequential)] struct PropertyKey {public Guid Format; public uint Id;}
 [StructLayout(LayoutKind.Explicit,Size=24)] struct StringVariant {
   [FieldOffset(0)] public ushort Type;
   [FieldOffset(8)] public IntPtr Value;
 }
 [ComImport,Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"),InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
 interface IPropertyStore {
   void GetCount(out uint count); void GetAt(uint index,out PropertyKey key);
   void GetValue(ref PropertyKey key,out StringVariant value);
 }
 [DllImport("shell32.dll")] static extern int SHGetPropertyStoreForWindow(IntPtr hwnd,ref Guid iid,out IPropertyStore store);
 [DllImport("ole32.dll")] static extern int PropVariantClear(ref StringVariant value);
 [DllImport("shlwapi.dll",CharSet=CharSet.Unicode,ExactSpelling=true)]
 static extern int SHLoadIndirectString(string source,StringBuilder output,uint length,IntPtr reserved);
 public static string[] GroupMetadata(IntPtr hwnd){
   Guid iid=typeof(IPropertyStore).GUID;
   IPropertyStore store;Marshal.ThrowExceptionForHR(SHGetPropertyStoreForWindow(hwnd,ref iid,out store));
   try{
     var result=new string[5];
     for(uint i=0;i<4;i++){
       // PKEY_AppUserModel_RelaunchCommand, RelaunchIconResource, RelaunchDisplayNameResource, ID
       PropertyKey key=new PropertyKey { Format=new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), Id=2+i };
       StringVariant value;store.GetValue(ref key,out value);
       try{
         if(value.Type!=31)throw new Exception("Expected VT_LPWSTR for window property "+key.Id+", got "+value.Type);
         result[i]=Marshal.PtrToStringUni(value.Value);
       }finally{PropVariantClear(ref value);}
     }
     var name=new StringBuilder(512);
     Marshal.ThrowExceptionForHR(SHLoadIndirectString(result[2],name,(uint)name.Capacity,IntPtr.Zero));
     result[4]=name.ToString();return result;
   }finally{Marshal.FinalReleaseComObject(store);}
 }
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h,StringBuilder s,int n);
 public static string WindowClass(IntPtr h){var text=new StringBuilder(256);GetClassName(h,text,text.Capacity);return text.ToString();}
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
 [DllImport("comctl32.dll")] static extern IntPtr ImageList_GetIcon(IntPtr list,int index,uint flags);
 public static Bitmap FromShortcutBase(string path){
   ShellFileInfo info;
   // Ask the same shell image list for its base image without drawing the shortcut arrow.
   IntPtr list=SHGetFileInfo(path,0,out info,(uint)Marshal.SizeOf(typeof(ShellFileInfo)),0x4000|0x100|0x40);
   if(info.Icon!=IntPtr.Zero)DestroyIcon(info.Icon);
   if(list==IntPtr.Zero)throw new Exception("No system image list for "+path);
   IntPtr icon=ImageList_GetIcon(list,info.Index & 0x00ffffff,0);
   if(icon==IntPtr.Zero)throw new Exception("No shell image at index "+info.Index);
   try{return Copy(icon);}finally{DestroyIcon(icon);}
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
 [DllImport("user32.dll",SetLastError=true)]
 static extern IntPtr CreateIconFromResourceEx(byte[] data,uint length,bool icon,uint version,int width,int height,uint flags);
 public static Bitmap FromIcoFrame(string path,int size){
   using(var reader=new System.IO.BinaryReader(System.IO.File.OpenRead(path))){
     if(reader.ReadUInt16()!=0||reader.ReadUInt16()!=1)throw new Exception("Invalid ICO header: "+path);
     int count=reader.ReadUInt16();
     for(int i=0;i<count;i++){
       int w=reader.ReadByte(),h=reader.ReadByte();
       reader.BaseStream.Seek(6,System.IO.SeekOrigin.Current);
       uint length=reader.ReadUInt32(),offset=reader.ReadUInt32();
       if((w==0?256:w)!=size||(h==0?256:h)!=size)continue;
       if((long)offset+length>reader.BaseStream.Length)throw new Exception("Truncated ICO frame: "+size);
       reader.BaseStream.Position=offset;
       var data=reader.ReadBytes(checked((int)length));
       Bitmap image;
       if(data.Length>=8&&BitConverter.ToUInt32(data,0)==0x474e5089u&&BitConverter.ToUInt32(data,4)==0x0a1a0a0du){
         // .NET Framework Icon(file,size).ToBitmap() can treat embedded PNG as DIB.
         using(var stream=new System.IO.MemoryStream(data))
         using(var png=new Bitmap(stream)){image=new Bitmap(png);}
       }else{
         // Give Windows just this DIB, with its own size and no rescaling.
         IntPtr icon=CreateIconFromResourceEx(data,length,true,0x00030000,0,0,0);
         if(icon==IntPtr.Zero)throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
         try{image=Copy(icon);}finally{DestroyIcon(icon);}
       }
       if(image.Width!=size||image.Height!=size){image.Dispose();throw new Exception("Wrong ICO frame dimensions: "+size);}
       return image;
     }
     throw new Exception("Missing ICO frame: "+size);
   }
 }
 public static Bitmap FromWindow(IntPtr hwnd,int kind){
   for(int attempt=0;attempt<20;attempt++){
     IntPtr icon;
     if(SendMessageTimeout(hwnd,127,new IntPtr(kind),IntPtr.Zero,2,500,out icon)!=IntPtr.Zero && icon!=IntPtr.Zero)return Copy(icon);
     System.Threading.Thread.Sleep(250);
   }
   throw new Exception("WM_GETICON returned no "+(kind==1?"large taskbar":"small window")+" icon");
 }
 static bool Pixel(Bitmap b,int x,int y,int size,double u,double v,bool dark){
   Color p=b.GetPixel(x+(int)(size*u),y+(int)(size*v));
   return p.A>128 && (dark ? p.R<95&&p.G<95&&p.B<100 : p.R>110&&p.G>95&&p.R-p.B>75&&p.G-p.B>40);
 }
 public static bool Mark(Bitmap b,int x,int y,int s){
   return Pixel(b,x,y,s,.50,.18,false)&&Pixel(b,x,y,s,.255,.70,true)&&Pixel(b,x,y,s,.50,.57,true)&&
          Pixel(b,x,y,s,.74,.55,true)&&Pixel(b,x,y,s,.39,.70,false)&&Pixel(b,x,y,s,.255,.43,false);
 }
 public static bool NativeDetail(Bitmap b){
   // The 16px frame omits the check. Larger native frames cut a yellow check into the last bar.
   int s=b.Width;
   return (s!=16&&s<32)||Pixel(b,0,0,s,.72,.35,s==16);
 }
 public static bool ShortcutMark(Bitmap b){
   // Sample the tops of all three bars, above the lower-left shortcut overlay.
   int s=b.Width;
   return Pixel(b,0,0,s,.50,.18,false)&&Pixel(b,0,0,s,.255,.58,true)&&Pixel(b,0,0,s,.50,.57,true)&&
          Pixel(b,0,0,s,.74,.55,true)&&Pixel(b,0,0,s,.39,.58,false)&&Pixel(b,0,0,s,.255,.43,false);
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
# Start the production binary in the disposable runner's actual Explorer profile.
# Never touch an existing database or application profile.
foreach($place in @(
    (Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'Анализ работ'),
    (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'local.analiz.rabot.tauri'),
    (Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'local.analiz.rabot.tauri')
)){
    if(Test-Path -LiteralPath $place){throw "An existing application profile prevents an isolated shell test: $place"}
}
$programs=[Environment]::GetFolderPath('Programs')
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
function Main-Window($path,[switch]$StartMenu){
    $process=Get-Process -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq $path} | Select-Object -First 1
    if($StartMenu){
        $lnk=Join-Path $programs 'Анализ работ.lnk'
        if(([AnalizIcon]::ReadLink($lnk)).Target -ne $path){throw 'Cannot launch an unrelated start menu shortcut'}
        if($process){Stop-Owned $path}
        Start-Process -FilePath $lnk
        $process=$null
        for($i=0;$i -lt 80 -and !$process;$i++){
            Start-Sleep -Milliseconds 250
            $process=Get-Process -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq $path} | Select-Object -First 1
        }
        if(!$process){throw 'Start menu did not launch the installed executable'}
    }elseif(!$process){$process=Start-Process $path -PassThru}
    for($i=0;$i -lt 240;$i++){
        Start-Sleep -Milliseconds 250
        $process.Refresh()
        if($process.HasExited){throw "Installed app exited before its main window: $($process.ExitCode)"}
        $hwnd=$process.MainWindowHandle
        if($hwnd -eq [IntPtr]::Zero -or [AnalizIcon]::WindowClass($hwnd) -ne 'Tauri Window'){continue}
        # A top-level HWND exists before cold WebView2 has initialized the page.
        $window=[System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
        $document=New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::ControlTypeProperty),([System.Windows.Automation.ControlType]::Document)
        if($window.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$document)){return $hwnd}
    }
    throw 'Installed application did not expose a WebView2 document in its Tauri window'
}
function Check-Bitmap($bitmap,$id){
    try{
        $bitmap.Save((Join-Path $out ("reference-icon-"+$id+".png")))
        if(![AnalizIcon]::Mark($bitmap,0,0,$bitmap.Width) -or ![AnalizIcon]::NativeDetail($bitmap)){
            throw "Expected yellow report icon with the correct artwork for $($bitmap.Width)px: $id"
        }
    }finally{$bitmap.Dispose()}
}
function Check-ShortcutIcon($shortcut,$id){
    $overlay=[AnalizIcon]::FromShortcut($shortcut)
    try{
        $overlay.Save((Join-Path $out ("reference-icon-"+$id+".png")))
        if(![AnalizIcon]::ShortcutMark($overlay)){throw "Shell shortcut displays different artwork: $id"}
    }finally{$overlay.Dispose()}
    Check-Bitmap ([AnalizIcon]::FromShortcutBase($shortcut)) ($id+'-base')
}
function Check-StartMenu($name,$exe,$record){
    $shortcut=Join-Path $programs 'Анализ работ.lnk'
    $info=@{path=$shortcut}
    $record.startMenu=$info
    if(!(Test-Path -LiteralPath $shortcut)){throw "Start menu shortcut missing: $shortcut"}
    $link=[AnalizIcon]::ReadLink($shortcut)
    $info.target=$link.Target
    $info.rawTarget=$link.RawTarget
    $info.icon=$link.Icon
    $info.iconIndex=$link.IconIndex
    $folder=(New-Object -ComObject Shell.Application).NameSpace($programs)
    $info.appId=$folder.ParseName((Split-Path $shortcut -Leaf)).ExtendedProperty('System.AppUserModel.ID')
    $icon=Join-Path (Split-Path $exe) 'analysis-reports.ico'
    if($info.target -ne $exe -or $info.appId -ne 'local.analiz.rabot.tauri'){throw 'Start menu link target or AppUserModelID differs'}
    if($info.icon -ne $icon -or $info.iconIndex -ne 0){throw "Start menu calls a different icon: $($info.icon),$($info.iconIndex)"}
    foreach($size in @(16,32,48,64,128,256)){
        Check-Bitmap ([AnalizIcon]::FromIcoFrame($icon,$size)) ($name+"-installed-"+$size)
    }
    Check-Bitmap ([AnalizIcon]::FromFile($icon)) ($name+'-installed-ico')
    Check-ShortcutIcon $shortcut ($name+'-start-menu')
}
function Check-WindowAndShell($name,$exe,$record){
    # Exercise each independent surface, even when a preceding assertion fails.
    try{
        $fromMenu=($name -eq 'upgrade')
        $hwnd=Main-Window $exe -StartMenu:$fromMenu
        $record.launch=if($fromMenu){'installed Start menu shortcut'}else{'installed executable'}
        $record.window=@{hwnd=$hwnd.ToInt64();class=[AnalizIcon]::WindowClass($hwnd);document='WebView2'}
        Check-Bitmap ([AnalizIcon]::FromWindow($hwnd,0)) ($name+'-window')
        Check-Bitmap ([AnalizIcon]::FromWindow($hwnd,1)) ($name+'-taskbar-hwnd')
        $record.hwnd='verified small and large'
        $group=[AnalizIcon]::GroupMetadata($hwnd)
        $record.group=@{command=$group[0];icon=$group[1];display=$group[2];appId=$group[3];name=$group[4]}
        $ico=Join-Path (Split-Path $exe) 'analysis-reports.ico'
        if($group[0] -ne ('"'+$exe+'"') -or $group[1] -ne ($ico+',0') -or $group[2] -ne ('@'+$exe+',-101') -or
           $group[3] -ne 'local.analiz.rabot.tauri' -or $group[4] -ne 'Анализ работ'){
            throw 'Window relaunch metadata differs from the installed product'
        }
    }catch{$record.hwndError=$_.Exception.Message}
    try{Check-StartMenu $name $exe $record}catch{
        $record.startMenuError=$_.Exception.Message
        $shortcut=Join-Path $programs 'Анализ работ.lnk'
        if(Test-Path -LiteralPath $shortcut){
            $bytes=[IO.File]::ReadAllBytes($shortcut)
            $record.startMenuLinkLength=$bytes.Length
            if($bytes.Length -ge 76){$record.startMenuLinkFlags=[BitConverter]::ToUInt32($bytes,20)}
            $record.startMenuLinkSha256=(Get-FileHash -LiteralPath $shortcut -Algorithm SHA256).Hash
        }
    }
    try{Capture-Taskbar $name $record}catch{$record.taskbarError=$_.Exception.Message}
    $failures=@($record.hwndError,$record.startMenuError,$record.taskbarError) | Where-Object {$_}
    if($failures){throw ($failures -join ' / ')}
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
    $paint=[Diagnostics.Stopwatch]::StartNew()
    while($paint.Elapsed.TotalSeconds -lt 30){
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
            if(([AnalizIcon]::ReadLink($link.FullName)).Target -eq $path){return $link.FullName}
        }
    }
}
function Taskbar-Verb($names){
    $ready=[Diagnostics.Stopwatch]::StartNew()
    do{
        $b=Taskbar-Button
        if(!$b){Start-Sleep -Milliseconds 250}
    }while(!$b -and $ready.Elapsed.TotalSeconds -lt 8)
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
        $wait=[Diagnostics.Stopwatch]::StartNew()
        do{
            $found=Find-Pin $path
            if($found){return $found}
            Start-Sleep -Milliseconds 250
        }while($wait.Elapsed.TotalSeconds -lt 5)
    }
    return Find-Pin $path
}
function New-ProbeShortcut($file,$target,$icon){
    $shell=New-Object -ComObject WScript.Shell
    $link=$shell.CreateShortcut($file)
    $link.TargetPath=$target
    $link.IconLocation=$icon
    $link.Save()
}

$fixtureId=[guid]::NewGuid().ToString('N')
$renamed=Join-Path $pinDir ("audit-renamed-"+$fixtureId+".lnk")
$previous=Join-Path $pinDir ("audit-previous-"+$fixtureId+".lnk")
$custom=Join-Path $pinDir ("audit-custom-"+$fixtureId+".lnk")
$other=Join-Path $pinDir ("audit-other-"+$fixtureId+".lnk")
$exe=$null
try{
    try{
        Check-Bitmap ([AnalizIcon]::FromFile($Candidate)) 'installer';$result.clean.installer='verified'
        $exe=Run-Setup $Candidate
        Check-Bitmap ([AnalizIcon]::FromFile($exe)) 'clean-exe';$result.clean.exe='verified'
        Check-WindowAndShell 'clean' $exe $result.clean
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
    # Exercise persisted links with custom filenames even if this Explorer session cannot pin.
    $null=New-Item -ItemType Directory -Path $pinDir -Force
    New-ProbeShortcut $renamed $oldExe ($oldExe+',0')
    New-ProbeShortcut $previous $oldExe (Join-Path $install 'analysis-bars-blue.ico,0')
    New-ProbeShortcut $custom $oldExe (Join-Path $env:SystemRoot 'System32\shell32.dll,3')
    New-ProbeShortcut $other (Join-Path $env:SystemRoot 'System32\notepad.exe') ($oldExe+',0')
    $customHash=(Get-FileHash -LiteralPath $custom).Hash
    $otherHash=(Get-FileHash -LiteralPath $other).Hash
    $exe=Run-Setup $Candidate -Update
    if($exe -ne $oldExe -or (Get-FileHash $exe -Algorithm SHA256).Hash -eq $oldExeHash){throw 'Existing executable was not replaced in place'}
    Check-Bitmap ([AnalizIcon]::FromFile($exe)) 'upgrade-exe';$result.upgrade.exe='verified in place over stable 1.10.0'
    foreach($linkPath in @($renamed,$previous)){
        $resolved=[AnalizIcon]::ReadLink($linkPath)
        if($resolved.Target -ne $exe -or $resolved.Icon -ne (Join-Path $install 'analysis-reports.ico') -or $resolved.IconIndex -ne 0){
            throw "Shortcut did not receive the current default icon: $linkPath"
        }
        Check-ShortcutIcon $linkPath (Split-Path $linkPath -Leaf)
    }
    if((Get-FileHash -LiteralPath $custom).Hash -ne $customHash -or (Get-FileHash -LiteralPath $other).Hash -ne $otherHash){
        throw 'Installer changed a custom icon or a shortcut for another target'
    }
    $result.upgrade.renamedShortcut='verified default icon, preserved custom and unrelated links'
    Remove-Item -LiteralPath $renamed,$previous,$custom,$other
    if($pin){
        if((Find-Pin $exe) -ne $pin){throw 'The existing pinned shortcut was lost during upgrade'}
        $link=[AnalizIcon]::ReadLink($pin)
        $result.upgrade.pinned=@{target=$link.Target;location=$link.Icon;index=$link.IconIndex}
        Check-ShortcutIcon $pin 'existing-pin'
        $result.upgrade.pinned.resolvedIcon='verified with SHGetFileInfo'
        Capture-Taskbar 'existing-pin' $result.upgrade.pinned
    }
    Check-WindowAndShell 'upgrade' $exe $result.upgrade
    if($result.clean.error){throw $result.clean.error}
}catch{$result.error=$_.Exception.Message;throw}
finally{
    Save-Result
    foreach($fixtureLink in @($renamed,$previous,$custom,$other)){
        if(Test-Path -LiteralPath $fixtureLink){Remove-Item -LiteralPath $fixtureLink}
    }
    if($pin){$null=Taskbar-Verb @('Unpin from taskbar','Открепить от панели задач')}
    if($exe){Stop-Owned $exe}
    Uninstall-Owned
    Remove-Item -LiteralPath $root -Recurse -Force
}
