# zz PowerShell shell integration. This file is generated into a private cache
# and dot-sourced by the startup command line zz hands to pwsh/powershell.exe.
#
# zz never passes -NoProfile, so PowerShell has already run the user's $PROFILE
# chain by the time this file loads: the hooks below wrap whatever the profile
# installed rather than replacing or re-sourcing it.
#
# Every sequence emitted here matches the bash/zsh integration byte for byte, so
# the terminal needs no PowerShell-specific parsing:
#   OSC 2    "\e]2;<title>\a"             window title (shell name, then command)
#   OSC 7    "\e]7;file://<host><cwd>\a"  working directory
#   DECSCUSR "\e[0 q"                     restore the configured cursor style

if (Test-Path -LiteralPath 'Variable:__ZZ_TITLE_INTEGRATION_INSTALLED') { return }
$global:__ZZ_TITLE_INTEGRATION_INSTALLED = 1
$global:__ZZ_PREEXEC_INSTALLED = 0

# Windows PowerShell 5.1 has no "`e" escape, so the control bytes are named once.
$global:__ZZ_ESC = [char]27
$global:__ZZ_BEL = [char]7
# The bash/zsh precmd titles the tab with the shell's own name; the process name
# is the PowerShell equivalent of ${BASH##*/} / ${ZSH_NAME:-zsh}.
$global:__ZZ_SHELL_NAME = 'powershell'
try { $global:__ZZ_SHELL_NAME = (Get-Process -Id $PID).ProcessName } catch { }

function global:__zz_write_title {
    param([string] $Value = '')

    $Value = $Value -replace "[`r`n]", ' '
    $Value = $Value -replace '\p{Cc}', ''
    if ($Value.Length -gt 512) { $Value = $Value.Substring(0, 512) }
    [Console]::Write("$($global:__ZZ_ESC)]2;$Value$($global:__ZZ_BEL)")
}

function global:__zz_write_working_directory {
    # Registry/certificate locations have no filesystem path to report.
    if ($PWD.Provider.Name -ne 'FileSystem') { return }

    $value = $PWD.ProviderPath
    $value = $value -replace "[`r`n]", ''
    $value = $value -replace '\p{Cc}', ''
    if ($value.Length -gt 4096) { $value = $value.Substring(0, 4096) }
    # file:// wants forward slashes and a rooted path: C:\src -> /C:/src.
    $value = $value -replace '\\', '/'
    if (-not $value.StartsWith('/')) { $value = '/' + $value }
    $machine = [System.Environment]::MachineName
    [Console]::Write("$($global:__ZZ_ESC)]7;file://$machine$value$($global:__ZZ_BEL)")
}

function global:__zz_title_precmd {
    __zz_write_working_directory
    __zz_write_title $global:__ZZ_SHELL_NAME
    # DECSCUSR default: the prompt restores the configured cursor style instead of
    # imposing one, so a program that exits without resetting cannot keep its shape.
    [Console]::Write("$($global:__ZZ_ESC)[0 q")
}

function global:__zz_title_preexec {
    param([string] $Command = '')

    # An empty or blank line runs nothing, so it must not retitle the tab the
    # way an executed command does.
    if ([string]::IsNullOrWhiteSpace($Command)) { return }
    [Console]::Write("$($global:__ZZ_ESC)[0 q")
    __zz_write_title $Command
}

# PowerShell has no preexec hook. PSReadLine owns PSConsoleHostReadLine, which
# the console host calls to read each command line, so wrapping it is the only
# place that runs after the user accepts a line and before it executes.
# Installation is deferred to the first prompt because the host imports
# PSReadLine after this script runs, exactly like the zsh hooks defer to the
# first precmd so prompt frameworks finish first.
function global:__zz_install_preexec_hook {
    if ($global:__ZZ_PREEXEC_INSTALLED) { return }
    if (-not (Test-Path -LiteralPath 'Function:PSConsoleHostReadLine')) { return }

    $global:__ZZ_PREEXEC_INSTALLED = 1
    $global:__ZZ_READ_LINE_ORIGINAL = $function:PSConsoleHostReadLine
    function global:PSConsoleHostReadLine {
        $__zz_line = & $global:__ZZ_READ_LINE_ORIGINAL
        try { __zz_title_preexec $__zz_line } catch { }
        $__zz_line
    }
}

if (Test-Path -LiteralPath 'Function:prompt') {
    $global:__ZZ_PROMPT_ORIGINAL = $function:prompt
} else {
    $global:__ZZ_PROMPT_ORIGINAL = {
        "PS $($ExecutionContext.SessionState.Path.CurrentLocation)$('>' * ($NestedPromptLevel + 1)) "
    }
}

function global:prompt {
    # The previous prompt runs first and untouched: anything zz does ahead of it
    # would overwrite the $? and $LASTEXITCODE the user's prompt reads.
    $__zz_rendered = & $global:__ZZ_PROMPT_ORIGINAL
    try {
        __zz_install_preexec_hook
        __zz_title_precmd
    } catch { }
    $__zz_rendered
}

Remove-Item -LiteralPath 'Env:ZZ_SHELL_INTEGRATION_ACTIVE' -ErrorAction SilentlyContinue
