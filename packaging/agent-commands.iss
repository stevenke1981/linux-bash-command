; Inno Setup 6 script for the Windows x64 installer.
;
; Build (from the repository root, after staging the release binaries in
; dist\windows-x86_64\):
;   iscc /DAppVersion=0.1.0 packaging\agent-commands.iss
; The installer is written to dist\Agent-Commands-<version>-Windows-Setup.exe.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6B1C4F2E-8D3A-4E7B-9C51-2A7F0D9E4B13}
AppName=Agent Commands
AppVersion={#AppVersion}
AppVerName=Agent Commands {#AppVersion}
AppPublisher=Agent Commands
AppPublisherURL=https://github.com/stevenke1981/linux-bash-command
AppSupportURL=https://github.com/stevenke1981/linux-bash-command/issues
DefaultDirName={localappdata}\Programs\Agent Commands
DefaultGroupName=Agent Commands
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
SourceDir=..
OutputDir=dist
OutputBaseFilename=Agent-Commands-{#AppVersion}-Windows-Setup
LicenseFile=LICENSE
InfoAfterFile=packaging\QuickStart.txt
UninstallDisplayName=Agent Commands {#AppVersion}

[Tasks]
Name: "addtopath"; Description: "Add the install directory to my user PATH"; GroupDescription: "Environment:"

[Files]
Source: "dist\windows-x86_64\*.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "packaging\Agent Commands.cmd"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "packaging\QuickStart.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "shell\*"; DestDir: "{app}\shell"; Flags: ignoreversion

[Icons]
Name: "{group}\Agent Commands Terminal"; Filename: "{app}\Agent Commands.cmd"; WorkingDir: "{app}"; Comment: "Open a terminal with Agent Commands on PATH"
Name: "{group}\Uninstall Agent Commands"; Filename: "{uninstallexe}"

[Code]
const
  EnvKey = 'Environment';

function PathContains(const Paths, Dir: string): Boolean;
begin
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Paths) + ';') > 0;
end;

procedure AddToUserPath(const Dir: string);
var
  Paths: string;
begin
  if not RegQueryStringValue(HKCU, EnvKey, 'Path', Paths) then
    Paths := '';
  if PathContains(Paths, Dir) then
    exit;
  if (Paths <> '') and (Copy(Paths, Length(Paths), 1) <> ';') then
    Paths := Paths + ';';
  RegWriteExpandStringValue(HKCU, EnvKey, 'Path', Paths + Dir);
end;

procedure RemoveFromUserPath(const Dir: string);
var
  Paths: string;
  P: Integer;
begin
  if not RegQueryStringValue(HKCU, EnvKey, 'Path', Paths) then
    exit;
  Paths := ';' + Paths + ';';
  P := Pos(';' + Uppercase(Dir) + ';', Uppercase(Paths));
  if P = 0 then
    exit;
  Delete(Paths, P, Length(Dir) + 1);
  Paths := Copy(Paths, 2, Length(Paths) - 2);
  RegWriteExpandStringValue(HKCU, EnvKey, 'Path', Paths);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    AddToUserPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveFromUserPath(ExpandConstant('{app}'));
end;
