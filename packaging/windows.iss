[Setup]
AppId=AgentCommands.Tools
AppName=Agent Commands
AppVersion=0.1.0
AppPublisher=Agent Commands
DefaultDirName={localappdata}\Programs\Agent Commands
DefaultGroupName=Agent Commands
PrivilegesRequired=lowest
Uninstallable=yes
Compression=zip
ArchitecturesAllowed=x64compatible
SourceDir=..
OutputBaseFilename=Agent-Commands-0.1.0-Windows-Setup
LicenseFile=LICENSE
InfoAfterFile=packaging\QuickStart.txt

[Files]
Source: "dist\windows-x86_64\*.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "packaging\*.cmd"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "packaging\QuickStart.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "shell\*.ps1"; DestDir: "{app}\shell"; Flags: ignoreversion

[Icons]
Name: "{group}\Agent Commands Terminal"; Filename: "{app}\Agent Commands.cmd"; WorkingDir: "{app}"; Comment: "Open a terminal with Agent Commands on PATH"
Name: "{group}\Uninstall Agent Commands"; Filename: "{uninstallexe}"
