; RawBlow Windows installer (NSIS 3, MUI2).
; Built by scripts\build-release-windows.ps1, which first stages RawBlow.exe + all runtime
; DLLs (ORT/Dawn + VC++ CRT) into a folder, then calls:
;   makensis /DAPPVERSION=0.5.0 /DSTAGEDIR="<stage>" /DOUTDIR="<dist>" scripts\rawblow.nsi
; Produces <dist>\RawBlow-Setup-v<APPVERSION>.exe - a single self-contained installer.
; NSIS is zlib-licensed (free for commercial use).

Unicode true
!include "MUI2.nsh"
!include "x64.nsh"

!ifndef APPVERSION
  !define APPVERSION "0.0.0"
!endif
!ifndef STAGEDIR
  !define STAGEDIR "..\dist\stage"
!endif
!ifndef OUTDIR
  !define OUTDIR "..\dist"
!endif

!define APPNAME "RawBlow"
!define PUBLISHER "Hare"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}"

Name "${APPNAME} ${APPVERSION}"
OutFile "${OUTDIR}\RawBlow-Setup-v${APPVERSION}.exe"
InstallDir "$PROGRAMFILES64\${APPNAME}"
InstallDirRegKey HKLM "Software\${APPNAME}" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma

VIProductVersion "${APPVERSION}.0"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "FileVersion" "${APPVERSION}.0"
VIAddVersionKey "ProductVersion" "${APPVERSION}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "FileDescription" "${APPNAME} Installer"
VIAddVersionKey "LegalCopyright" "(C) ${PUBLISHER}"

!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\RawBlow.exe"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
; First language is the fallback default; NSIS auto-selects by system locale when present.
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Korean"

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "RawBlow requires 64-bit Windows."
    Abort
  ${EndIf}
  SetRegView 64
FunctionEnd

Function un.onInit
  SetRegView 64
FunctionEnd

Section "Install"
  SetOutPath "$INSTDIR"
  ; RawBlow.exe + Dawn GPU DLLs + VC++ runtime DLLs (everything the staging folder holds).
  File "${STAGEDIR}\RawBlow.exe"
  File "${STAGEDIR}\*.dll"

  CreateShortCut "$SMPROGRAMS\${APPNAME}.lnk" "$INSTDIR\RawBlow.exe"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr   HKLM "Software\${APPNAME}" "InstallDir" "$INSTDIR"
  WriteRegStr   HKLM "${UNINSTKEY}" "DisplayName"     "${APPNAME}"
  WriteRegStr   HKLM "${UNINSTKEY}" "DisplayVersion"  "${APPVERSION}"
  WriteRegStr   HKLM "${UNINSTKEY}" "DisplayIcon"     "$INSTDIR\RawBlow.exe"
  WriteRegStr   HKLM "${UNINSTKEY}" "Publisher"       "${PUBLISHER}"
  WriteRegStr   HKLM "${UNINSTKEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr   HKLM "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKLM "${UNINSTKEY}" "EstimatedSize"   57000
  WriteRegDWORD HKLM "${UNINSTKEY}" "NoModify"        1
  WriteRegDWORD HKLM "${UNINSTKEY}" "NoRepair"        1
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\*.dll"
  Delete "$INSTDIR\RawBlow.exe"
  Delete "$INSTDIR\Uninstall.exe"
  Delete "$SMPROGRAMS\${APPNAME}.lnk"
  RMDir  "$INSTDIR"
  DeleteRegKey HKLM "${UNINSTKEY}"
  DeleteRegKey HKLM "Software\${APPNAME}"
SectionEnd
