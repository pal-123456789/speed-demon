#Requires -Version 5.1
<#
.SYNOPSIS
    Run the REAL scoring protocol locally, on Linux, and get the M axis.

.DESCRIPTION
    The M axis (peak memory, 20% of K, capped at 50x) cannot be measured with
    wall-clock tooling. It is the container's cgroup v2 `memory.peak`, read on
    the host after the container exits, and it counts allocations, /tmp writes
    and first-touched pages -- none of which a Windows process can report for a
    container it did not create.

    This host has a full Linux environment in WSL Ubuntu with Docker on cgroup
    v2, so the real thing is available here after all. This script:

      1. builds the Rust harness as a LINUX ELF inside WSL, into its own target
         directory, so the Windows PE artefacts the PowerShell gates use are
         left untouched (they are separate binaries with different names, and
         `tooling/ci.sh` picks the host-native one);
      2. builds the track image the way the board does -- offline, from the
         digest-pinned base images;
      3. runs `sd-harness bench` inside WSL with the board's exact `docker run`
         flags, so the harness can read the cgroup peak itself;
      4. reports T and M, the four gates, and appends to
         verification/ledger.jsonl for PERF.md to be generated from.

    Without this, every rung measured on this machine reports
    "peak memory : n/a (run inside a container to read cgroup memory.peak)".

.PARAMETER Mode
    bench (default, the measurement protocol) or verify (the gate checks only).

.PARAMETER Track
    a, b, c or d.

.PARAMETER Runs
    Number of scored runs. The harness requires at least 3 (it discards the
    highest and lowest); the organizers use 5, which is the default.

.PARAMETER Label
    Ledger label for this rung, e.g. "arena" or "swar-delim". Defaults to "wsl".

.PARAMETER BuildOnly
    Build the Linux harness and the image, then stop.

.PARAMETER NoLedger
    Do not write to verification/ledger.jsonl (useful for exploratory runs).

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tooling\wsl-harness.ps1 -Track a -Label baseline
#>
[CmdletBinding()]
param(
    [ValidateSet("bench", "verify")]
    [string]$Mode = "bench",
    [ValidateSet("a", "b", "c", "d")]
    [string]$Track = "a",
    [int]$Runs = 5,
    [string]$Label = "wsl",
    [switch]$BuildOnly,
    [switch]$NoLedger
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
$TL   = $Track.ToUpper()

# The repository path as WSL sees it (/mnt/<drive>/<rest>).
$drive = $Root.Substring(0, 1).ToLower()
$rest  = $Root.Substring(2) -replace '\\', '/'
$WslRoot = "/mnt/$drive$rest"

$TargetDir = "verification/harness/target-linux"
$Harness   = "$TargetDir/release/sd-harness"
$Image     = "speeddemon-wsl/track-$Track"

function Invoke-Wsl {
    param([string]$Command, [switch]$AllowFail)
    # WSL prints "Failed to start the systemd user session" on STDERR on this
    # host even when the command succeeds, and $ErrorActionPreference = "Stop"
    # turns a native command's stderr into a TERMINATING error -- so the first
    # version of this script died before it ran anything at all. This is the
    # same PowerShell 5.1 quirk that broke verification/verify.ps1 the same way,
    # and the guard has to be repeated wherever a native tool writes to stderr.
    # PowerShell 7 adds $PSNativeCommandUseErrorActionPreference, which is not
    # available in 5.1, so the try/finally is the portable form.
    $eap = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $out = & wsl -d Ubuntu -- bash -lc $Command 2>&1
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $eap
    }
    if ($code -ne 0 -and -not $AllowFail) {
        $out | ForEach-Object { "    $_" }
        throw "WSL command failed (exit $code): $Command"
    }
    return @{ Output = $out; Code = $code }
}

Write-Host ""
Write-Host "=== LOCAL SCORING PROTOCOL (Linux, cgroup v2, real memory.peak) ===" -ForegroundColor Cyan
Write-Host "  repo   : $WslRoot" -ForegroundColor Gray
Write-Host "  mode   : $Mode      track: $TL      runs: $Runs      label: $Label" -ForegroundColor Gray

# ── 0. is the toolchain there? ───────────────────────────────────────────────
$probe = Invoke-Wsl -AllowFail "command -v cargo >/dev/null && command -v docker >/dev/null && echo ready"
if ($probe.Output -notcontains "ready") {
    throw "WSL Ubuntu needs both cargo and docker on PATH. Install rustup and docker, then retry."
}

# ── 1. build the Linux harness, in its own target dir ────────────────────────
Write-Host ""
Write-Host "--- 1. build the Linux harness (own target dir; the PE artefacts are untouched) ---" -ForegroundColor Cyan
$b = Invoke-Wsl "cd '$WslRoot' && CARGO_TARGET_DIR=$TargetDir cargo build --release --offline --manifest-path verification/harness/Cargo.toml 2>&1 | tail -3"
$b.Output | ForEach-Object { "    $_" }

# ── 2. build the track image the way the board does ──────────────────────────
Write-Host ""
Write-Host "--- 2. docker build --network=none -t $Image track-$Track ---" -ForegroundColor Cyan
$i = Invoke-Wsl "cd '$WslRoot' && docker build --network=none -q -t $Image track-$Track 2>&1 | tail -3"
$i.Output | ForEach-Object { "    $_" }
$size = Invoke-Wsl -AllowFail "docker image inspect -f '{{.Size}}' $Image"
if ($size.Code -eq 0) {
    $mb = [math]::Round([double]($size.Output | Select-Object -First 1) / 1MB, 1)
    Write-Host "    image is $mb MB  (cap is 2048 MB)" -ForegroundColor Gray
}

if ($BuildOnly) {
    Write-Host ""
    Write-Host "BuildOnly: stopping here." -ForegroundColor Yellow
    exit 0
}

# ── 3. run the protocol ──────────────────────────────────────────────────────
# --data and --out are passed as ABSOLUTE WSL paths. The harness bind-mounts
# them into the container, and Docker refuses a relative source: it reads
# `verification/data/track-a` as a named volume and fails with
#   "verification/data/track-a" includes invalid characters for a local volume name
# (exit 125). Given no --data/--out at all the harness derives absolute defaults
# from the repository root itself, which is why they are built from $WslRoot
# here rather than repeated as relative strings.
$dataWsl = "$WslRoot/verification/data/track-$Track"
$outWsl  = "$WslRoot/verification/out/track-$Track"

$ledgerArg = if ($NoLedger) {
    "--ledger /tmp/sd-wsl-$Track.jsonl"
} else {
    "--ledger $WslRoot/verification/ledger.jsonl"
}

Write-Host ""
Write-Host "--- 3. $($Mode): the board's docker run flags + cgroup memory.peak ---" -ForegroundColor Cyan
$cmd = "cd '$WslRoot' && ./$Harness $Mode --track $TL --runs $Runs " +
       "--image $Image --data $dataWsl --out $outWsl --label $Label $ledgerArg"
$r = Invoke-Wsl -AllowFail $cmd
$r.Output | ForEach-Object { "  $_" }

Write-Host ""
if (-not $NoLedger) {
    Write-Host "Ledger appended. Regenerate the docs with: python tooling/gen_docs.py" -ForegroundColor Green
} else {
    Write-Host "(--NoLedger: nothing written to the committed ledger)" -ForegroundColor Yellow
}
exit $r.Code
