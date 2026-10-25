; Per-user installer for Desktop Icons Sorter.
;
; Build (from the repo root, after `cargo build --release`):
;   makensis -DVERSION=0.2.0 -DOUTFILE=..\dist\desktop-icons-sorter-0.2.0-setup.exe installer\installer.nsi
; (OUTFILE is relative to this folder.)
;
; Installs to %LOCALAPPDATA%\Programs\Desktop Icons Sorter, where the Electron
; builds installed too, and removes an Electron install it finds there first.
; Settings (%APPDATA%\desktop-icons-sorter) and panel folders are never
; touched, so panels carry over. The Run entry it adds is what makes the app
; start with Windows (and tells the app, on its first run, that it was
; installed rather than run as the portable exe).

Unicode true
!include "MUI2.nsh"

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef OUTFILE
  !define OUTFILE "desktop-icons-sorter-${VERSION}-setup.exe"
!endif

!define APP "Desktop Icons Sorter"
!define EXE "Desktop Icons Sorter.exe"
!define APP_ID "com.shadowhussein.desktopiconssorter"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"

Name "${APP}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP}"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "FileDescription" "${APP} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "MIT License"

!define MUI_ICON "..\build\icon.ico"
!define MUI_UNICON "..\build\icon.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP}"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; Closes a running copy (Rust or Electron) so its exe can be replaced. Both
; save their settings as they go, so nothing is lost.
!macro CloseRunning
  nsExec::Exec 'taskkill /IM "${EXE}" /F'
  nsExec::Exec 'taskkill /IM "desktop-icons-sorter.exe" /F'
  Sleep 500
!macroend

Section "Install"
  !insertmacro CloseRunning

  ; An Electron build left its own uninstaller; run it quietly first, so its
  ; ~200 MB of Chromium files don't linger next to the new exe.
  IfFileExists "$INSTDIR\Uninstall ${APP}.exe" 0 +3
    ExecWait '"$INSTDIR\Uninstall ${APP}.exe" /S _?=$INSTDIR'
    Delete "$INSTDIR\Uninstall ${APP}.exe"

  SetOutPath "$INSTDIR"
  File "/oname=${EXE}" "..\target\release\desktop-icons-sorter.exe"
  File "..\LICENSE"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateShortcut "$SMPROGRAMS\${APP}.lnk" "$INSTDIR\${EXE}"
  WriteRegStr HKCU "${RUN_KEY}" "${APP_ID}" '"$INSTDIR\${EXE}"'

  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "shadow-hussein"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${EXE}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  !insertmacro CloseRunning
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP}.lnk"
  DeleteRegValue HKCU "${RUN_KEY}" "${APP_ID}"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  ; Settings and panel folders stay: they hold the user's files.
SectionEnd
