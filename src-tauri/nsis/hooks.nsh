; Tauri's installer template hardcodes the main executable as the icon for every
; file association, so the per-extension icons are rewritten here after the
; associations have been registered. The class names must match
; bundle.fileAssociations[].name in tauri.conf.json, and the .ico files are
; shipped as resources by tauri.windows.conf.json.
;
; Thumbnail provider: dither_shell.dll is copied to $INSTDIR\shell\<version>\
; and registered under HKCU/HKLM (SHCTX) for .dyproj / .dyuki and ProgIDs.
;
; CLSID {BC7D0A00-220F-46DD-AAA8-C754864EE648} is frozen — never change.
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

!define DITHER_CLSID_THUMB "{BC7D0A00-220F-46DD-AAA8-C754864EE648}"
!define DITHER_SHELL_THUMB "{E357FCCD-A995-4576-B01F-234630154E96}"

; Close a running copy before overwriting dither.exe (in-app update / reinstall).
!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToLog 'taskkill /F /IM "dither.exe" /T'
  Pop $0
  Sleep 800
!macroend

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr SHCTX "Software\Classes\Dither Project\DefaultIcon" "" "$INSTDIR\proj-icon.ico"
  WriteRegStr SHCTX "Software\Classes\Dither Pattern\DefaultIcon" "" "$INSTDIR\pattern-icon.ico"

  ; Versioned shell DLL folder (allows update while Explorer holds the old DLL).
  CreateDirectory "$INSTDIR\shell\${VERSION}"
  ; DLL is expected beside resources as dither_shell.dll when built into the bundle.
  IfFileExists "$INSTDIR\dither_shell.dll" 0 dither_shell_skip
    CopyFiles /SILENT "$INSTDIR\dither_shell.dll" "$INSTDIR\shell\${VERSION}\dither_shell.dll"
    WriteRegStr SHCTX "Software\Classes\CLSID\${DITHER_CLSID_THUMB}" "" "Dither Thumbnail Provider"
    WriteRegStr SHCTX "Software\Classes\CLSID\${DITHER_CLSID_THUMB}\InprocServer32" "" "$INSTDIR\shell\${VERSION}\dither_shell.dll"
    WriteRegStr SHCTX "Software\Classes\CLSID\${DITHER_CLSID_THUMB}\InprocServer32" "ThreadingModel" "Apartment"

    WriteRegStr SHCTX "Software\Classes\.dyproj\ShellEx\${DITHER_SHELL_THUMB}" "" "${DITHER_CLSID_THUMB}"
    WriteRegStr SHCTX "Software\Classes\.dyuki\ShellEx\${DITHER_SHELL_THUMB}" "" "${DITHER_CLSID_THUMB}"
    WriteRegStr SHCTX "Software\Classes\Dither Project\ShellEx\${DITHER_SHELL_THUMB}" "" "${DITHER_CLSID_THUMB}"
    WriteRegStr SHCTX "Software\Classes\Dither Pattern\ShellEx\${DITHER_SHELL_THUMB}" "" "${DITHER_CLSID_THUMB}"
  dither_shell_skip:

  !insertmacro UPDATEFILEASSOC
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegKey SHCTX "Software\Classes\CLSID\${DITHER_CLSID_THUMB}"
  DeleteRegKey SHCTX "Software\Classes\.dyproj\ShellEx\${DITHER_SHELL_THUMB}"
  DeleteRegKey SHCTX "Software\Classes\.dyuki\ShellEx\${DITHER_SHELL_THUMB}"
  DeleteRegKey SHCTX "Software\Classes\Dither Project\ShellEx\${DITHER_SHELL_THUMB}"
  DeleteRegKey SHCTX "Software\Classes\Dither Pattern\ShellEx\${DITHER_SHELL_THUMB}"
  RMDir /r "$INSTDIR\shell"
  !insertmacro UPDATEFILEASSOC
!macroend
