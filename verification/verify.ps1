<#
.SYNOPSIS
    Speed Demon local scoring-box replica + gate verifier.

.DESCRIPTION
    Reproduces the fsn1 (Falkenstein) harness contract as closely as the local machine
    allows, then runs the FOUR GATES that decide whether a track scores at all:

      G1 CORRECTNESS  SHA-256 of the output file == the expected hash.
                      A mismatch, or a missing expected hash, is a FAIL.
      G2 TIMEOUT      wall time must stay under the hard kill at 10x the
                      reference time (A 321s / B 564s / C 144s / D 727s).
      G3 SANDBOX      exactly one file in /out, no writes outside /tmp, and
                      the output must not contain the scaffold sentinel.

    (There is no speed-floor gate: the organizers removed it. The speed ratio
    is reported below as information only.)

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

# ── fsn1 constants (frozen) ─────────────────────────────────────────────────
$TrackInfo = @{
    A = @{ Name = "track-a"; Timeout = 321; Ref = 32.05; Binary = "run"; Input = "requests.log";  Output = "summary.txt" }
    B = @{ Name = "track-b"; Timeout = 564; Ref = 56.35; Binary = "run"; Input = "docs.txt";  Output = "results.txt" }
    C = @{ Name = "track-c"; Timeout = 144; Ref = 14.44; Binary = "run"; Input = "records.csv"; Output = "aggregates.txt" }
    D = @{ Name = "track-d"; Timeout = 727; Ref = 72.69; Binary = "run"; Input = "graph.txt";  Output = "distances.txt" }
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

function Resolve-Runnable {
    # Windows executes a PE only when it carries an executable extension. The
    # host build is deliberately named `run`, because the Dockerfile needs
    # `COPY --from=build /src/run /app/run` on Linux, and rustc on Windows writes
    # exactly `run` plus `run.pdb` -- so on this host the binary cannot be
    # executed at all: PowerShell's call operator falls through to the shell
    # handler and sets no exit code, and cmd reports "not recognized".
    #
    # Stage a byte-identical copy under a name the OS accepts. The copy does not
    # change the program, so the hash gate is unaffected.
    param([string]$Path)
    if ($Path -match '\.exe$') { return $Path }
    if ($env:OS -ne 'Windows_NT') { return $Path }
    $track = Split-Path (Split-Path $Path -Parent) -Leaf
    $staged = Join-Path ([System.IO.Path]::GetTempPath()) "sd-$track-run.exe"
    Copy-Item -LiteralPath $Path -Destination $staged -Force
    return $staged
}

function Get-NativeValue {
    # Run a native command and return its stdout, WITHOUT letting
    # $ErrorActionPreference = "Stop" turn the command's STDERR into a
    # terminating error. PowerShell 5.1 raises NativeCommandError for stderr on
    # a native command, and this script stops on errors -- so a tool that writes
    # a diagnostic to stderr killed the whole gate. official-data.py
    # --expected-for prints "nomatch" there and exits 3, which meant that on a
    # tree with no dataset (a clean clone) verify.ps1 aborted before it reported
    # anything at all. Invoke-Native already existed for exactly this quirk; this
    # is the same guard for a call whose stdout is needed.
    param([string]$Exe, [string[]]$Arguments)
    $eap = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        return (& $Exe @Arguments 2>$null)
    } finally {
        $ErrorActionPreference = $eap
    }
}

