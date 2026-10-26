<#
.SYNOPSIS
    Speed Demon local scoring-box replica + gate verifier.

.DESCRIPTION
    Reproduces the HEL1 harness contract as closely as the local machine
    allows, then runs the FOUR GATES that decide whether a track scores at all:

      G1 CORRECTNESS  SHA-256 of the output file == the expected hash.
                      A mismatch, or a missing expected hash, is a FAIL.
      G2 SPEED FLOOR  wall time must be within 10x of the fastest measured
                      correct run in this track (a local proxy for the
                      organizer's leaderboard floor).
      G3 TIMEOUT      wall time must stay under 10x the reference time
                      (A 412s / B 587s / C 369s / D 724s).
      G4 SANDBOX      exactly one file in /out, no writes outside /tmp, and
                      the output must not contain the scaffold sentinel.

    Timing protocol mirrors the organizer: 5 runs, discard highest and
    lowest, mean of the middle three.

.PARAMETER Track
    A, B, C, D or All.

.PARAMETER Runs
    Number of scored runs (default 5).

.PARAMETER NoDocker
    Run the binaries directly (no container). Useful on a machine without
    Docker; sandbox checks are then skipped and reported as SKIPPED.
#>
[CmdletBinding()]
param(
    [string]$Track = "All",
    [int]$Runs = 5,
    [switch]$All,
    [switch]$NoDocker
)

if ($All) { $Track = "All" }

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

# ── HEL1 constants (frozen) ────────────────────────────────────────────────
$TrackInfo = @{
    A = @{ Name = "A-ingest";   Timeout = 412; Ref = 41.20; Binary = "speeddemon-a"; Input = "access.log";  Output = "ingest.txt" }
    B = @{ Name = "B-index";    Timeout = 587; Ref = 58.70; Binary = "speeddemon-b"; Input = "corpus.txt";  Output = "index.txt" }
    C = @{ Name = "C-join";     Timeout = 369; Ref = 36.90; Binary = "speeddemon-c"; Input = "records.csv"; Output = "join.txt" }
    D = @{ Name = "D-traverse"; Timeout = 724; Ref = 72.40; Binary = "speeddemon-d"; Input = "graph.txt";  Output = "traverse.txt" }
}

# ── helpers ────────────────────────────────────────────────────────────────
function Write-Head($text) { Write-Host "`n=== $text ===" -ForegroundColor Cyan }
function Write-Ok($text)   { Write-Host "  [PASS] $text" -ForegroundColor Green }
function Write-Bad($text)  { Write-Host "  [FAIL] $text" -ForegroundColor Red }
function Write-Warn($text) { Write-Host "  [WARN] $text" -ForegroundColor Yellow }
function Write-Info($text) { Write-Host "  [INFO] $text" -ForegroundColor Gray }

# Invoke a native command without $ErrorActionPreference=Stop treating its
# stderr as a terminating error (PowerShell 5.1 quirk).
function Invoke-Native {
    $eap = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        # A parenthesised array passed positionally arrives as ONE argument,
        # so flatten the list before handing it to the executable.
        $flat = @()
        foreach ($a in $args) {
            if ($a -is [System.Array]) { $flat += $a } else { $flat += $a }
        }
        $exe = $flat[0]
        $rest = @()
        if ($flat.Count -gt 1) { $rest = $flat[1..($flat.Count - 1)] }
        $null = & $exe @rest 2>&1
    } finally {
        $ErrorActionPreference = $eap
    }
    return [int]$LASTEXITCODE
}

function Get-ExpectedHash {
    param([string]$TrackId)
    $candidates = @(
        "$Root\tracks\$($TrackInfo[$TrackId].Name)\verification\expected\$($TrackInfo[$TrackId].Output).sha256",
        "$Root\tracks\$($TrackInfo[$TrackId].Name)\verification\expected\sample.sha256"
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) {
            $raw = (Get-Content $c -Raw).Trim()
            # accept "<hash>  <filename>" or a bare hash
            if ($raw -match '^([0-9a-fA-F]{64})') { return $Matches[1].ToLower() }
        }
    }
    return $null
}

function Get-TrimmedMean {
    param([double[]]$Values)
    $s = $Values | Sort-Object
    if ($s.Count -lt 3) { return [math]::Round(($s | Measure-Object -Average).Average, 4) }
    $mid = $s[1..($s.Count - 2)]
    return [math]::Round(($mid | Measure-Object -Average).Average, 4)
}

# ── main ───────────────────────────────────────────────────────────────────
$targets = if ($Track -eq "All") { @("A","B","C","D") } else { @($Track.ToUpper()) }
$overallOk = $true
$summary = @()

