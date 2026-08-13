; The Windows installer for zz. The release workflow compiles it with
;
;   ISCC.exe packaging\windows\zz.iss /DAppVersion=1.2.3 /DVersionInfo=1.2.3 ^
;     /DBundleDir=dist\zz /DOutputDir=dist /DOutputBaseFilename=zz-1.2.3-windows-x64-setup
;
; The defaults below match `just build windows`, so a bare
; `ISCC.exe packaging\windows\zz.iss` produces an unversioned local installer
; from dist\zz. Requires Inno Setup 6.3 or newer (ArchitecturesAllowed spellings).

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
; VersionInfoVersion rejects prerelease suffixes, so the caller passes the
; numeric part separately (0.1.0 for a 0.1.0-rc.1 tag).
#ifndef VersionInfo
  #define VersionInfo "0.0.0"
#endif
#ifndef BundleDir
  #define BundleDir "dist\zz"
#endif
#ifndef OutputDir
  #define OutputDir "dist"
#endif
#ifndef OutputBaseFilename
  #define OutputBaseFilename "zz-windows-x64-setup"
#endif

#define RepoRoot AddBackslash(SourcePath) + "..\.."

[Setup]
; Never change AppId: it is the upgrade and uninstall identity, and the winget
; manifest derives its ProductCode ({AppId}_is1) from it.
AppId={{4DAEF46A-646C-4AA3-9391-E31B4D32319D}
AppName=zz
AppVersion={#AppVersion}
VersionInfoVersion={#VersionInfo}
AppPublisher=demfabris
AppPublisherURL=https://github.com/demfabris/zz
AppSupportURL=https://github.com/demfabris/zz/issues
AppUpdatesURL=https://github.com/demfabris/zz/releases
; Inno resolves every relative path against SourceDir, which defaults to the
; directory holding this script. Point it at the repository root so BundleDir
; and OutputDir read the way the rest of the repository writes paths.
SourceDir={#RepoRoot}
; Per-user by default: no elevation, installs under %LOCALAPPDATA%\Programs\zz.
; The user can still pick a machine-wide install from the wizard's first page.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DefaultDirName={autopf}\zz
; The Start Menu entry is a bare shortcut rather than a folder, so there is no
; program group to choose.
DisableProgramGroupPage=yes
UninstallDisplayName=zz
UninstallDisplayIcon={app}\zz.exe
; CEF 151 drops anything older than Windows 10, and only x64 is built.
MinVersion=10.0
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}
; The same multi-resolution icon xtask embeds into the bundled zz.exe.
SetupIconFile={#SourcePath}\..\..\assets\windows\zz.ico

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; The CEF bundle ships its own CREDITS.html and CEF_LICENSE.txt; the PDB is a
; build artifact and would multiply the installer's size.
Source: "{#BundleDir}\*"; DestDir: "{app}"; Excludes: "*.pdb"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#RepoRoot}\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\zz"; Filename: "{app}\zz.exe"
Name: "{autodesktop}\zz"; Filename: "{app}\zz.exe"; Tasks: desktopicon

; runasoriginaluser keeps zz (and the Chromium it hosts) out of an elevated
; session when someone picks the machine-wide install from the wizard.
[Run]
Filename: "{app}\zz.exe"; Description: "{cm:LaunchProgram,zz}"; Flags: nowait postinstall skipifsilent runasoriginaluser

; CEF writes caches and logs next to the executable, so uninstalling has to take
; the whole directory rather than only the files this installer laid down.
; User settings under %APPDATA%\zz survive on purpose, matching the cask, which
; only removes them on an explicit `brew uninstall --zap`.
[UninstallDelete]
Type: filesandordirs; Name: "{app}"

; TODO: add a PATH task once the Windows bundle carries a CLI. The bundled
; zz.exe is the GUI entry point; `cargo xtask bundle-cef` does not install a
; zz_cli.exe the way the macOS bundle does.
