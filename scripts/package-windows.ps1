<#
.SYNOPSIS
Validate a Windows CEF bundle and pack it into a distributable zip.

.DESCRIPTION
The zip carries the bundle's contents at its root, so unpacking it anywhere
yields a runnable zz.exe. Scoop installs it exactly that way, and the Inno
Setup installer packs the same directory.

.EXAMPLE
powershell.exe -File scripts\package-windows.ps1 dist\zz dist\zz-windows.zip
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$BundleDir,
    [Parameter(Mandatory = $true)][string]$Output
)

$ErrorActionPreference = 'Stop'

function Die($message) {
    throw "error: $message"
}

if (-not (Test-Path -LiteralPath $BundleDir -PathType Container)) {
    Die "CEF bundle directory does not exist: $BundleDir"
}
$BundleDir = (Resolve-Path -LiteralPath $BundleDir).Path

# A subset of what `cargo xtask verify-cef-bundle` checks: enough to catch a
# half-copied bundle without duplicating xtask's layout decisions here.
foreach ($required in 'zz.exe', 'libcef.dll', 'icudtl.dat', 'resources.pak',
    'locales\en-US.pak', 'CREDITS.html', 'CEF_LICENSE.txt') {
    $path = Join-Path $BundleDir $required
    if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or (Get-Item -LiteralPath $path).Length -eq 0) {
        Die "CEF bundle file is missing or empty: $required"
    }
}

$outputName = Split-Path -Leaf $Output
$outputDir = Split-Path -Parent $Output
if (-not $outputDir) {
    $outputDir = '.'
}
if (-not (Test-Path -LiteralPath $outputDir)) {
    New-Item -ItemType Directory -Path $outputDir | Out-Null
}
$Output = Join-Path (Resolve-Path -LiteralPath $outputDir).Path $outputName
if (Test-Path -LiteralPath $Output) {
    Remove-Item -LiteralPath $Output -Force
}

# 7-Zip ships with the GitHub runner image and with most dev setups, and it
# packs the ~1 GB bundle several times faster than Compress-Archive. The
# built-in is the fallback so the script works on a bare Windows install.
$sevenZip = Get-Command 7z -ErrorAction SilentlyContinue
if ($sevenZip) {
    & $sevenZip.Path a -tzip -mx=5 -bso0 -bsp0 -- $Output "$BundleDir\*"
    if ($LASTEXITCODE -ne 0) {
        Die "7z failed with exit code $LASTEXITCODE"
    }
} else {
    Compress-Archive -Path (Join-Path $BundleDir '*') -DestinationPath $Output -CompressionLevel Optimal
}

if (-not (Test-Path -LiteralPath $Output -PathType Leaf) -or (Get-Item -LiteralPath $Output).Length -eq 0) {
    Die "packaging did not produce an archive: $Output"
}

Write-Host "Windows archive ready: $Output"
