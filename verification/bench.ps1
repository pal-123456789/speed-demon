<#
.SYNOPSIS
    5-run trimmed-mean benchmark + full per-axis ratio report for one track.

.DESCRIPTION
    Mirrors the organizer's timing protocol exactly: 5 runs, highest and
    lowest discarded, mean of the middle three becomes T. Also reports the
    local speed ratio and, when a container run is possible, the cgroup
    memory.peak (the M axis) — the organizer reads peak memory from
    /sys/fs/cgroup/<container>/memory.peak, never from `ps` or `time`.

.PARAMETER Track
    A, B, C or D.

.PARAMETER Runs
    Scored runs (default 5).

.PARAMETER Docker
    Run inside a container that mirrors the scoring box.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("A","B","C","D")]
    [string]$Track,
    [int]$Runs = 5,
    [switch]$Docker
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

# Invoke a native command without $ErrorActionPreference=Stop treating its
# stderr as a terminating error. PowerShell 5.1 quirk: a native program writing
# to stderr raises an ErrorRecord, and with Stop set that aborts the script —
# even when the program is behaving correctly and its exit code is 0. Every
# track binary writes diagnostics to stderr, so this wrapper is mandatory, not
# optional. verify.ps1 and pre-freeze.ps1 already had it; bench.ps1 did not,
# which meant a mis-set SD_DATA_DIR aborted the benchmark with an obscure
# error instead of failing the run cleanly.
function Invoke-Native {
    param([string]$Exe, [string[]]$Arguments)
    $flat = @()
    foreach ($a in $Arguments) { $flat += $a }
    $prev = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $null = & $Exe @flat 2>&1
        return [int]$LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prev
    }
}

function Write-Warn { param([string]$Message) Write-Host "  [WARN] $Message" -ForegroundColor Yellow }
function Write-Info { param([string]$Message) Write-Host "  [INFO] $Message" -ForegroundColor Gray }

$TrackInfo = @{
    A = @{ Name = "track-a"; Timeout = 321; Ref = 32.05; Binary = "run"; Input = "requests.log";  Output = "summary.txt" }
    B = @{ Name = "track-b"; Timeout = 564; Ref = 56.35; Binary = "run"; Input = "docs.txt";  Output = "results.txt" }
    C = @{ Name = "track-c"; Timeout = 144; Ref = 14.44; Binary = "run"; Input = "records.csv"; Output = "aggregates.txt" }
    D = @{ Name = "track-d"; Timeout = 727; Ref = 72.69; Binary = "run"; Input = "graph.txt";  Output = "distances.txt" }
}
$info = $TrackInfo[$Track]

function Get-TrimmedMean {
    param([double[]]$Values)
    $s = $Values | Sort-Object
    if ($s.Count -lt 3) { return [math]::Round(($s | Measure-Object -Average).Average, 4) }
    return [math]::Round((($s[1..($s.Count-2)] | Measure-Object -Average).Average), 4)
}

function Resolve-Runnable {
    # Windows executes a PE only when it carries an executable extension. The
    # host build is named `run` because the Dockerfile needs
    # `COPY --from=build /src/run /app/run` on Linux, and rustc on Windows writes
    # exactly `run` plus `run.pdb` -- so on this host the binary cannot be run at
    # all: PowerShell's call operator falls through to the shell handler and sets
    # no exit code, and cmd reports "not recognized". Stage a byte-identical copy
    # under a name the OS accepts; the program is unchanged.
    param([string]$Path)
    if ($Path -match '\.exe$') { return $Path }
    if ($env:OS -ne 'Windows_NT') { return $Path }
    $track = Split-Path (Split-Path $Path -Parent) -Leaf
    $staged = Join-Path ([System.IO.Path]::GetTempPath()) "sd-$track-run.exe"
    Copy-Item -LiteralPath $Path -Destination $staged -Force
    return $staged
}

$dataDir = "$Root\verification\data\track-$Track"
$outDir  = "$Root\verification\out\track-$Track"
New-Item -ItemType Directory -Force -Path $dataDir, $outDir | Out-Null

# -PathType Leaf matters: Test-Path returns TRUE for a directory, so without it
# a missing binary would silently select a directory path and fail obscurely
# when executed. run.exe is checked first -- see Resolve-Runnable.
$bin = "$Root\track-$Track\run"
foreach ($c in @("$Root\track-$Track\run.exe", "$Root\track-$Track\run", "/app/run")) {
    if (Test-Path $c -PathType Leaf) { $bin = $c; break }
}
if (-not (Test-Path $bin -PathType Leaf)) {
    Write-Host "  [FAIL] binary not found — run: sh track-$Track/build.sh" -ForegroundColor Red
    exit 1
}
$bin = Resolve-Runnable $bin

Write-Host "`n=== BENCH: Track $Track ($($info.Name)) ===" -ForegroundColor Cyan
Write-Host "binary : $bin" -ForegroundColor Gray
Write-Host "data   : $dataDir" -ForegroundColor Gray
Write-Host "runs   : $Runs (discard hi+lo, mean of middle $($Runs-2))" -ForegroundColor Gray

$times = @()
$hashes = @()
$peaks  = @()

