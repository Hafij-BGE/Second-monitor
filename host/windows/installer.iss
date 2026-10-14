; Inno Setup script. Built by CI: iscc /DAppVersion=x.y.z installer.iss
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
AppId={{8C3F4E2A-6B1D-4F7A-9E0C-5D2B7A1C3E94}
AppName=Second Monitor Host
AppVersion={#AppVersion}
AppPublisher=Hafij-BGE
DefaultDirName={autopf}\Second Monitor
DefaultGroupName=Second Monitor
OutputDir=..\target\installer
OutputBaseFilename=second-monitor-setup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
WizardStyle=modern

[Files]
Source: "..\target\release\second-monitor.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Second Monitor Host"; Filename: "{app}\second-monitor.exe"
Name: "{group}\Second Monitor Host (mirror)"; Filename: "{app}\second-monitor.exe"; Parameters: "--mode mirror"
Name: "{group}\Uninstall Second Monitor"; Filename: "{uninstallexe}"

[Run]
; Let WiFi viewers reach the host.
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""Second Monitor"" dir=in action=allow program=""{app}\second-monitor.exe"" enable=yes"; Flags: runhidden
Filename: "{app}\second-monitor.exe"; Description: "Start Second Monitor Host"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Second Monitor"""; Flags: runhidden; RunOnceId: "RemoveFirewallRule"
