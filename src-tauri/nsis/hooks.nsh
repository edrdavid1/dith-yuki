; Tauri's installer template hardcodes the main executable as the icon for every
; file association, so the per-extension icons are rewritten here after the
; associations have been registered. The class names must match
; bundle.fileAssociations[].name in tauri.conf.json, and the .ico files are
; shipped as resources by tauri.windows.conf.json.
;
; APP_UNASSOCIATE removes the whole class key on uninstall, so no cleanup hook
; is needed.

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr SHCTX "Software\Classes\Dither Project\DefaultIcon" "" "$INSTDIR\proj-icon.ico"
  WriteRegStr SHCTX "Software\Classes\Dither Pattern\DefaultIcon" "" "$INSTDIR\pattern-icon.ico"
  !insertmacro UPDATEFILEASSOC
!macroend
