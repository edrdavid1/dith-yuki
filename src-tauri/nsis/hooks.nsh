; Tauri's installer template hardcodes the main executable as the icon for every
; file association, so the per-extension icons are rewritten here after the
; associations have been registered. The class names must match
; bundle.fileAssociations[].name in tauri.conf.json, and the .ico files are
; shipped as resources by tauri.windows.conf.json.
;
; Thumbnail and preview handler: keys come from platform/windows/registry_keys.nsh
; (generated from dither-shell registry_keys.rs). The DLL is copied to
; $INSTDIR\shell\<version>\ so Explorer can keep the previous file mapped.
;
; APP_UNASSOCIATE removes the whole class key on uninstall; we also clean CLSID.
;
; --- Installer / uninstaller copy (SPEC §3; no custom .nsi template) ---
; This file is !include'd before Tauri !define's VERSION / PRODUCTNAME and before
; !insertmacro MUI_PAGE_*. MUI_* !defines here therefore win. Use $${VERSION} so
; the symbol expands later (when VERSION exists), not at include time.
; Do NOT set MUI_FINISHPAGE_RUN — Tauri wires RunMainBinary → dither.exe.

!include "FileFunc.nsh"

; Welcome
!define MUI_WELCOMEPAGE_TITLE "Welcome to Dither Yuki"
!define MUI_WELCOMEPAGE_TEXT "This will install Dither Yuki $${VERSION} on your computer.$\r$\n$\r$\nDither Yuki turns your images into retro-styled pixel art — Bayer patterns, error diffusion, and hardware-accurate palettes from classic machines.$\r$\n$\r$\nClick Next to continue."

; License / user agreement (page only appears when bundle.licenseFile is set)
!define MUI_LICENSEPAGE_TEXT_TOP "Please review the End User Agreement and software license before installing Dither Yuki."
!define MUI_LICENSEPAGE_TEXT_BOTTOM "If you accept the terms of the agreement, click I Agree to continue. You must accept the agreement to install Dither Yuki."
!define MUI_LICENSEPAGE_BUTTON "I Agree"

; Directory
!define MUI_DIRECTORYPAGE_TEXT_TOP "Setup will install Dither Yuki in the following folder. To install in a different folder, click Browse and select another folder."

; Finish (run checkbox label only — binary launch stays Tauri's RunMainBinary)
!define MUI_FINISHPAGE_TITLE "Dither Yuki is ready"
!define MUI_FINISHPAGE_TEXT "Setup has finished installing Dither Yuki on your computer.$\r$\n$\r$\nClick Finish to close Setup."
!define MUI_FINISHPAGE_RUN_TEXT "Launch Dither Yuki"

; Uninstall confirm — reassure that user documents are kept
!define MUI_UNCONFIRMPAGE_TEXT_TOP "This will remove Dither Yuki from your computer. Your saved .dyproj files and exported images will not be deleted."

!include "${__FILEDIR__}\..\..\platform\windows\registry_keys.nsh"

Var DitherShellDll

; Pick an arch-specific DLL when the installer shipped one. Otherwise use
; dither_shell.dll from the bundle (the arch the installer itself was built for).
Function DitherSelectShellDll
  StrCpy $DitherShellDll "$INSTDIR\dither_shell.dll"
  Push $R7
  Push $R8
  Push $R9
  System::Call "kernel32::GetCurrentProcess() p .R9"
  System::Call "kernel32::IsWow64Process2(p R9, *i .R8, *i .R7)"
  IntCmp $R7 43620 dither_is_arm dither_try_x64 dither_try_x64
  dither_is_arm:
    IfFileExists "$INSTDIR\dither_shell_arm64.dll" 0 dither_dll_done
      StrCpy $DitherShellDll "$INSTDIR\dither_shell_arm64.dll"
    Goto dither_dll_done
  dither_try_x64:
    IntCmp $R7 34404 0 dither_dll_done dither_dll_done
      IfFileExists "$INSTDIR\dither_shell_x64.dll" 0 dither_dll_done
        StrCpy $DitherShellDll "$INSTDIR\dither_shell_x64.dll"
  dither_dll_done:
  Pop $R9
  Pop $R8
  Pop $R7
FunctionEnd

; Close a running copy before overwriting dither.exe (in-app update / reinstall).
!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToLog 'taskkill /F /IM "dither.exe" /T'
  Pop $0
  Sleep 800
!macroend

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr SHCTX "Software\Classes\Dither Project\DefaultIcon" "" "$INSTDIR\proj-icon.ico"
  WriteRegStr SHCTX "Software\Classes\Dither Pattern\DefaultIcon" "" "$INSTDIR\pattern-icon.ico"

  Call DitherSelectShellDll
  IfFileExists "$DitherShellDll" 0 dither_shell_skip
    CreateDirectory "$INSTDIR\shell\${VERSION}"
    CopyFiles /SILENT "$DitherShellDll" "$INSTDIR\shell\${VERSION}\dither_shell.dll"
    StrCpy $DitherShellDll "$INSTDIR\shell\${VERSION}\dither_shell.dll"
    !insertmacro DITHER_WRITE_SHELL_KEYS

    Push $R8
    Push $R9
    FindFirst $R8 $R9 "$INSTDIR\shell\*"
    dither_clean_loop:
      StrCmp $R9 "" dither_clean_done
      StrCmp $R9 "." dither_clean_next
      StrCmp $R9 ".." dither_clean_next
      StrCmp $R9 "${VERSION}" dither_clean_next
      RMDir /r "$INSTDIR\shell\$R9"
      IfErrors 0 dither_clean_next
        System::Call 'kernel32::MoveFileEx(t "$INSTDIR\shell\$R9", i 0, i 4)'
      dither_clean_next:
      FindNext $R8 $R9
      Goto dither_clean_loop
    dither_clean_done:
    FindClose $R8
    Pop $R9
    Pop $R8
  dither_shell_skip:

  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
  !insertmacro UPDATEFILEASSOC
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro DITHER_DELETE_SHELL_KEYS
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro DITHER_DELETE_SHELL_KEYS
  RMDir /r "$INSTDIR\shell"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
  !insertmacro UPDATEFILEASSOC
!macroend
