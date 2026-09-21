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

!include "FileFunc.nsh"

!define DITHER_CLSID_THUMB "{BC7D0A00-220F-46DD-AAA8-C754864EE648}"
!define DITHER_SHELL_THUMB "{E357FCCD-A995-4576-B01F-234630154E96}"

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
