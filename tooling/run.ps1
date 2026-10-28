# run.ps1 — run a tooling shell script from PowerShell without `sh` on PATH.
#
# Windows PowerShell has no `sh` unless Git for Windows' bin directory is on
# PATH, so `sh tooling/ci.sh` fails with "The term 'sh' is not recognized".
# This shim finds a POSIX shell (Git for Windows preferred; see
# tooling/_shell.py for the same order) and forwards every argument.
#
#   .\tooling\run.ps1 ci                     # sh tooling/ci.sh
#   .\tooling\run.ps1 ci --fast              # sh tooling/ci.sh --fast
#   .\tooling\run.ps1 bench-rung.sh a --label rung-01
#   .\tooling\run.ps1 pin-hash s             # wrong on purpose -> usage error
#   .\tooling\run.ps1 shell                  # print the shell it would use
#
# Exit code is the script's exit code, so `$LASTEXITCODE` is meaningful.

param(
    [Parameter(Position = 0)][string]$Name = "",
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Rest
)

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here

function Find-Sh {
    if ($env:SD_SH -and (Test-Path $env:SD_SH)) { return $env:SD_SH }
    foreach ($p in @(
            "$env:ProgramFiles\Git\bin\sh.exe",
            "${env:ProgramFiles(x86)}\Git\bin\sh.exe",
            "$env:ProgramFiles\Git\usr\bin\sh.exe",
            "$env:LOCALAPPDATA\Programs\Git\bin\sh.exe")) {
        if ($p -and (Test-Path $p)) { return $p }
    }
    foreach ($c in @('sh', 'bash', 'wsl')) {
        $cmd = Get-Command $c -ErrorAction SilentlyContinue
        if ($cmd) { return $cmd.Source }
    }
    return $null
}

$sh = Find-Sh
if (-not $sh) {
    Write-Error "no POSIX shell found. Install Git for Windows, or set `$env:SD_SH to a shell path."
    exit 2
}

if ($Name -eq 'shell') { Write-Output $sh; exit 0 }
if (-not $Name) {
    Write-Output "usage: .\tooling\run.ps1 <script> [args...]"
    Write-Output "  e.g. .\tooling\run.ps1 ci --fast"
    Write-Output "       .\tooling\run.ps1 bench-rung.sh a --label rung-01"
    exit 2
}

$target = if ($Name -match '[/\\]') { $Name }
          elseif ($Name -match '\.(sh|ps1)$') { "tooling/$Name" }
          else { "tooling/$Name.sh" }
$full = Join-Path $root $target
if (-not (Test-Path $full)) {
    Write-Error "no such script: $target"
    exit 2
}

Write-Verbose "running: $sh $target $($Rest -join ' ')"
& $sh $target @Rest
exit $LASTEXITCODE
