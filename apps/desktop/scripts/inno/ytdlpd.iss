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
; Ledger ground colours in light and dark (docs/design.md, Tokens); needs Inno Setup 6.7
WizardStyle=modern dynamic hidebevels
WizardBackColor=#f2ede3
WizardBackColorDynamicDark=#15130f
WizardSmallImageBackColor=#f2ede3
WizardSmallImageBackColorDynamicDark=#15130f
WizardImageFile=
WizardSmallImageFileDynamicDark=..\..\..\..\icons\ytdlpd\linux\hicolor\256x256\apps\ytdlpd.png
WizardSmallImageFile=..\..\..\..\icons\ytdlpd\linux\hicolor\256x256\apps\ytdlpd.png
SetupIconFile=..\..\assets\windows\ytdlpd.ico
; the running app can hold the exe and python DLLs; ask the user to close it
CloseApplications=yes
; a first install asks for the folder in one window, an existing one updates at once; both end by opening the app
DisableProgramGroupPage=yes
DisableReadyPage=yes
DisableFinishedPage=yes

[Files]
Source: "{#Stage}\ytdlpd.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\*.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\python\*"; DestDir: "{app}\python"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Stage}\THIRD-PARTY.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\ytdlpd"; Filename: "{app}\ytdlpd.exe"
Name: "{autodesktop}\ytdlpd"; Filename: "{app}\ytdlpd.exe"; Check: WantsDesktopIcon

