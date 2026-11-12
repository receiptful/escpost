; Build defines FullVersion, ProductVersion, and SourceDir explicitly.
[Setup]
AppId={{8F907A59-EC48-4FD0-9E1D-31B920D195F4}
AppName=ESCPost
AppVersion={#FullVersion}
VersionInfoVersion={#ProductVersion}
AppPublisher=Receiptful
AppPublisherURL=https://escpost.dev
AppSupportURL=https://github.com/receiptful/escpost/issues
DefaultDirName={localappdata}\Programs\ESCPost
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
; Windows 11 is the initial supported/tested release target.
MinVersion=10.0.22000
OutputBaseFilename=escpost-{#FullVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
SetupIconFile=escpost.ico
UninstallDisplayIcon={app}\escpost-mark.ico
ChangesEnvironment=yes
CloseApplications=yes
RestartApplications=no
; Signing is deferred. Do not configure SignTool or silently claim signatures.
SignedUninstaller=no

[Tasks]
; Checked on first install; Inno remembers the user's choice on upgrades.
Name: "addtopath"; Description: "Add ESCPost to my PATH (open a new terminal after installation)"

[Files]
Source: "escpost.ico"; DestDir: "{app}"; DestName: "escpost-mark.ico"; Flags: ignoreversion
Source: "{#SourceDir}\escpost.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\README.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\licenses\*"; DestDir: "{app}\licenses"; Flags: recursesubdirs createallsubdirs ignoreversion

[InstallDelete]
; Retire our previous detailed icon. The new path also refreshes shell caches.
Type: files; Name: "{app}\escpost.ico"

[Icons]
Name: "{userprograms}\ESCPost Workbench"; Filename: "{app}\escpost.exe"; Parameters: "serve --listen --web-listen"; WorkingDir: "{app}"; IconFilename: "{app}\escpost-mark.ico"

[Code]
const
  OwnershipKey = 'Software\Receiptful\ESCPost';

function TakePathEntry(var Remaining: String): String;
var
  Separator: Integer;
begin
  Separator := Pos(';', Remaining);
  if Separator = 0 then begin
    Result := Remaining;
    Remaining := '';
  end else begin
    Result := Copy(Remaining, 1, Separator - 1);
    Delete(Remaining, 1, Separator);
  end;
end;

function NormalizePathEntry(Value: String): String;
begin
  Value := Trim(Value);
  if (Length(Value) >= 2) and (Value[1] = '"') and (Value[Length(Value)] = '"') then
    Value := Copy(Value, 2, Length(Value) - 2);
  while (Length(Value) > 3) and (Value[Length(Value)] = '\') do
    Delete(Value, Length(Value), 1);
  Result := Lowercase(Value);
end;

function ContainsPathEntry(Path, Entry: String): Boolean;
begin
  Result := False;
  while Path <> '' do
    if NormalizePathEntry(TakePathEntry(Path)) = NormalizePathEntry(Entry) then begin
      Result := True;
      Exit;
    end;
end;

procedure RemoveOwnedPathEntry();
var
  Owned, Path, Entry, Updated: String;
  First, TrailingSeparator: Boolean;
begin
  { Only remove an entry this installer added, never a pre-existing user entry. }
  if not RegQueryStringValue(HKCU, OwnershipKey, 'PathEntry', Owned) then Exit;
  if NormalizePathEntry(Owned) <> NormalizePathEntry(ExpandConstant('{app}')) then Exit;
  if RegQueryStringValue(HKCU, 'Environment', 'Path', Path) then begin
    TrailingSeparator := (Path <> '') and (Path[Length(Path)] = ';');
    Updated := '';
    First := True;
    while Path <> '' do begin
      Entry := TakePathEntry(Path);
      if NormalizePathEntry(Entry) <> NormalizePathEntry(Owned) then begin
        if not First then Updated := Updated + ';';
        Updated := Updated + Entry;
        First := False;
      end;
    end;
    if TrailingSeparator and not First then Updated := Updated + ';';
    if not RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Updated) then
      RaiseException('Could not remove ESCPost from your PATH.');
  end;
  RegDeleteValue(HKCU, OwnershipKey, 'PathEntry');
  RegDeleteKeyIfEmpty(HKCU, OwnershipKey);
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Path, AppDir: String;
begin
  if CurStep <> ssPostInstall then Exit;
  if not WizardIsTaskSelected('addtopath') then begin
    RemoveOwnedPathEntry();
    Exit;
  end;
  AppDir := ExpandConstant('{app}');
  RegQueryStringValue(HKCU, 'Environment', 'Path', Path);
  if ContainsPathEntry(Path, AppDir) then Exit;
  { Preserve even an existing empty final entry when this addition is removed. }
  if Path <> '' then Path := Path + ';';
  if not RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Path + AppDir) then
    RaiseException('Could not add ESCPost to your PATH.');
  if not RegWriteStringValue(HKCU, OwnershipKey, 'PathEntry', AppDir) then
    RaiseException('Could not record the ESCPost PATH entry for uninstallation.');
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then RemoveOwnedPathEntry();
end;