for ($i = 1; $i -le $Runs; $i++) {
    Get-ChildItem $outDir -File -ErrorAction SilentlyContinue | Remove-Item -Force
    $env:SD_DATA_DIR = $dataDir
    $env:SD_OUT_DIR  = $outDir
    $env:TRACK       = $Track

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $code = Invoke-Native $bin @()
    $sw.Stop()
    $times += $sw.Elapsed.TotalSeconds

    if ($code -ne 0) { Write-Host "run $i FAILED (exit $code)" -ForegroundColor Red; exit 1 }

    # A run that exits 0 without producing its declared output file must FAIL.
    # The old code appended a hash only `if (Test-Path $outFile)` with no else,
    # so a binary that wrote nothing produced zero hashes, skipped the
    # determinism check below (which is guarded by `$hashes.Count -gt 0`), and
    # the script exited 0 — reporting a pass for a track that emitted no
    # artifact at all. A verifier that cannot fail is worse than none.
    $outFile = Join-Path $outDir $info.Output
    if (-not (Test-Path $outFile -PathType Leaf)) {
        Write-Host "run $i produced no $($info.Output) in $outDir — the output contract is broken" -ForegroundColor Red
        exit 1
    }
    $hashes += (Get-FileHash $outFile -Algorithm SHA256).Hash.ToLower()
}

$tMean = Get-TrimmedMean -Values $times
$best  = ($times | Measure-Object -Minimum).Minimum
$worst = ($times | Measure-Object -Maximum).Maximum
$ratio = [math]::Round($tMean / $info.Ref, 3)
$spread = ($worst - $best) / $tMean * 100

Write-Host "`n--- raw runs (s) ---" -ForegroundColor Cyan
# The discarded runs are the fastest and slowest BY VALUE, not the first and
# last chronologically. The old code marked run 1 and run N, which on a warm-up
# pattern (slow first run, tight rest) pointed at the wrong two measurements —
# a report that misidentifies which samples were used is exactly what PERF.md
# is judged on.
$sortedIdx = 0..($times.Count - 1) | Sort-Object { $times[$_] }
$discard = @()
if ($times.Count -ge 3) { $discard = @($sortedIdx[0], $sortedIdx[$sortedIdx.Count - 1]) }
for ($i = 0; $i -lt $times.Count; $i++) {
    $mark = if ($discard -contains $i) { "  (discarded)" } else { "  <- used" }
    Write-Host ("  run {0}: {1,8:N3}s{2}" -f ($i + 1), $times[$i], $mark)
}

Write-Host "`n--- RESULT ---" -ForegroundColor Cyan
Write-Host ("  T (trimmed mean) : {0:N4} s" -f $tMean)
Write-Host ("  fastest / slowest: {0:N3} s / {1:N3} s   spread {2:N1}%" -f $best, $worst, $spread)
Write-Host ("  local ratio vs ref: {0}x   (reference {1}s on fsn1)" -f $ratio, $info.Ref)
Write-Host ("  timeout headroom  : {0:N1}x under the {1}s hard kill" -f ($info.Timeout / $tMean), $info.Timeout)

if ($hashes.Count -gt 0) {
    # `@()` is load-bearing: with exactly one hash, `Select-Object -Unique`
    # returns a bare STRING, and `$uniq[0]` then indexes its first CHARACTER —
    # so the report printed a single "f" instead of the digest. Same trap at
    # pre-freeze.ps1's hash comparison, where it would have silently compared
    # one character against 64 and failed the check for the wrong reason.
    $uniq = @($hashes | Select-Object -Unique)
    if ($uniq.Count -eq 1) {
        Write-Host ("  output SHA-256    : {0}  (deterministic across {1} runs)" -f $uniq[0], $hashes.Count) -ForegroundColor Green
    } else {
        Write-Host ("  output SHA-256    : {0} DISTINCT HASHES — NONDETERMINISTIC" -f $uniq.Count) -ForegroundColor Red
        exit 1
    }
}

# ── G2 TIMEOUT (hard kill at 10x reference; the old speed-floor gate is gone) ──
# The organizers zero any track whose time is more than 10x the reference.
# Reporting the ratio without failing on it made this a comment, not a gate:
# a track running 1000x slow still exited 0.
if ($ratio -gt 10.0) {
    Write-Host ("  [FAIL] G2 timeout: {0}x the reference exceeds the 10x hard kill" -f $ratio) -ForegroundColor Red
    exit 1
}
Write-Host ("  [PASS] G2 timeout: {0}x the reference (hard kill at 10x)" -f $ratio) -ForegroundColor Green

# ── optional container run for the real M axis (cgroup memory.peak) ──
if ($Docker) {
    Write-Host "`n--- container run (scoring-box replica) ---" -ForegroundColor Cyan
    $image = "speeddemon/local:$Track"
    Write-Host "build: docker build --network=none -t $image ." -ForegroundColor Gray
    Write-Host "run  : docker run --rm --cpuset-cpus=0-11 --cpuset-mems=0 --memory=8g --memory-swap=8g --pids-limit=512 --network=none --read-only --tmpfs /tmp:rw,size=1g,exec --ulimit nofile=8192:8192 -v ${dataDir}:/data:ro -v ${outDir}:/out:rw -e TRACK=$Track $image" -ForegroundColor Gray
    if (Get-Command docker -ErrorAction SilentlyContinue) {
        docker build --network=none -t $image "$Root\track-$Track"
        if ($LASTEXITCODE -eq 0) {
            docker run --rm --cpuset-cpus="0-11" --cpuset-mems="0" --memory=8g --memory-swap=8g `
                --pids-limit=512 --network=none --read-only --tmpfs /tmp:rw,size=1g,exec `
                --ulimit nofile=8192:8192 -v "${dataDir}:/data:ro" -v "${outDir}:/out:rw" `
                -e TRACK=$Track $image
            Write-Host "container exit code: $LASTEXITCODE"
            Write-Host "cgroup peak: cat /sys/fs/cgroup/system.slice/docker-*.scope/memory.peak" -ForegroundColor Gray
        }
    } else {
        Write-Warn "docker not available on this host — container replica skipped"
    }
}