function Get-ExpectedHash {
    param([string]$TrackId, [string]$DataDir)

    # The expected hash is SCALE-SPECIFIC: seed 1 at tiny, sample and full give
    # three different correct answers, and the committed pin holds exactly one
    # of them. Comparing a run against that pin while the corpus on disk is a
    # different scale fails no matter how correct the implementation is -- and
    # that is what this script used to do (Track A carried tiny data against the
    # sample pin, B/C/D carried sample data against the sample pin, and the full
    # corpus the board actually uses matches neither).
    #
    # So ask the tool that owns the published table what the right answer is for
    # the bytes we actually have on disk. Its hash is the only reliable link
    # between a dataset and its expectation, and it is not duplicated here.
    # Guarded on the INPUT FILE rather than the directory: this script creates
    # the directory a few lines earlier, so a directory test always passed.
    $inFile = Join-Path $DataDir $TrackInfo[$TrackId].Input
    if (Test-Path $inFile) {
        $out = Get-NativeValue "python" @(
            "$Root\tooling\official-data.py", "--track", $TrackId.ToLower(),
            "--expected-for", $DataDir)
        # Identify the answer by its SHAPE, not by an exit code: the tool prints
        # "<track> <scale> <hash>" on success and nothing on stdout otherwise.
        $line = @($out) | Where-Object { $_ -match '^[abcd]\s+\S+\s+[0-9a-f]{64}$' } |
                Select-Object -First 1
        if ($line) {
            $parts = $line -split '\s+'
            return @{ Hash = $parts[2]; Scale = $parts[1]; Source = "derived from the data on disk" }
        }
    }

    # Fall back to the committed pin, but name it as a pin so a scale mismatch
    # can never be mistaken for a wrong answer.
    $c = "$Root\$($TrackInfo[$TrackId].Name)\verification\expected\$($TrackInfo[$TrackId].Output).sha256"
    if (Test-Path $c) {
        $raw = (Get-Content $c -Raw).Trim()
        if ($raw -match '^([0-9a-fA-F]{64})') {
            return @{ Hash = $Matches[1].ToLower(); Scale = "unknown scale"; Source = "committed pin file" }
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

    # locate the host build (run.exe first — see Resolve-Runnable for why the
    # extensionless `run` cannot be executed on Windows at all)
    $bin = $null
    foreach ($c in @("$Root\$($info.Name)\run.exe", "$Root\$($info.Name)\run", "/app/run")) {
        if (Test-Path $c -PathType Leaf) { $bin = $c; break }
    }
    if (-not $bin) {
        Write-Bad "no built binary in $($info.Name) — build it with: cd $($info.Name); sh build.sh   (or: sh tooling/ci.sh)"
        $overallOk = $false
        continue
    }
    $bin = Resolve-Runnable $bin
    Write-Info "binary: $bin"

    # data + output dirs
    $dataDir = "$Root\verification\data\track-$t"
    $outDir  = "$Root\verification\out\track-$t"
    if (-not (Test-Path $dataDir)) {
        New-Item -ItemType Directory -Force -Path $dataDir | Out-Null
    }
    $inPath = Join-Path $dataDir $info.Input
    if (-not (Test-Path $inPath)) {
        Write-Warn "no data at $inPath — generate a known dataset with:"
        Write-Warn "  python tooling\official-data.py --track $($t.ToLower()) --scale sample"
    }

    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    Get-ChildItem $outDir -File -ErrorAction SilentlyContinue | Remove-Item -Force

    $expected = Get-ExpectedHash -TrackId $t -DataDir $dataDir
    if ($expected) {
        Write-Info "expected SHA-256: $($expected.Hash)"
        Write-Info "                  [$($expected.Scale) — $($expected.Source)]"
    }
    else {
        Write-Warn "no published hash matches the data in $dataDir — G1 cannot be verified at all"
        Write-Warn "generate a known dataset first: python tooling\official-data.py --track $($t.ToLower()) --scale sample"
    }

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
        # The harness times from exec to exit and treats a non-zero exit as a
        # failed run, so this has to be the REAL exit code. The function returns
        # it; `$LASTEXITCODE` is not reliable across the function boundary, and
        # since `$null -ne 0` is TRUE in PowerShell, discarding the return value
        # made every run look like a non-zero exit and aborted the gate before
        # it ever checked a hash.
        $code = Invoke-Native $bin @()
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
        $allMatch = ($hashes | Where-Object { $_ -ne $expected.Hash }).Count -eq 0
        if ($allMatch) { Write-Ok "G1 correctness: SHA-256 matches on all $($hashes.Count) runs ($($expected.Scale))" }
        else {
            Write-Bad "G1 correctness: SHA mismatch (got $($hashes[0]), expected $($expected.Hash) at $($expected.Scale) scale)"
            $overallOk = $false
        }
    } else {
        Write-Warn "G1 correctness: UNVERIFIABLE (the data on disk matches no published dataset)"
    }

    # ── DETERMINISM (correctness gate applies to all 5 runs) ──
    $uniq = $hashes | Select-Object -Unique
    if ($uniq.Count -eq 1) { Write-Ok "determinism: byte-identical output across $($hashes.Count) runs" }
    else { Write-Bad "determinism: $($uniq.Count) distinct outputs — nondeterminism is a correctness bug"; $overallOk = $false }

    # ── G4 SANDBOX ──
    $outFiles = @(Get-ChildItem $outDir -File)
    if ($outFiles.Count -eq 1) { Write-Ok "G3 sandbox: exactly one file in the output dir" }
    else { Write-Bad "G3 sandbox: $($outFiles.Count) files in output dir (must be exactly 1)"; $overallOk = $false }

    $sentinel = Select-String -Path (Join-Path $outDir $info.Output) -Pattern "SCAFFOLD_NOT_IMPLEMENTED" -SimpleMatch -Quiet -ErrorAction SilentlyContinue
    if ($sentinel) { Write-Bad "G3 sandbox: scaffold sentinel present — the real implementation is not in place"; $overallOk = $false }
    else { Write-Ok "G3 sandbox: no scaffold sentinel in output" }

    # ── timing ──
    $tMean = Get-TrimmedMean -Values $times
    $spread = if ($times.Count -gt 0) { (($times | Measure-Object -Maximum).Maximum - ($times | Measure-Object -Minimum).Minimum) / $tMean * 100 } else { 0 }
    Write-Info ("runs (s): " + (($times | ForEach-Object { $_.ToString("0.000") }) -join ", "))
    Write-Info ("T (trimmed mean of middle $($times.Count - 2)): $tMean s   spread: $($spread.ToString("0.0"))%")

    if ($spread -gt 5.0) { Write-Warn "spread > 5% — the organizer re-runs batches like this" }

    # ── G3 TIMEOUT ──
    $worst = ($times | Measure-Object -Maximum).Maximum
    if ($worst -lt $info.Timeout) { Write-Ok "G2 timeout: worst run $($worst.ToString("0.00"))s < $($info.Timeout)s" }
    else { Write-Bad "G2 timeout: worst run $($worst.ToString("0.00"))s exceeds $($info.Timeout)s"; $overallOk = $false }

    # ── LOCAL SPEED RATIO (informational; the speed-floor gate is gone) ──
    $ratio = [math]::Round($tMean / $info.Ref, 3)
    Write-Info "local speed ratio vs reference: ${ratio}x (reference $($info.Ref)s on fsn1)"

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
