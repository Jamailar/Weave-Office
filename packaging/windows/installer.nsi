Unicode true
!include "MUI2.nsh"
!include "x64.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

Name "Weave Office"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\Weave Office"
InstallDirRegKey HKCU "Software\WeaveOffice" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
!ifdef SIGN_COMMAND
!uninstfinalize '${SIGN_COMMAND}' = 0
!finalize '${SIGN_COMMAND}' = 0
!endif
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "Weave Office"
VIAddVersionKey "FileDescription" "Weave Office ${ARCH} installer"
VIAddVersionKey "FileVersion" "${VERSION}"

!define MUI_ABORTWARNING
!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"

; Automatic updates run this installer as `/S /UPDATE [/RELAUNCH] /D=<current folder>` just
; before the application exits: wait for it to release weave-office.exe, install over it, then
; start the new version when asked to.
Var UpdateMode
Var Relaunch

Function .onInit
!if "${ARCH}" == "arm64"
  ${IfNot} ${IsNativeARM64}
    MessageBox MB_ICONSTOP "This installer requires Windows ARM64."
    Abort
  ${EndIf}
!else if "${ARCH}" == "x64"
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "This installer requires 64-bit Windows."
    Abort
  ${EndIf}
!endif
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/UPDATE" $R1
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
  ClearErrors
  ${GetOptions} $R0 "/RELAUNCH" $R1
  ${IfNot} ${Errors}
    StrCpy $Relaunch 1
  ${EndIf}
FunctionEnd

; A running weave-office.exe cannot be opened for writing. Wait up to 60 seconds for it to exit.
Function WaitForApplication
  StrCpy $R2 0
  ${Do}
    ${IfNot} ${FileExists} "$INSTDIR\weave-office.exe"
      ${Break}
    ${EndIf}
    ClearErrors
    FileOpen $R3 "$INSTDIR\weave-office.exe" a
    ${IfNot} ${Errors}
      FileClose $R3
      ${Break}
    ${EndIf}
    IntOp $R2 $R2 + 1
    ${If} $R2 >= 300
      ${Break}
    ${EndIf}
    Sleep 200
  ${Loop}
FunctionEnd

Section "Weave Office"
  SetShellVarContext current
  ${If} $UpdateMode == 1
    Call WaitForApplication
  ${EndIf}
  SetOutPath "$INSTDIR"
  File "${STAGE}/weave-office.exe"
  SetOutPath "$INSTDIR\licenses"
  File /r "${STAGE}/licenses/*"
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortcut "$SMPROGRAMS\Weave Office.lnk" "$INSTDIR\weave-office.exe" "" "$INSTDIR\weave-office.exe" 0
  WriteRegStr HKCU "Software\WeaveOffice" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WeaveOffice" "DisplayName" "Weave Office (${ARCH})"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WeaveOffice" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WeaveOffice" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WeaveOffice" "QuietUninstallString" '$\"$INSTDIR\Uninstall.exe$\" /S'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WeaveOffice" "DisplayIcon" "$INSTDIR\weave-office.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WeaveOffice" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WeaveOffice" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WeaveOffice" "NoRepair" 1
  ${If} $Relaunch == 1
    Exec '"$INSTDIR\weave-office.exe"'
  ${EndIf}
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  Delete "$SMPROGRAMS\Weave Office.lnk"
  Delete "$INSTDIR\weave-office.exe"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir /r "$INSTDIR\licenses"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\WeaveOffice"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WeaveOffice"
SectionEnd
