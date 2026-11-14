# Placeholder: package the --no-bundle Windows exe into an MSIX for Store submission.
# Spike checklist (see docs/git_docs/git-vers-spec.md §12):
#   - AppxManifest.xml + required icon sizes
#   - WebView2 dependency / bootstrapper
#   - makeappx pack
param(
  [string]$ExePath = "src-tauri/target/release/dither.exe",
  [string]$OutDir = "dist/msix"
)

Write-Error "package-msix.ps1 is not implemented yet (MS Store spike). Exe expected at: $ExePath → $OutDir"
exit 1
