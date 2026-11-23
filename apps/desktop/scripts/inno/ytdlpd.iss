; Inno Setup script. Built by scripts/package-windows.ps1:
;   ISCC /DStage=<dir> /DVersion=<x.y.z> /DArch=x64|arm64 /DOut=<dir> ytdlpd.iss
; <Stage> holds ytdlpd.exe, the Python DLLs and python\ (the shipped runtime, no libs\).
#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Arch
  #define Arch "x64"
#endif

[Setup]
AppId={{6B1C0E52-3D54-4E0B-9F7A-5A7C3C0F9E21}
AppName=ytdlpd
AppVersion={#Version}
AppPublisher=ytdlpd
DefaultDirName={autopf}\ytdlpd
DefaultGroupName=ytdlpd
UninstallDisplayIcon={app}\ytdlpd.exe
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed={#Arch}compatible
ArchitecturesInstallIn64BitMode={#Arch}compatible
Compression=lzma2/max
SolidCompression=yes
OutputDir={#Out}
OutputBaseFilename=ytdlpd-{#Version}-windows-{#Arch}-setup
; Ledger ground colours in light and dark (docs/design.md, Tokens); they need Inno Setup 6.7
#if Ver >= EncodeVer(6,7,0)
WizardStyle=modern dynamic hidebevels
WizardBackColor=#f2ede3
WizardBackColorDynamicDark=#15130f
WizardSmallImageBackColor=#f2ede3
WizardSmallImageBackColorDynamicDark=#15130f
WizardImageFile=
WizardSmallImageFileDynamicDark=..\..\..\..\icons\ytdlpd\linux\hicolor\256x256\apps\ytdlpd.png
#else
WizardStyle=modern
#endif
WizardSmallImageFile=..\..\..\..\icons\ytdlpd\linux\hicolor\256x256\apps\ytdlpd.png
SetupIconFile=..\..\assets\windows\ytdlpd.ico
; the running app can hold the exe and python DLLs; ask the user to close it
CloseApplications=yes

[Files]
Source: "{#Stage}\ytdlpd.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\*.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\python\*"; DestDir: "{app}\python"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Stage}\THIRD-PARTY.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\ytdlpd"; Filename: "{app}\ytdlpd.exe"
Name: "{autodesktop}\ytdlpd"; Filename: "{app}\ytdlpd.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop icon"; Flags: unchecked

; ytdlpd:// deep link, per user (HKCU) so it works without admin; the app re-writes the same values at launch
[Registry]
Root: HKCU; Subkey: "Software\Classes\ytdlpd"; ValueType: string; ValueData: "URL:ytdlpd"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\ytdlpd"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\ytdlpd\DefaultIcon"; ValueType: string; ValueData: """{app}\ytdlpd.exe"",0"
Root: HKCU; Subkey: "Software\Classes\ytdlpd\shell\open\command"; ValueType: string; ValueData: """{app}\ytdlpd.exe"" add ""%1"""

[Run]
Filename: "{app}\ytdlpd.exe"; Description: "Launch ytdlpd"; Flags: nowait postinstall skipifsilent

; user data lives under %APPDATA%\ytdlpd and is kept on uninstall

; the update card (crates/state/src/updates/apply.rs) reads the percentage from the /PROGRESS file
[Code]
var
  LastPercent: Integer;

procedure CurInstallProgressChanged(CurProgress, MaxProgress: Integer);
var
  Path: String;
  Percent: Integer;
begin
  Path := ExpandConstant('{param:PROGRESS|}');
  if (Path = '') or (MaxProgress <= 0) then
    exit;
  Percent := Round(CurProgress * 100.0 / MaxProgress);
  if Percent <> LastPercent then
  begin
    LastPercent := Percent;
    SaveStringToFile(Path, IntToStr(Percent) + #10, True);
  end;
end;
