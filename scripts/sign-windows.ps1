<#
.SYNOPSIS
Authenticode-sign the Windows executables and the installer.

.DESCRIPTION
Reads the certificate from the environment so the release workflow can hand it
over as a secret and never write it to the repository:

  WINDOWS_CERTIFICATE_PFX_BASE64  base64 of the PKCS#12 file (required)
  WINDOWS_CERTIFICATE_PASSWORD    its password (required)
  WINDOWS_TIMESTAMP_URL           RFC 3161 timestamp server (optional)

No certificate exists yet. Provision one before a signed release: OV and EV
code-signing certificates have been hardware-bound since June 2023, so the
realistic sources are a cloud signing service (Azure Trusted Signing, SignPath,
ssl.com eSigner) or a token whose provider exposes a PFX for CI. Every one of
them still drives signtool.exe, so only how the key material arrives changes.

.EXAMPLE
powershell.exe -File scripts\sign-windows.ps1 dist\zz\zz.exe dist\zz\zz.dll
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)][string[]]$Paths
)

$ErrorActionPreference = 'Stop'

function Die($message) {
    throw "error: $message"
}

$pfxBase64 = $env:WINDOWS_CERTIFICATE_PFX_BASE64
$password = $env:WINDOWS_CERTIFICATE_PASSWORD
$timestampUrl = $env:WINDOWS_TIMESTAMP_URL
if (-not $timestampUrl) {
    $timestampUrl = 'http://timestamp.digicert.com'
}

if (-not $pfxBase64) {
    Die 'WINDOWS_CERTIFICATE_PFX_BASE64 is unset; nothing to sign with'
}
if (-not $password) {
    Die 'WINDOWS_CERTIFICATE_PASSWORD is unset'
}

foreach ($path in $Paths) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        Die "file to sign does not exist: $path"
    }
}

# signtool.exe ships with the Windows SDK and is not on PATH there, so take the
# newest x64 copy the SDK installed.
$signtool = Get-Command signtool.exe -ErrorAction SilentlyContinue
if ($signtool) {
    $signtoolPath = $signtool.Path
} else {
    $signtoolPath = Get-ChildItem -Path 'C:\Program Files (x86)\Windows Kits\10\bin' `
        -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -like '*\x64\*' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}
if (-not $signtoolPath) {
    Die 'signtool.exe not found; install the Windows SDK signing tools'
}

$certificate = Join-Path ([System.IO.Path]::GetTempPath()) "zz-signing-$([guid]::NewGuid()).pfx"
try {
    [System.IO.File]::WriteAllBytes($certificate, [System.Convert]::FromBase64String($pfxBase64))

    & $signtoolPath sign /fd sha256 /td sha256 /tr $timestampUrl `
        /f $certificate /p $password $Paths
    if ($LASTEXITCODE -ne 0) {
        Die "signtool sign failed with exit code $LASTEXITCODE"
    }

    & $signtoolPath verify /pa /v $Paths
    if ($LASTEXITCODE -ne 0) {
        Die "signtool verify failed with exit code $LASTEXITCODE"
    }
} finally {
    Remove-Item -LiteralPath $certificate -Force -ErrorAction SilentlyContinue
}

Write-Host "signed: $($Paths -join ', ')"
