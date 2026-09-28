; Generated from platform/windows/dither-shell/src/registry_keys.rs
; Regenerate: cargo run -p dither-shell-xtask
; Do not edit by hand.

!define DITHER_CLSID_THUMB "{BC7D0A00-220F-46DD-AAA8-C754864EE648}"
!define DITHER_CLSID_PREVIEW "{C8BC1EC9-FB9C-4374-9925-D82D2819A965}"
!define DITHER_SHELL_THUMB "{E357FCCD-A995-4576-B01F-234630154E96}"
!define DITHER_SHELL_PREVIEW "{8895b1c6-b41f-4c1c-a562-0d564250836f}"
!define DITHER_PREVIEW_APPID "{6d2b5079-2f0b-48dd-ab7f-97cec514d30b}"

!macro DITHER_WRITE_SHELL_KEYS
  WriteRegStr SHCTX "Software\Classes\CLSID\{BC7D0A00-220F-46DD-AAA8-C754864EE648}" "" "Dither Thumbnail Provider"
  WriteRegStr SHCTX "Software\Classes\CLSID\{BC7D0A00-220F-46DD-AAA8-C754864EE648}\InprocServer32" "" "$DitherShellDll"
  WriteRegStr SHCTX "Software\Classes\CLSID\{BC7D0A00-220F-46DD-AAA8-C754864EE648}\InprocServer32" "ThreadingModel" "Apartment"
  WriteRegStr SHCTX "Software\Classes\CLSID\{C8BC1EC9-FB9C-4374-9925-D82D2819A965}" "" "Dither Preview Handler"
  WriteRegStr SHCTX "Software\Classes\CLSID\{C8BC1EC9-FB9C-4374-9925-D82D2819A965}\InprocServer32" "" "$DitherShellDll"
  WriteRegStr SHCTX "Software\Classes\CLSID\{C8BC1EC9-FB9C-4374-9925-D82D2819A965}\InprocServer32" "ThreadingModel" "Apartment"
  WriteRegStr SHCTX "Software\Classes\CLSID\{C8BC1EC9-FB9C-4374-9925-D82D2819A965}" "AppID" "{6d2b5079-2f0b-48dd-ab7f-97cec514d30b}"
  WriteRegStr SHCTX "Software\Classes\.dyproj\ShellEx\{E357FCCD-A995-4576-B01F-234630154E96}" "" "{BC7D0A00-220F-46DD-AAA8-C754864EE648}"
  WriteRegStr SHCTX "Software\Classes\Dither Project\ShellEx\{E357FCCD-A995-4576-B01F-234630154E96}" "" "{BC7D0A00-220F-46DD-AAA8-C754864EE648}"
  WriteRegStr SHCTX "Software\Classes\.dyproj\ShellEx\{8895b1c6-b41f-4c1c-a562-0d564250836f}" "" "{C8BC1EC9-FB9C-4374-9925-D82D2819A965}"
  WriteRegStr SHCTX "Software\Classes\Dither Project\ShellEx\{8895b1c6-b41f-4c1c-a562-0d564250836f}" "" "{C8BC1EC9-FB9C-4374-9925-D82D2819A965}"
  WriteRegStr SHCTX "Software\Classes\.dyuki\ShellEx\{E357FCCD-A995-4576-B01F-234630154E96}" "" "{BC7D0A00-220F-46DD-AAA8-C754864EE648}"
  WriteRegStr SHCTX "Software\Classes\Dither Pattern\ShellEx\{E357FCCD-A995-4576-B01F-234630154E96}" "" "{BC7D0A00-220F-46DD-AAA8-C754864EE648}"
  WriteRegStr SHCTX "Software\Classes\.dyuki\ShellEx\{8895b1c6-b41f-4c1c-a562-0d564250836f}" "" "{C8BC1EC9-FB9C-4374-9925-D82D2819A965}"
  WriteRegStr SHCTX "Software\Classes\Dither Pattern\ShellEx\{8895b1c6-b41f-4c1c-a562-0d564250836f}" "" "{C8BC1EC9-FB9C-4374-9925-D82D2819A965}"
  WriteRegStr SHCTX "Software\Microsoft\Windows\CurrentVersion\PreviewHandlers" "{C8BC1EC9-FB9C-4374-9925-D82D2819A965}" "Dither Preview Handler"
!macroend

!macro DITHER_DELETE_SHELL_KEYS
  DeleteRegKey SHCTX "Software\Classes\CLSID\{BC7D0A00-220F-46DD-AAA8-C754864EE648}"
  DeleteRegKey SHCTX "Software\Classes\CLSID\{C8BC1EC9-FB9C-4374-9925-D82D2819A965}"
  DeleteRegKey SHCTX "Software\Classes\.dyproj\ShellEx\{E357FCCD-A995-4576-B01F-234630154E96}"
  DeleteRegKey SHCTX "Software\Classes\Dither Project\ShellEx\{E357FCCD-A995-4576-B01F-234630154E96}"
  DeleteRegKey SHCTX "Software\Classes\.dyproj\ShellEx\{8895b1c6-b41f-4c1c-a562-0d564250836f}"
  DeleteRegKey SHCTX "Software\Classes\Dither Project\ShellEx\{8895b1c6-b41f-4c1c-a562-0d564250836f}"
  DeleteRegKey SHCTX "Software\Classes\.dyuki\ShellEx\{E357FCCD-A995-4576-B01F-234630154E96}"
  DeleteRegKey SHCTX "Software\Classes\Dither Pattern\ShellEx\{E357FCCD-A995-4576-B01F-234630154E96}"
  DeleteRegKey SHCTX "Software\Classes\.dyuki\ShellEx\{8895b1c6-b41f-4c1c-a562-0d564250836f}"
  DeleteRegKey SHCTX "Software\Classes\Dither Pattern\ShellEx\{8895b1c6-b41f-4c1c-a562-0d564250836f}"
  DeleteRegValue SHCTX "Software\Microsoft\Windows\CurrentVersion\PreviewHandlers" "{C8BC1EC9-FB9C-4374-9925-D82D2819A965}"
!macroend
