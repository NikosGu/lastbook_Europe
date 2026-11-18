; Lastbook Europe for Windows: the installer, made with Inno Setup by package_windows.ps1.
; Installed for this user only (no administrator needed), in %LOCALAPPDATA%\Programs\Lastbook Europe, with a Start menu
; entry. The window needs Microsoft Edge WebView2, which Windows 10 and 11 already have.
#define Name "Lastbook Europe"
#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Stage
  #define Stage "stage"
#endif
#ifndef Out
  #define Out "dist"
#endif

[Setup]
AppId={{6F1C2D3A-8B4E-4E2F-9C51-3A7D0E1B5C42}
AppName={#Name}
AppVersion={#Version}
AppPublisher=Lastbook Europe
AppPublisherURL=https://github.com/NikosGu/lastbook_Europe
AppContact=nikos.gu.devs@gmail.com
AppSupportURL=https://github.com/NikosGu/lastbook_Europe/issues
DefaultDirName={localappdata}\Programs\{#Name}
DefaultGroupName={#Name}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#Out}
OutputBaseFilename=LastbookEurope-{#Version}-windows-x64-setup
SetupIconFile=..\icon\Lastbook.ico
UninstallDisplayIcon={app}\lastbook.exe
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "it"; MessagesFile: "compiler:Languages\Italian.isl"
Name: "de"; MessagesFile: "compiler:Languages\German.isl"
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "pl"; MessagesFile: "compiler:Languages\Polish.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Stage}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#Name}"; Filename: "{app}\lastbook.exe"
Name: "{userdesktop}\{#Name}"; Filename: "{app}\lastbook.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\lastbook.exe"; Description: "{cm:LaunchProgram,{#Name}}"; Flags: nowait postinstall skipifsilent
