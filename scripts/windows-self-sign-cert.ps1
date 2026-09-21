# Create / use a self-signed Authenticode certificate for Windows alpha (T1).
# Publisher: L'eco non di Bergamo
#
# Usage:
#   pwsh scripts/windows-self-sign-cert.ps1
#   pwsh scripts/windows-self-sign-cert.ps1 -SignPath .\target\...\dither_shell.dll
#   pwsh scripts/windows-self-sign-cert.ps1 -SignPath .\installer.exe -Timestamp
#
# Env out (current session):
#   WINDOWS_CERT_THUMBPRINT
#   WINDOWS_CERT_SUBJECT
#
# The .pfx is written under $env:LOCALAPPDATA\dither-yuki\ (never commit it).

param(
  [string]$SignPath = "",
  [string]$Subject = "CN=L'eco non di Bergamo, O=L'eco non di Bergamo, C=IT",
  [string]$FriendlyName = "L'eco non di Bergamo",
  [switch]$Timestamp
)

$ErrorActionPreference = "Stop"

function Get-OrCreateCert {
  $existing = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert -ErrorAction SilentlyContinue |
    Where-Object { $_.Subject -like "*L'eco non di Bergamo*" -and $_.NotAfter -gt (Get-Date) } |
    Select-Object -First 1
  if ($existing) {
    Write-Host "Reusing cert: $($existing.Thumbprint) ($($existing.Subject))"
    return $existing
  }

  Write-Host "Creating self-signed code-signing certificate…"
  $cert = New-SelfSignedCertificate `
    -Type CodeSigningCert `
    -Subject $Subject `
    -FriendlyName $FriendlyName `
    -CertStoreLocation "Cert:\CurrentUser\My" `
    -KeyExportPolicy Exportable `
    -KeySpec Signature `
    -KeyLength 2048 `
    -HashAlgorithm SHA256 `
    -NotAfter (Get-Date).AddYears(5)

  $dir = Join-Path $env:LOCALAPPDATA "dither-yuki"
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  $pfx = Join-Path $dir "selfsign-codesign.pfx"
  $pass = ConvertTo-SecureString -String "dither-yuki-selfsign" -Force -AsPlainText
  Export-PfxCertificate -Cert $cert -FilePath $pfx -Password $pass | Out-Null
  Write-Host "Exported PFX (local only): $pfx"
  return $cert
}

$cert = Get-OrCreateCert
$env:WINDOWS_CERT_THUMBPRINT = $cert.Thumbprint
$env:WINDOWS_CERT_SUBJECT = $cert.Subject
Write-Host "WINDOWS_CERT_THUMBPRINT=$($cert.Thumbprint)"
Write-Host "WINDOWS_CERT_SUBJECT=$($cert.Subject)"

if ($SignPath -ne "") {
  if (-not (Test-Path $SignPath)) {
    throw "File not found: $SignPath"
  }
  $args = @{
    FilePath = $SignPath
    Certificate = $cert
    HashAlgorithm = "SHA256"
  }
  if ($Timestamp) {
    # Public free TSA; replace with your CA's URL for T2.
    $args["TimestampServer"] = "http://timestamp.digicert.com"
  }
  $sig = Set-AuthenticodeSignature @args
  Write-Host "Signature status: $($sig.Status) ($($sig.StatusMessage))"
  if ($sig.Status -ne "Valid" -and $sig.Status -ne "UnknownError") {
    # Self-signed often reports UnknownError on machines that don't trust the cert — still signed.
    Write-Host "Note: self-signed certs are untrusted by default; Status may not be Valid until the cert is trusted locally."
  }
  Get-AuthenticodeSignature $SignPath | Format-List *
}
