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

$bin = @(
    "$Root\target\release\$($info.Binary).exe",
    "$Root\target\debug\$($info.Binary).exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $bin) {
    Write-Error "binary $($info.Binary) not found — run 'cargo build --release'"
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
    & $bin *>$null
    $code = $LASTEXITCODE
    $sw.Stop()
    $times += $sw.Elapsed.TotalSeconds

    if ($code -ne 0) { Write-Host "run $i FAILED (exit $code)" -ForegroundColor Red; exit 1 }

    $outFile = Join-Path $outDir $info.Output
    if (Test-Path $outFile) {
        $hashes += (Get-FileHash $outFile -Algorithm SHA256).Hash.ToLower()
    }
}

$tMean = Get-TrimmedMean -Values $times
$best  = ($times | Measure-Object -Minimum).Minimum
$worst = ($times | Measure-Object -Maximum).Maximum
$ratio = [math]::Round($tMean / $info.Ref, 3)
$spread = ($worst - $best) / $tMean * 100

Write-Host "`n--- raw runs (s) ---" -ForegroundColor Cyan
for ($i = 0; $i -lt $times.Count; $i++) {
    $mark = if ($i -eq 0 -or $i -eq $times.Count-1) { "  (discarded)" } else { "  <- used" }
    Write-Host ("  run {0}: {1,8:N3}s{2}" -f ($i+1), $times[$i], $mark)
}

Write-Host "`n--- RESULT ---" -ForegroundColor Cyan
Write-Host ("  T (trimmed mean) : {0:N4} s" -f $tMean)
Write-Host ("  fastest / slowest: {0:N3} s / {1:N3} s   spread {2:N1}%" -f $best, $worst, $spread)
Write-Host ("  local ratio vs ref: {0}x   (reference {1}s on HEL1)" -f $ratio, $info.Ref)
Write-Host ("  timeout headroom  : {0:N1}x under the {1}s hard kill" -f ($info.Timeout / $tMean), $info.Timeout)

if ($hashes.Count -gt 0) {
    $uniq = $hashes | Select-Object -Unique
    if ($uniq.Count -eq 1) {
        Write-Host ("  output SHA-256    : {0}  (deterministic across {1} runs)" -f $uniq[0], $hashes.Count) -ForegroundColor Green
    } else {
        Write-Host ("  output SHA-256    : {0} DISTINCT HASHES — NONDETERMINISTIC" -f $uniq.Count) -ForegroundColor Red
        exit 1
    }
}

# ── optional container run for the real M axis (cgroup memory.peak) ──
if ($Docker) {
    Write-Host "`n--- container run (scoring-box replica) ---" -ForegroundColor Cyan
    $image = "speeddemon/local:$Track"
    Write-Host "build: docker build --network=none -t $image ." -ForegroundColor Gray
    Write-Host "run  : docker run --rm --cpuset-cpus=0-11 --cpuset-mems=0 --memory=8g --memory-swap=8g --pids-limit=512 --network=none --read-only --tmpfs /tmp:rw,size=1g,exec --ulimit nofile=8192:8192 -v $dataDir:/data:ro -v $outDir:/out:rw -e TRACK=$Track $image" -ForegroundColor Gray
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
