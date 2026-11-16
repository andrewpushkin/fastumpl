; Inno Setup 7 script for the fastumpl Windows installer.
;
; Built by packaging/windows/installer.sh (Docker, under Wine) and by the
; release workflow (Windows runner). Defines passed on the command line:
;   AppVersion   0.1.N
;   Arch         Inno architecture: x64compatible | arm64
;   SourceDir    folder with fastumpl.exe, umpl.exe and the other files
;   OutputDir    where the setup .exe goes
;   OutputName   setup file name without .exe

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef Arch
  #define Arch "x64compatible"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\dist\fastumpl-windows-x86_64-portable"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif
#ifndef OutputName
  #define OutputName "fastumpl-windows-x86_64-setup"
#endif

[Setup]
AppId={{6E1C5D0B-3F1A-4C8E-9B53-2B7D3A6C9F41}
AppName=fastumpl
AppVersion={#AppVersion}
AppVerName=fastumpl {#AppVersion}
AppPublisher=fastumpl contributors
AppPublisherURL=https://github.com/andrewpushkin/fastumpl
AppSupportURL=https://github.com/andrewpushkin/fastumpl/issues
AppUpdatesURL=https://andrewpushkin.github.io/fastumpl/
VersionInfoVersion={#AppVersion}.0
DefaultDirName={autopf}\fastumpl
DefaultGroupName=fastumpl
DisableProgramGroupPage=yes
; Install for the current user by default; offer all users.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed={#Arch}
ArchitecturesInstallIn64BitMode={#Arch}
ChangesAssociations=yes
ChangesEnvironment=yes
LicenseFile={#SourceDir}\LICENSE.txt
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\fastumpl.ico
OutputDir={#OutputDir}
OutputBaseFilename={#OutputName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "association"; Description: "Open .umpl diagram files with fastumpl"; GroupDescription: "File types:"
Name: "addtopath"; Description: "Add the umpl command line tool to PATH"; GroupDescription: "Command line:"; Flags: unchecked

[Files]
Source: "{#SourceDir}\fastumpl.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\umpl.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\*.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#SourceDir}\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\DejaVu-LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\examples\*.umpl"; DestDir: "{app}\examples"; Flags: ignoreversion
Source: "..\..\assets\icon.ico"; DestDir: "{app}"; DestName: "fastumpl.ico"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\fastumpl"; Filename: "{app}\fastumpl.exe"; IconFilename: "{app}\fastumpl.ico"
Name: "{autodesktop}\fastumpl"; Filename: "{app}\fastumpl.exe"; IconFilename: "{app}\fastumpl.ico"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Classes\.umpl"; ValueType: string; ValueName: ""; ValueData: "fastumpl.diagram"; Flags: uninsdeletevalue; Tasks: association
Root: HKA; Subkey: "Software\Classes\fastumpl.diagram"; ValueType: string; ValueName: ""; ValueData: "fastumpl diagram"; Flags: uninsdeletekey; Tasks: association
Root: HKA; Subkey: "Software\Classes\fastumpl.diagram\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\fastumpl.ico"; Tasks: association
Root: HKA; Subkey: "Software\Classes\fastumpl.diagram\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\fastumpl.exe"" ""%1"""; Tasks: association
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Check: NeedsAddPath(ExpandConstant('{app}')); Tasks: addtopath

[Run]
Filename: "{app}\fastumpl.exe"; Description: "{cm:LaunchProgram,fastumpl}"; Flags: nowait postinstall skipifsilent

[Code]
function NeedsAddPath(Dir: string): Boolean;
var
  Path: string;
begin
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Path) then
  begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Path) + ';') = 0;
end;
