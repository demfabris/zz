[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere)) {
    throw 'Install Visual Studio Build Tools with the Desktop development with C++ workload and a Windows SDK.'
}
$visualStudio = & $vswhere -latest -products '*' -property installationPath
if (-not $visualStudio) {
    throw 'No Visual Studio installation was found.'
}
$devShell = Join-Path $visualStudio 'Common7\Tools\Launch-VsDevShell.ps1'
& $devShell -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
$linker = Join-Path $env:VCToolsInstallDir 'bin\HostX64\x64\link.exe'
if (-not (Test-Path -LiteralPath $linker)) {
    throw 'The Visual Studio x64 C++ tools are missing.'
}
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = $linker

$git = (Get-Command git.exe -ErrorAction Stop).Source
$gitBash = Join-Path (Split-Path -Parent (Split-Path -Parent $git)) 'bin\bash.exe'
if (-not (Test-Path -LiteralPath $gitBash)) {
    throw 'Git for Windows with Git Bash is required.'
}
$env:Path = (Split-Path -Parent $gitBash) + ';' + $env:Path
foreach ($tool in 'cargo', 'zig', 'cmake', 'ninja', 'just', 'cl', 'rc') {
    Get-Command $tool -ErrorAction Stop | Out-Null
}

Push-Location $repoRoot
try {
    & just build windows
    $buildExitCode = $LASTEXITCODE
} finally {
    Pop-Location
}
exit $buildExitCode
