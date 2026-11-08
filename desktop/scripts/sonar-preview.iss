#ifndef SonarVersion
  #error SonarVersion must be supplied by the build script
#endif

[Setup]
AppId={{A40C7BB7-9384-4CB3-B73C-2CCDE1B3D049}
AppName=Sonar Desktop Preview
AppVersion={#SonarVersion}
DefaultDirName={localappdata}\Programs\Sonar Desktop Preview
DefaultGroupName=Sonar Desktop Preview
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=Sonar-Desktop-Preview-{#SonarVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "..\target\release\sonar-desktop.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\WINDOWS-PREVIEW.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Sonar Desktop Preview"; Filename: "{app}\sonar-desktop.exe"
Name: "{autodesktop}\Sonar Desktop Preview"; Filename: "{app}\sonar-desktop.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\sonar-desktop.exe"; Description: "Launch Sonar Desktop Preview"; Flags: nowait postinstall skipifsilent
