; NSIS template rendered by `dx bundle` (Dioxus 0.7). Adapted from Miko-Kagura packaging/windows.nsi
; (same author, see crates/tk-app/NOTICES-B6.md).
;
; Per-user install, no elevation. The uninstaller removes ONLY the files and folders listed below and never
; touches the data folder %APPDATA%\Tatakai-Next (history, extensions, Link keys, logs).
; PAYLOAD must point at the folder that holds tatakai-next.exe and its runtime files:
;   makensis /DPAYLOAD=<dir> /DWINDOWS_VERSION=0.1.0.0 ...   (dx supplies the {{...}} values)
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "x64.nsh"

!ifndef PAYLOAD
  !error "Pass /DPAYLOAD=<folder with tatakai-next.exe>"
!endif
!ifndef WINDOWS_VERSION
  !define WINDOWS_VERSION "0.1.0.0"
!endif

Unicode true
Name "Tatakai Next"
OutFile "{{output_path}}"
InstallDir "$LOCALAPPDATA\Programs\Tatakai-Next"
InstallDirRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\app.tatakai.own.next" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma
VIProductVersion "${WINDOWS_VERSION}"
VIAddVersionKey "ProductName" "Tatakai Next"
VIAddVersionKey "FileDescription" "Tatakai Next Setup"
VIAddVersionKey "FileVersion" "{{version}}"
VIAddVersionKey "ProductVersion" "{{version}}"
VIAddVersionKey "LegalCopyright" "AGPL-3.0-or-later"

!define REGKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\app.tatakai.own.next"
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_LICENSE "{{license}}"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Spanish"
LangString CloseApp ${LANG_ENGLISH} "Close Tatakai Next before continuing."
LangString CloseApp ${LANG_SPANISH} "Cierra Tatakai Next antes de continuar."
LangString NeedWebview ${LANG_ENGLISH} "WebView2 could not be installed. Check your connection and try again."
LangString NeedWebview ${LANG_SPANISH} "No se pudo instalar WebView2. Revisa tu red y vuelve a intentarlo."

Function .onInit
    SetShellVarContext current
    ${IfNot} ${RunningX64}
        Abort "This package requires Windows x64."
    ${EndIf}
    SetRegView 64
    !insertmacro MUI_LANGDLL_DISPLAY
FunctionEnd

Function un.onInit
    SetShellVarContext current
    SetRegView 64
FunctionEnd

!macro CheckAppClosedMacro PREFIX
Function ${PREFIX}CheckAppClosed
    IfFileExists "$INSTDIR\tatakai-next.exe" 0 done
    ClearErrors
    FileOpen $0 "$INSTDIR\tatakai-next.exe" a
    ${If} ${Errors}
        MessageBox MB_OK|MB_ICONEXCLAMATION "$(CloseApp)" /SD IDOK
        SetErrorLevel 1
        Abort
    ${EndIf}
    FileClose $0
    done:
FunctionEnd
!macroend
!insertmacro CheckAppClosedMacro ""
!insertmacro CheckAppClosedMacro "un."

Section "Install"
    Call CheckAppClosed
    ; Bootstrap WebView2 only when absent. The Evergreen client key is 32-bit.
    SetRegView 32
    ReadRegStr $1 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
    ${If} $1 == ""
    ${OrIf} $1 == "0.0.0.0"
        ReadRegStr $1 HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
    ${EndIf}
    ${If} $1 == ""
    ${OrIf} $1 == "0.0.0.0"
        {{webview_install_code}}
        ${If} $0 != 0
        ${AndIf} $0 != 3010
            MessageBox MB_OK|MB_ICONSTOP "$(NeedWebview)" /SD IDOK
            SetErrorLevel 1
            Abort
        ${EndIf}
    ${EndIf}
    SetRegView 64
    SetOutPath "$INSTDIR"
    File /r "${PAYLOAD}\*"
    WriteUninstaller "$INSTDIR\uninstall.exe"
    CreateDirectory "$SMPROGRAMS\Tatakai Next"
    CreateShortcut "$SMPROGRAMS\Tatakai Next\Tatakai Next.lnk" "$INSTDIR\tatakai-next.exe"
    CreateShortcut "$DESKTOP\Tatakai Next.lnk" "$INSTDIR\tatakai-next.exe"
    WriteRegStr HKCU "${REGKEY}" "DisplayName" "Tatakai Next"
    WriteRegStr HKCU "${REGKEY}" "DisplayVersion" "{{version}}"
    WriteRegStr HKCU "${REGKEY}" "Publisher" "Tatakai"
    WriteRegStr HKCU "${REGKEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${REGKEY}" "DisplayIcon" "$INSTDIR\tatakai-next.exe"
    WriteRegStr HKCU "${REGKEY}" "UninstallString" '$\"$INSTDIR\uninstall.exe$\"'
    WriteRegStr HKCU "${REGKEY}" "QuietUninstallString" '$\"$INSTDIR\uninstall.exe$\" /S'
    WriteRegDWORD HKCU "${REGKEY}" "NoModify" 1
    WriteRegDWORD HKCU "${REGKEY}" "NoRepair" 1
    ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
    WriteRegDWORD HKCU "${REGKEY}" "EstimatedSize" $0
SectionEnd

; Explicit list: no recursive delete of $INSTDIR itself, so a user-chosen folder with other files survives.
Section "Uninstall"
    Call un.CheckAppClosed
    Delete "$INSTDIR\tatakai-next.exe"
    Delete "$INSTDIR\libmpv-2.dll"
    Delete "$INSTDIR\LICENSE"
    RMDir /r "$INSTDIR\shaders"
    RMDir /r "$INSTDIR\ffmpeg"
    RMDir /r "$INSTDIR\assets"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"
    Delete "$SMPROGRAMS\Tatakai Next\Tatakai Next.lnk"
    RMDir "$SMPROGRAMS\Tatakai Next"
    Delete "$DESKTOP\Tatakai Next.lnk"
    DeleteRegKey HKCU "${REGKEY}"
SectionEnd
