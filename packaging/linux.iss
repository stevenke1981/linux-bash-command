[Setup]
AppId=AgentCommands.Linux
AppName=Agent Commands Linux
AppVersion=0.1.0
AppPublisher=Agent Commands
DefaultDirName={localappdata}\Programs\AgentCommands
DefaultGroupName=Agent Commands
PrivilegesRequired=lowest
Uninstallable=yes
Compression=zip
SourceDir=payload
OutputBaseFilename=Agent-Commands-0.1.0-Linux-x86_64
LicenseFile=LICENSE
InfoAfterFile=QuickStart.txt

[Files]
Source: "search"; DestDir: "{app}"; Flags: ignoreversion
Source: "diff"; DestDir: "{app}"; Flags: ignoreversion
Source: "list"; DestDir: "{app}"; Flags: ignoreversion
Source: "read"; DestDir: "{app}"; Flags: ignoreversion
Source: "rg"; DestDir: "{app}"; Flags: ignoreversion
Source: "ls"; DestDir: "{app}"; Flags: ignoreversion
Source: "pwd"; DestDir: "{app}"; Flags: ignoreversion
Source: "cd"; DestDir: "{app}"; Flags: ignoreversion
Source: "cat"; DestDir: "{app}"; Flags: ignoreversion
Source: "less"; DestDir: "{app}"; Flags: ignoreversion
Source: "head"; DestDir: "{app}"; Flags: ignoreversion
Source: "tail"; DestDir: "{app}"; Flags: ignoreversion
Source: "grep"; DestDir: "{app}"; Flags: ignoreversion
Source: "find"; DestDir: "{app}"; Flags: ignoreversion
Source: "ps"; DestDir: "{app}"; Flags: ignoreversion
Source: "df"; DestDir: "{app}"; Flags: ignoreversion
Source: "curl"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "QuickStart.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "shell\cd.bash"; DestDir: "{app}\shell"; Flags: ignoreversion
