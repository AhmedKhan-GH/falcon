; The Windows installer, built by scripts/package-windows.sh, which passes the defines below. It
; installs for the current user, needing no admin rights, into %LOCALAPPDATA%\Programs\Falcon.
;
;   VERSION          from Cargo.toml
;   OUTFILE          the installer to write
;   ICON             the installer's and uninstaller's icon
;   FILES_INSTALL    generated: installs each file of the app folder
;   FILES_UNINSTALL  generated: removes those files, by name
;
; Silent use: /S installs or uninstalls, /DESKTOP also adds a desktop shortcut, /D=<folder> (last)
; installs elsewhere. If Falcon is running, a silent run changes nothing and exits with status 2.
;
; The uninstaller deletes the files and shortcuts the installer made, one by one, and the folder only
; if that leaves it empty. It never touches libraries or settings.

Unicode true
SetCompressor /SOLID /FINAL lzma
RequestExecutionLevel user
ManifestDPIAware true

!include MUI2.nsh
!include LogicLib.nsh
!include FileFunc.nsh
!include x64.nsh
!include WinVer.nsh

!define PUBLISHER "Ahmed Khan"
!define EXE "falcon-app.exe"
!define WORKER "falcon-ifc-worker.exe"
!define UNINSTALLER "Uninstall Falcon.exe"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Falcon"

Name "Falcon"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\Falcon"
InstallDirRegKey HKCU "${UNINST_KEY}" "InstallLocation"
BrandingText "Falcon ${VERSION}"

VIProductVersion "${VERSION}.0"
VIFileVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "Falcon"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "Falcon ${VERSION} Setup"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "Copyright (c) 2026 ${PUBLISHER}"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING

!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Launch Falcon"
!define MUI_FINISHPAGE_RUN_FUNCTION LaunchFalcon
; The finish page's second checkbox, which MUI calls its readme.
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Create a desktop shortcut"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION AddDesktopShortcut
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!define MUI_UNCONFIRMPAGE_TEXT_TOP "Falcon will be removed from your computer. Your libraries and your settings stay where they are."
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Var Previous
Var WantDesktop

; Stops with Retry and Cancel while Falcon runs from the folder in $R0. A running program's file can't
; be opened for writing, so this finds exactly that copy, not one running from anywhere else.
!macro NOT_RUNNING_FUNCTION un
Function ${un}CheckFile
  Pop $R1
  retry:
    ${If} ${FileExists} "$R1"
      ClearErrors
      FileOpen $R2 "$R1" a
      ${If} ${Errors}
        MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Falcon is running. Close it, then choose Retry." /SD IDCANCEL IDRETRY retry
        Abort "Falcon is running."
      ${EndIf}
      FileClose $R2
    ${EndIf}
FunctionEnd

Function ${un}NotRunning
  Push "$R0\${EXE}"
  Call ${un}CheckFile
  Push "$R0\${WORKER}"
  Call ${un}CheckFile
FunctionEnd
!macroend
!insertmacro NOT_RUNNING_FUNCTION ""
!insertmacro NOT_RUNNING_FUNCTION "un."

Function .onInit
  ${IfNot} ${RunningX64}
  ${OrIfNot} ${AtLeastWin10}
    MessageBox MB_OK|MB_ICONSTOP "Falcon needs 64-bit Windows 10 or later." /SD IDOK
    SetErrorLevel 3
    Quit
  ${EndIf}
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/DESKTOP" $1
  ${IfNot} ${Errors}
    StrCpy $WantDesktop "1"
  ${EndIf}
FunctionEnd

Function WriteDesktopShortcut
  CreateShortCut "$DESKTOP\Falcon.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0
  ; The uninstaller removes it only if this says the installer made it.
  WriteRegDWORD HKCU "${UNINST_KEY}" "DesktopShortcut" 1
FunctionEnd

Function AddDesktopShortcut
  SetOutPath "$INSTDIR"
  Call WriteDesktopShortcut
FunctionEnd

Function LaunchFalcon
  SetOutPath "$INSTDIR"
  Exec '"$INSTDIR\${EXE}"'
FunctionEnd

Section
  ; An older version here (or wherever it was installed) removes itself first, so files it had that
  ; this one doesn't are gone and only what's listed below remains. Anything else stays.
  ReadRegStr $Previous HKCU "${UNINST_KEY}" "InstallLocation"
  ${If} $Previous == ""
    StrCpy $Previous "$INSTDIR"
  ${EndIf}
  StrCpy $R0 "$Previous"
  Call NotRunning
  StrCpy $R0 "$INSTDIR"
  Call NotRunning
  ReadRegDWORD $0 HKCU "${UNINST_KEY}" "DesktopShortcut"
  ${If} $0 == 1
    StrCpy $WantDesktop "1"
  ${EndIf}
  ${If} ${FileExists} "$Previous\${UNINSTALLER}"
    ClearErrors
    ExecWait '"$Previous\${UNINSTALLER}" /S _?=$Previous' $0
    ${If} ${Errors}
    ${OrIf} $0 != 0
      Abort "The installed version of Falcon could not be removed."
    ${EndIf}
    Delete "$Previous\${UNINSTALLER}"
    RMDir "$Previous"
  ${EndIf}

  SetOutPath "$INSTDIR"
  !include "${FILES_INSTALL}"
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\${UNINSTALLER}"
  CreateShortCut "$SMPROGRAMS\Falcon.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0

  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "Falcon"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${EXE},0"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\${UNINSTALLER}"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\${UNINSTALLER}" /S'
  WriteRegStr HKCU "${UNINST_KEY}" "URLInfoAbout" "https://ahmedkhan-gh.github.io/falcon/"
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" $0

  ${If} $WantDesktop == "1"
    Call WriteDesktopShortcut
  ${EndIf}
SectionEnd

Section "Uninstall"
  StrCpy $R0 "$INSTDIR"
  Call un.NotRunning
  !include "${FILES_UNINSTALL}"
  Delete "$SMPROGRAMS\Falcon.lnk"
  ReadRegDWORD $0 HKCU "${UNINST_KEY}" "DesktopShortcut"
  ${If} $0 == 1
    Delete "$DESKTOP\Falcon.lnk"
  ${EndIf}
  DeleteRegKey HKCU "${UNINST_KEY}"
  Delete "$INSTDIR\${UNINSTALLER}"
  ; Not /r: a folder with anything else in it stays.
  RMDir "$INSTDIR"
SectionEnd
