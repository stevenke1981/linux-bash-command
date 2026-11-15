[Setup]
AppId=AgentCommands.Linux
AppName=Agent Commands Linux
AppVersion=0.2.0
AppPublisher=Agent Commands
DefaultDirName={localappdata}\Programs\AgentCommands
DefaultGroupName=Agent Commands
PrivilegesRequired=lowest
Uninstallable=yes
Compression=zip
SourceDir=payload
OutputBaseFilename=Agent-Commands-0.2.0-Linux-x86_64
LicenseFile=LICENSE
InfoAfterFile=QuickStart.txt

[Files]
; The payload is staged from agentcmd --list before compiling the installer.
; This includes README.md, LICENSE, NOTICE.md, QuickStart.txt and all commands.
Source: "*"; DestDir: "{app}"; Flags: ignoreversion
Source: "shell\*"; DestDir: "{app}\shell"; Flags: ignoreversion