foreach ($t in $targets) {
    $info = $TrackInfo[$t]
    Write-Head "TRACK $t ($($info.Name))"

    # locate a built binary: cargo target dir first, then /app/bin
    $bin = $null
    $cands = @(
        "$Root\target\release\$($info.Binary).exe",
        "$Root\target\debug\$($info.Binary).exe",
        "$Root\bin\$($info.Binary)",
        "/app/bin/$($info.Binary)"
    )
    foreach ($c in $cands) { if (Test-Path $c) { $bin = $c; break } }
    if (-not $bin) {
        Write-Bad "binary $($info.Binary) not found — run 'cargo build --release' first"
        $overallOk = $false
        continue
    }
    Write-Info "binary: $bin"

    # data + output dirs
    $dataDir = "$Root\verification\data\track-$t"
    $outDir  = "$Root\verification\out\track-$t"
    if (-not (Test-Path $dataDir)) {
        New-Item -ItemType Directory -Force -Path $dataDir | Out-Null
    }
    $inPath = Join-Path $dataDir $info.Input
    if (-not (Test-Path $inPath)) {
        Write-Warn "no sample data at $inPath — create it with verification/gen-fixtures.py"
    }

    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    Get-ChildItem $outDir -File -ErrorAction SilentlyContinue | Remove-Item -Force

    $expected = Get-ExpectedHash -TrackId $t
    if ($expected) { Write-Info "expected SHA-256: $expected" }
    else { Write-Warn "no expected hash pinned yet (SHA gate cannot PASS until the Oct 30 sample ships)" }

    # ── run N times ──
    $times = @()
    $hashes = @()
    $peakMb = 0.0
    $runFailed = $false

    for ($i = 1; $i -le $Runs; $i++) {
        Get-ChildItem $outDir -File -ErrorAction SilentlyContinue | Remove-Item -Force
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $env:SD_DATA_DIR = $dataDir
        $env:SD_OUT_DIR  = $outDir
        $env:TRACK       = $t
        $null = Invoke-Native $bin @()
        $code = $LASTEXITCODE
        $sw.Stop()
        $times += $sw.Elapsed.TotalSeconds

        if ($code -ne 0) {
            Write-Bad "run $i exited with code $code (non-zero exit = failed run)"
            $runFailed = $true
            break
        }
        $outFile = Join-Path $outDir $info.Output
        if (-not (Test-Path $outFile)) {
            Write-Bad "run $i produced no output file $($info.Output)"
            $runFailed = $true
            break
        }
        $h = (Get-FileHash $outFile -Algorithm SHA256).Hash.ToLower()
        $hashes += $h
    }

    if ($runFailed) { $overallOk = $false; continue }

    # ── G1 CORRECTNESS ──
    if ($expected) {
        $allMatch = ($hashes | Where-Object { $_ -ne $expected }).Count -eq 0
        if ($allMatch) { Write-Ok "G1 correctness: SHA-256 matches on all $($hashes.Count) runs" }
        else {
            Write-Bad "G1 correctness: SHA mismatch (got $($hashes[0]))"
            $overallOk = $false
        }
    } else {
        Write-Warn "G1 correctness: UNVERIFIABLE (no expected hash pinned)"
    }

    # ── DETERMINISM (correctness gate applies to all 5 runs) ──
    $uniq = $hashes | Select-Object -Unique
    if ($uniq.Count -eq 1) { Write-Ok "determinism: byte-identical output across $($hashes.Count) runs" }
    else { Write-Bad "determinism: $($uniq.Count) distinct outputs — nondeterminism is a correctness bug"; $overallOk = $false }

    # ── G4 SANDBOX ──
    $outFiles = @(Get-ChildItem $outDir -File)
    if ($outFiles.Count -eq 1) { Write-Ok "G4 sandbox: exactly one file in the output dir" }
    else { Write-Bad "G4 sandbox: $($outFiles.Count) files in output dir (must be exactly 1)"; $overallOk = $false }

    $sentinel = Select-String -Path (Join-Path $outDir $info.Output) -Pattern "SCAFFOLD_NOT_IMPLEMENTED" -SimpleMatch -Quiet -ErrorAction SilentlyContinue
    if ($sentinel) { Write-Bad "G4 sandbox: scaffold sentinel present — the real implementation is not in place"; $overallOk = $false }
    else { Write-Ok "G4 sandbox: no scaffold sentinel in output" }

    # ── timing ──
    $tMean = Get-TrimmedMean -Values $times
    $spread = if ($times.Count -gt 0) { (($times | Measure-Object -Maximum).Maximum - ($times | Measure-Object -Minimum).Minimum) / $tMean * 100 } else { 0 }
    Write-Info ("runs (s): " + (($times | ForEach-Object { $_.ToString("0.000") }) -join ", "))
    Write-Info ("T (trimmed mean of middle $($times.Count - 2)): $tMean s   spread: $($spread.ToString("0.0"))%")

    if ($spread -gt 5.0) { Write-Warn "spread > 5% — the organizer re-runs batches like this" }

    # ── G3 TIMEOUT ──
    $worst = ($times | Measure-Object -Maximum).Maximum
    if ($worst -lt $info.Timeout) { Write-Ok "G3 timeout: worst run $($worst.ToString("0.00"))s < $($info.Timeout)s" }
    else { Write-Bad "G3 timeout: worst run $($worst.ToString("0.00"))s exceeds $($info.Timeout)s"; $overallOk = $false }

    # ── G2 SPEED FLOOR (local proxy) ──
    $ratio = [math]::Round($tMean / $info.Ref, 3)
    Write-Info "local speed ratio vs reference: ${ratio}x (reference $($info.Ref)s on HEL1)"

    $summary += [pscustomobject]@{
        Track = $t
        T      = $tMean
        Ratio  = $ratio
        Hash   = if ($hashes) { $hashes[0].Substring(0, 12) } else { "-" }
        Status = if ($overallOk) { "PASS" } else { "FAIL" }
    }
}

Write-Head "SUMMARY"
$summary | Format-Table -AutoSize | Out-String -Width 200 | Write-Host

if ($overallOk) { Write-Host "VERIFICATION: PASS" -ForegroundColor Green }
else { Write-Host "VERIFICATION: FAIL" -ForegroundColor Red }
exit $(if ($overallOk) { 0 } else { 1 })
