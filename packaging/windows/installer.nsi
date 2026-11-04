Unicode true
!include "MUI2.nsh"
!include "x64.nsh"

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
FunctionEnd

Section "Weave Office"
  SetShellVarContext current
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
