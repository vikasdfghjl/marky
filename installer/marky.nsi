; Marky's per-user installer. Built by scripts/build-installer.ps1 with /DVERSION, /DVERSION4,
; /DSRCDIR and /DOUTDIR. Ships marky.exe; no admin prompt.
Unicode true
!include "MUI2.nsh"

Name "Marky"
OutFile "${OUTDIR}\Marky_${VERSION}_x64-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\Marky"
InstallDirRegKey HKCU "Software\Marky" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma

VIProductVersion "${VERSION4}"
VIAddVersionKey "ProductName" "Marky"
VIAddVersionKey "FileDescription" "Marky installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "MIT"

!define MUI_ICON "${SRCDIR}\assets\marky.ico"
!define MUI_UNICON "${SRCDIR}\assets\marky.ico"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_LANGUAGE "English"

Section "Marky"
  SetOutPath "$INSTDIR"
  File /oname=marky.exe "${SRCDIR}\target\release\marky.exe"
  File "/oname=LICENSE.txt" "${SRCDIR}\LICENSE"
  File "/oname=THIRD_PARTY_NOTICES.txt" "${SRCDIR}\THIRD_PARTY_NOTICES.txt"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateDirectory "$SMPROGRAMS\Marky"
  CreateShortcut "$SMPROGRAMS\Marky\Marky.lnk" "$INSTDIR\marky.exe"
  CreateShortcut "$SMPROGRAMS\Marky\Uninstall Marky.lnk" "$INSTDIR\uninstall.exe"

  WriteRegStr HKCU "Software\Marky" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Marky" "DisplayName" "Marky"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Marky" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Marky" "DisplayIcon" "$INSTDIR\marky.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Marky" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Marky" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Marky" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Marky" "NoRepair" 1

  ; Register with "Open with" for Markdown and text files, without taking over the default.
  WriteRegStr HKCU "Software\Classes\Marky.Document" "" "Marky document"
  WriteRegStr HKCU "Software\Classes\Marky.Document\DefaultIcon" "" "$INSTDIR\marky.exe,0"
  WriteRegStr HKCU "Software\Classes\Marky.Document\shell\open\command" "" '"$INSTDIR\marky.exe" "%1"'
  WriteRegStr HKCU "Software\Classes\Applications\marky.exe\shell\open\command" "" '"$INSTDIR\marky.exe" "%1"'
  WriteRegStr HKCU "Software\Classes\Applications\marky.exe\SupportedTypes" ".md" ""
  WriteRegStr HKCU "Software\Classes\Applications\marky.exe\SupportedTypes" ".markdown" ""
  WriteRegStr HKCU "Software\Classes\Applications\marky.exe\SupportedTypes" ".txt" ""
  WriteRegStr HKCU "Software\Classes\.md\OpenWithProgids" "Marky.Document" ""
  WriteRegStr HKCU "Software\Classes\.markdown\OpenWithProgids" "Marky.Document" ""
  WriteRegStr HKCU "Software\Classes\.txt\OpenWithProgids" "Marky.Document" ""
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\marky.exe"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\THIRD_PARTY_NOTICES.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Marky\Marky.lnk"
  Delete "$SMPROGRAMS\Marky\Uninstall Marky.lnk"
  RMDir "$SMPROGRAMS\Marky"

  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Marky"
  DeleteRegKey HKCU "Software\Marky"
  DeleteRegKey HKCU "Software\Classes\Marky.Document"
  DeleteRegKey HKCU "Software\Classes\Applications\marky.exe"
  DeleteRegValue HKCU "Software\Classes\.md\OpenWithProgids" "Marky.Document"
  DeleteRegValue HKCU "Software\Classes\.markdown\OpenWithProgids" "Marky.Document"
  DeleteRegValue HKCU "Software\Classes\.txt\OpenWithProgids" "Marky.Document"
SectionEnd
