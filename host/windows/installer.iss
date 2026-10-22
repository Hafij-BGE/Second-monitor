; Inno Setup script. Built by CI: iscc /DAppVersion=x.y.z installer.iss
#ifndef AppVersion
  #define AppVersion "0.2.0"
#endif

[Setup]
AppId={{8C3F4E2A-6B1D-4F7A-9E0C-5D2B7A1C3E94}
AppName=Extendo
AppVersion={#AppVersion}
AppPublisher=Md Hafijur Rahman
AppPublisherURL=https://github.com/Hafij-BGE
AppCopyright=Md Hafijur Rahman
DefaultDirName={autopf}\Extendo
DefaultGroupName=Extendo
OutputDir=..\target\installer
OutputBaseFilename=extendo-setup-{#AppVersion}
SetupIconFile=..\..\assets\extendo.ico
UninstallDisplayIcon={app}\extendo.exe
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"
Name: "startup"; Description: "Start Extendo in the background when I log in"; GroupDescription: "Background:"; Flags: unchecked

[Files]
Source: "..\target\release\extendo.exe"; DestDir: "{app}"; Flags: ignoreversion
; scrcpy (Extendo Mirror) and adb (USB), downloaded by CI into windows\scrcpy.
Source: "scrcpy\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs skipifsourcedoesntexist

[Icons]
Name: "{group}\Extendo"; Filename: "{app}\extendo.exe"
Name: "{group}\Uninstall Extendo"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Extendo"; Filename: "{app}\extendo.exe"; Tasks: desktopicon

[Registry]
; Same entry as "Start Extendo when I log in" in the app; removed on uninstall either way.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Extendo"; ValueData: """{app}\extendo.exe"" --background"; Flags: uninsdeletevalue; Tasks: startup
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "Extendo"; Flags: uninsdeletevalue dontcreatekey

[Run]
; Let WiFi viewers reach the host.
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""Extendo"" dir=in action=allow program=""{app}\extendo.exe"" enable=yes"; Flags: runhidden
Filename: "{app}\extendo.exe"; Description: "Start Extendo"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Extendo may be running in the tray.
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM extendo.exe"; Flags: runhidden; RunOnceId: "StopExtendo"
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Extendo"""; Flags: runhidden; RunOnceId: "RemoveFirewallRule"

[Code]
// Extendo keeps running in the tray after its window closes; stop it so
// its files can be replaced.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM extendo.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Result := '';
end;