; ytdlpd:// deep link, per user (HKCU) so it works without admin; the app re-writes the same values at launch
[Registry]
Root: HKCU; Subkey: "Software\Classes\ytdlpd"; ValueType: string; ValueData: "URL:ytdlpd"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\ytdlpd"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\ytdlpd\DefaultIcon"; ValueType: string; ValueData: """{app}\ytdlpd.exe"",0"
Root: HKCU; Subkey: "Software\Classes\ytdlpd\shell\open\command"; ValueType: string; ValueData: """{app}\ytdlpd.exe"" add ""%1"""

[Run]
Filename: "{app}\ytdlpd.exe"; Flags: nowait skipifsilent runasoriginaluser

; user data lives under %APPDATA%\ytdlpd and is kept on uninstall

[Code]
const
  // Ledger tokens (docs/design.md) as BGR, dark then light; the tracks are amber at 24% on the ground
  Amber = $3BA2E9;
  AmberLight = $004F8A;
  Track = $1A3548;
  TrackLight = $ACCDD9;
  Paper = $D8E5EC;
  PaperLight = $151A1D;
  Pencil = $84939C;
  PencilLight = $53626B;
  BM_CLICK = $00F5;

type
  TLook = (lkPlain, lkWindow, lkCard);

var
  Asked: Boolean;
  LastPercent, PlainWidth, PlainHeight: Integer;
  ButtonLeft, ButtonTop: array [0..2] of Integer;
  Icon, TrackBar, FillBar: TBitmapImage;
  Title, Status, PercentText, FolderLabel: TNewStaticText;
  DesktopIcon: TNewCheckBox;

function PostMessage(Wnd: HWND; Msg: UINT; W, L: LongInt): BOOL;
  external 'PostMessageW@user32.dll stdcall';

function Pick(Dark, Light: Integer): Integer;
begin
  if IsDarkInstallMode then
    Result := Dark
  else
    Result := Light;
end;

function NewBar(Color: Integer): TBitmapImage;
var
  Dot: TRect;
begin
  Result := TBitmapImage.Create(WizardForm);
  Result.Parent := WizardForm;
  Result.Stretch := True;
  Result.Bitmap.Width := 1;
  Result.Bitmap.Height := 1;
  Dot.Right := 1;
  Dot.Bottom := 1;
  Result.Bitmap.Canvas.Brush.Color := Color;
  Result.Bitmap.Canvas.FillRect(Dot);
end;

function NewText(Color: Integer): TNewStaticText;
begin
  Result := TNewStaticText.Create(WizardForm);
  Result.Parent := WizardForm;
  Result.StyleElements := Result.StyleElements - [seFont];
  Result.Font.Color := Color;
end;

// puts a text at Left, or centred in the window when Left is 0
procedure Place(T: TNewStaticText; Left, Top: Integer);
begin
  T.Top := ScaleY(Top);
  if Left = 0 then
    T.Left := (WizardForm.ClientWidth - T.Width) div 2
  else
    T.Left := ScaleX(Left);
end;

procedure ShowPercent(Percent: Integer);
begin
  FillBar.SetBounds(TrackBar.Left, TrackBar.Top, TrackBar.Width * Percent div 100, TrackBar.Height);
  PercentText.Caption := IntToStr(Percent) + '%';
  if WizardForm.BorderStyle = bsNone then
    PercentText.Left := WizardForm.ClientWidth - ScaleX(16) - PercentText.Width
  else
    PercentText.Left := (WizardForm.ClientWidth - PercentText.Width) div 2;
end;

procedure InitializeWizard;
begin
  PlainWidth := WizardForm.ClientWidth;
  PlainHeight := WizardForm.ClientHeight;
  ButtonLeft[0] := WizardForm.NextButton.Left;
  ButtonTop[0] := WizardForm.NextButton.Top;
  ButtonLeft[1] := WizardForm.CancelButton.Left;
  ButtonTop[1] := WizardForm.CancelButton.Top;
  ButtonLeft[2] := WizardForm.BackButton.Left;
  ButtonTop[2] := WizardForm.BackButton.Top;

  Icon := TBitmapImage.Create(WizardForm);
  Icon.Parent := WizardForm;
  Icon.Stretch := True;
  Icon.BackColor := WizardForm.WizardSmallBitmapImage.BackColor;
  Icon.PngImage := WizardForm.WizardSmallBitmapImage.PngImage;
  Title := NewText(Pick(Paper, PaperLight));
  Title.Font.Size := 11;
  Status := NewText(Pick(Pencil, PencilLight));
  PercentText := NewText(Pick(Amber, AmberLight));
  FolderLabel := NewText(Pick(Pencil, PencilLight));
  FolderLabel.Caption := 'Install to';
  TrackBar := NewBar(Pick(Track, TrackLight));
  FillBar := NewBar(Pick(Amber, AmberLight));
  DesktopIcon := TNewCheckBox.Create(WizardForm);
  DesktopIcon.Parent := WizardForm;
  DesktopIcon.Caption := 'Create a desktop icon';
end;

function WantsDesktopIcon: Boolean;
begin
  Result := DesktopIcon.Checked;
end;

procedure SetLook(Look: TLook; Working: Boolean);
var
  CenterX, CenterY, W, H: Integer;
  Custom, Choosing: Boolean;
begin
  Custom := Look <> lkPlain;
  Choosing := (Look = lkWindow) and not Working;
  CenterX := WizardForm.Left + WizardForm.Width div 2;
  CenterY := WizardForm.Top + WizardForm.Height div 2;
  W := PlainWidth;
  H := PlainHeight;
  if Look = lkWindow then
  begin
    W := ScaleX(480);
    H := ScaleY(320);
  end;
  if Look = lkCard then
  begin
    W := ScaleX(380);
    H := ScaleY(140);
    WizardForm.BorderStyle := bsNone;
  end
  else if WizardForm.BorderStyle = bsNone then
    WizardForm.BorderStyle := bsDialog;
  WizardForm.ClientWidth := W;
  WizardForm.ClientHeight := H;
  WizardForm.Left := CenterX - WizardForm.Width div 2;
  WizardForm.Top := CenterY - WizardForm.Height div 2;

  WizardForm.OuterNotebook.Visible := not Custom;
  Icon.Visible := Custom;
  Title.Visible := Custom;
  Status.Visible := Custom;
  TrackBar.Visible := Custom and not Choosing;
  FillBar.Visible := TrackBar.Visible;
  PercentText.Visible := TrackBar.Visible;
  FolderLabel.Visible := Choosing;
  DesktopIcon.Visible := Choosing;
  WizardForm.DirEdit.Visible := Choosing;
  WizardForm.DirBrowseButton.Visible := Choosing;

  if not Custom then
  begin
    WizardForm.NextButton.Left := ButtonLeft[0];
    WizardForm.NextButton.Top := ButtonTop[0];
    WizardForm.CancelButton.Left := ButtonLeft[1];
    WizardForm.CancelButton.Top := ButtonTop[1];
    WizardForm.BackButton.Left := ButtonLeft[2];
    WizardForm.BackButton.Top := ButtonTop[2];
    exit;
  end;

  WizardForm.BackButton.Visible := False;
  WizardForm.CancelButton.Left := W - ScaleX(16) - WizardForm.CancelButton.Width;
  WizardForm.CancelButton.Top := H - ScaleY(14) - WizardForm.CancelButton.Height;
  WizardForm.NextButton.Left := WizardForm.CancelButton.Left - ScaleX(8) - WizardForm.NextButton.Width;
  WizardForm.NextButton.Top := WizardForm.CancelButton.Top;
  Status.Caption := 'Extracting files';
  if Look = lkCard then
  begin
    WizardForm.NextButton.Visible := False;
    Title.Caption := 'Installing {#Version}';
    Icon.SetBounds(ScaleX(16), ScaleY(16), ScaleX(48), ScaleY(48));
    Place(Title, 76, 20);
    Place(Status, 76, 44);
    PercentText.Top := ScaleY(44);
    TrackBar.SetBounds(ScaleX(16), ScaleY(80), W - ScaleX(32), ScaleY(3));
  end
  else if Choosing then
  begin
    Title.Caption := 'Install ytdlpd';
    WizardForm.NextButton.Caption := SetupMessage(msgButtonInstall);
    Status.Caption := 'Version {#Version}';
    Icon.SetBounds((W - ScaleX(56)) div 2, ScaleY(24), ScaleX(56), ScaleY(56));
    Place(Title, 0, 92);
    Place(Status, 0, 116);
    Place(FolderLabel, 40, 152);
    WizardForm.DirEdit.Parent := WizardForm;
    WizardForm.DirBrowseButton.Parent := WizardForm;
    WizardForm.DirEdit.SetBounds(ScaleX(40), ScaleY(172), ScaleX(304), WizardForm.DirEdit.Height);
    WizardForm.DirBrowseButton.SetBounds(ScaleX(352), ScaleY(172) + (WizardForm.DirEdit.Height - WizardForm.DirBrowseButton.Height) div 2,
      ScaleX(88), WizardForm.DirBrowseButton.Height);
    DesktopIcon.SetBounds(ScaleX(40), ScaleY(208), ScaleX(400), ScaleY(20));
  end
  else
  begin
    WizardForm.NextButton.Visible := False;
    Title.Caption := 'Installing ytdlpd';
    Icon.SetBounds((W - ScaleX(56)) div 2, ScaleY(64), ScaleX(56), ScaleY(56));
    Place(Title, 0, 132);
    Place(Status, 0, 156);
    TrackBar.SetBounds((W - ScaleX(200)) div 2, ScaleY(190), ScaleX(200), ScaleY(3));
    PercentText.Top := ScaleY(202);
  end;
  if not Choosing then
    ShowPercent(0);
end;

// the folder page is the first install's window; an existing install has no folder page. The
// ready page (shown even when disabled if nothing came before it) goes straight on. Any other page (apps to close, an error) keeps Inno's own layout.
procedure CurPageChanged(CurPageID: Integer);
begin
  if WizardSilent then
    exit;
  WizardForm.Caption := 'ytdlpd';
  if CurPageID = wpSelectDir then
  begin
    Asked := True;
    SetLook(lkWindow, False);
  end
  else if CurPageID = wpReady then
  begin
    if Asked then
      SetLook(lkWindow, True)
    else
      SetLook(lkCard, True);
    PostMessage(WizardForm.NextButton.Handle, BM_CLICK, 0, 0);
  end
  else if (CurPageID = wpInstalling) and Asked then
    SetLook(lkWindow, True)
  else if CurPageID = wpInstalling then
    SetLook(lkCard, True)
  else
    SetLook(lkPlain, False);
end;

procedure CurInstallProgressChanged(CurProgress, MaxProgress: Integer);
var
  Path: String;
  Percent: Integer;
begin
  if MaxProgress <= 0 then
    exit;
  Percent := Round(CurProgress * 100.0 / MaxProgress);
  if Percent = LastPercent then
    exit;
  LastPercent := Percent;
  if not WizardSilent then
    ShowPercent(Percent);
  // the app's update card (crates/state/src/updates/apply.rs) reads this file
  Path := ExpandConstant('{param:PROGRESS|}');
  if Path <> '' then
    SaveStringToFile(Path, IntToStr(Percent) + #10, True);
end;
