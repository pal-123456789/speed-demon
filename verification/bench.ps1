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
    A, B, C, D or E.

.PARAMETER Runs
    Scored runs (default 5).

.PARAMETER Docker
    Run inside a container that mirrors the scoring box.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("A","B","C","D","E")]
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
    A = @{ Name = "A-ingest";   Timeout = 412; Ref = 41.20; Binary = "speeddemon-a"; Input = "access.log";  Output = "ingest.txt" }
    B = @{ Name = "B-index";    Timeout = 587; Ref = 58.70; Binary = "speeddemon-b"; Input = "corpus.txt";  Output = "index.txt" }
    C = @{ Name = "C-join";     Timeout = 369; Ref = 36.90; Binary = "speeddemon-c"; Input = "records.csv"; Output = "join.txt" }
    D = @{ Name = "D-traverse"; Timeout = 724; Ref = 72.40; Binary = "speeddemon-d"; Input = "graph.txt";  Output = "traverse.txt" }
    E = @{ Name = "E-compress"; Timeout = 298; Ref = 29.80; Binary = "speeddemon-e"; Input = "corpus.bin";  Output = "corpus.sd" }
}
$info = $TrackInfo[$Track]

function Get-TrimmedMean {
    param([double[]]$Values)
    $s = $Values | Sort-Object
    if ($s.Count -lt 3) { return [math]::Round(($s | Measure-Object -Average).Average, 4) }
    return [math]::Round((($s[1..($s.Count-2)] | Measure-Object -Average).Average), 4)
}

$dataDir = "$Root\verification\data\track-$Track"
$outDir  = "$Root\verification\out\track-$Track"
New-Item -ItemType Directory -Force -Path $dataDir, $outDir | Out-Null

# -PathType Leaf matters: Test-Path returns TRUE for a directory, so without it
# a missing binary would silently select a directory path and fail obscurely
# when executed.
$bin = @(
    "$Root\target\release\$($info.Binary).exe",
    "$Root\target\debug\$($info.Binary).exe",
    "$Root\bin\$($info.Binary)"
) | Where-Object { Test-Path $_ -PathType Leaf } | Select-Object -First 1

if (-not $bin) {
    Write-Host "  [FAIL] binary $($info.Binary) not found — run 'cargo build --release'" -ForegroundColor Red
    exit 1
}

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
Write-Host ("  local ratio vs ref: {0}x   (reference {1}s on HEL1)" -f $ratio, $info.Ref)
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

# ── G2 SPEED FLOOR ──
# The organizers zero any track whose time is more than 10x the reference.
# Reporting the ratio without failing on it made this a comment, not a gate:
# a track running 1000x slow still exited 0.
if ($ratio -gt 10.0) {
    Write-Host ("  [FAIL] G2 speed floor: {0}x the reference exceeds the 10x ceiling" -f $ratio) -ForegroundColor Red
    exit 1
}
Write-Host ("  [PASS] G2 speed floor: {0}x the reference (ceiling 10x)" -f $ratio) -ForegroundColor Green

# ── optional container run for the real M axis (cgroup memory.peak) ──
if ($Docker) {
    Write-Host "`n--- container run (scoring-box replica) ---" -ForegroundColor Cyan
    $image = "speeddemon/local:$Track"
    Write-Host "build: docker build --network=none -t $image ." -ForegroundColor Gray
    Write-Host "run  : docker run --rm --cpuset-cpus=0-11 --cpuset-mems=0 --memory=8g --memory-swap=8g --pids-limit=512 --network=none --read-only --tmpfs /tmp:rw,size=1g,exec --ulimit nofile=8192:8192 -v ${dataDir}:/data:ro -v ${outDir}:/out:rw -e TRACK=$Track $image" -ForegroundColor Gray
    if (Get-Command docker -ErrorAction SilentlyContinue) {
        docker build --network=none -t $image $Root
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
