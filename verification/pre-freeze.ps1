<#
.SYNOPSIS
    THE PRE-FREEZE LOCKDOWN. Run this in the last 6 hours before Nov 2 18:00 UTC.

.DESCRIPTION
    A literal, executable checklist of everything that must be true at the
    moment of code freeze. Any FAIL here means the submission is at risk of
    scoring ZERO on a track — a gate failure publishes itself, so this script
    exists to make sure it never happens.

    The checks:
      1.  offline build from a clean checkout (no network)
      2.  sample SHA-256 green (the correctness gate)
      3.  byte-identical output across 10 runs (nondeterminism = gate fail)
      4.  no scaffold sentinel left in any track
      5.  every required file present per track
          (Dockerfile, PERF.md, README.md, .speed-demon.toml, src/)
      6.  D-axis honesty: declared_dependencies == 0 and no vendored crates
      7.  exactly one output file per track
      8.  peak memory and wall time under the safety floors
      9.  the repo is clean and the final commit is recorded
      10. the public repo + OSI licence are in place

.PARAMETER Track
    A, B, C, D, E or All.

.PARAMETER SkipCleanBuild
    Skip check 1 (it clones the repo into a temp dir; slow).
#>
[CmdletBinding()]
param(
    [string]$Track = "All",
    [switch]$All,
    [switch]$SkipCleanBuild
)

if ($All) { $Track = "All" }

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

$TrackInfo = @{
    A = @{ Name = "A-ingest";   Timeout = 412; Ref = 41.20; Binary = "speeddemon-a"; Input = "access.log";  Output = "ingest.txt" }
    B = @{ Name = "B-index";    Timeout = 587; Ref = 58.70; Binary = "speeddemon-b"; Input = "corpus.txt";  Output = "index.txt" }
    C = @{ Name = "C-join";     Timeout = 369; Ref = 36.90; Binary = "speeddemon-c"; Input = "records.csv"; Output = "join.txt" }
    D = @{ Name = "D-traverse"; Timeout = 724; Ref = 72.40; Binary = "speeddemon-d"; Input = "graph.txt";  Output = "traverse.txt" }
    E = @{ Name = "E-compress"; Timeout = 298; Ref = 29.80; Binary = "speeddemon-e"; Input = "corpus.bin";  Output = "corpus.sd" }
}

$script:failures = 0
$script:checks   = 0

# Invoke a native command without $ErrorActionPreference=Stop treating its
# stderr as a terminating error (PowerShell 5.1 quirk): the redirection
# runs in a child scope where EAP is Continue.
#
# Returns ONLY the exit code. Output is discarded on purpose: a function
# returning both stdout and the exit code yields an array, and `$rc -eq 0`
# on an array is not the comparison you think it is.
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

# Same, but returns the command's stdout as a trimmed string. The exit code
# is left in $LASTEXITCODE for the caller to read.
function Get-NativeOut {
    $eap = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $flat = @()
        foreach ($a in $args) {
            if ($a -is [System.Array]) { $flat += $a } else { $flat += $a }
        }
        $exe = $flat[0]
        $rest = @()
        if ($flat.Count -gt 1) { $rest = $flat[1..($flat.Count - 1)] }
        $out = & $exe @rest 2>&1
    } finally {
        $ErrorActionPreference = $eap
    }
    return (($out | Out-String)).Trim()
}

function Check { param([string]$Name, [bool]$Ok, [string]$Detail = "", [switch]$Skip)
    $script:checks++
    if ($Skip) { Write-Host "  [SKIP] $Name  $Detail" -ForegroundColor DarkGray; return }
    if ($Ok)   { Write-Host "  [PASS] $Name  $Detail" -ForegroundColor Green }
    else       { Write-Host "  [FAIL] $Name  $Detail" -ForegroundColor Red; $script:failures++ }
}

Write-Host "`n################ PRE-FREEZE LOCKDOWN ################" -ForegroundColor Magenta
Write-Host "Track selection: $Track" -ForegroundColor Magenta
Write-Host "This is the last gate before Nov 2 18:00 UTC code freeze." -ForegroundColor Magenta

# ── 1. offline build from a clean checkout ────────────────────────────────
if (-not $SkipCleanBuild) {
    Write-Host "`n[1] OFFLINE BUILD FROM A CLEAN CHECKOUT" -ForegroundColor Cyan
    $tmp = Join-Path $env:TEMP ("speeddemon-freeze-" + [guid]::NewGuid().ToString("N").Substring(0,8))
    if (Get-Command git -ErrorAction SilentlyContinue) {
        $rc = Invoke-Native git @("-C", $Root, "rev-parse", "--git-dir")
        if ($rc -eq 0) {
            $rc = Invoke-Native git @("clone", "--quiet", $Root, $tmp)
            if ((Test-Path $tmp) -and ($rc -eq 0)) {
                Check "clone of the recorded commit" $true
                Push-Location $tmp
                $rc = Invoke-Native cargo @("build", "--release", "--offline")
                Check "offline release build (no network)" ($rc -eq 0) "cargo build --offline"
                Pop-Location
                Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
            } else {
                Check "clone of the recorded commit" $false "git clone failed"
            }
        } else {
            Check "clean checkout build" $true "SKIPPED - not a git repository yet" -Skip
        }
    } else {
        Check "clean checkout build" $true "SKIPPED - git unavailable" -Skip
    }
}

$targets = if ($Track -eq "All") { @("A","B","C","D","E") } else { @($Track.ToUpper()) }

foreach ($t in $targets) {
    $info = $TrackInfo[$t]
    $dir  = "$Root\tracks\$($info.Name)"
    Write-Host "`n[TRACK $t — $($info.Name)]" -ForegroundColor Cyan

    # ── 5. required files ──
    foreach ($f in @("Dockerfile","PERF.md","README.md",".speed-demon.toml","build.sh")) {
        Check "required file: $f" (Test-Path "$dir\$f")
    }
    Check "required dir: src/" (Test-Path "$dir\src")

    # ── 6. D-axis honesty ──
    $tomlPath = "$dir\.speed-demon.toml"
    if (Test-Path $tomlPath) {
        $toml = Get-Content $tomlPath -Raw
        $decl = if ($toml -match 'declared_dependencies\s*=\s*(\d+)') { [int]$Matches[1] } else { -1 }
        Check "D axis: declared_dependencies == 0" ($decl -eq 0) "declared=$decl"
        $vend = if ($toml -match 'vendored_dependency_bytes\s*=\s*(\d+)') { [int]$Matches[1] } else { -1 }
        Check "D axis: no vendored dependency bytes" ($vend -eq 0) "vendored=$vend"
    }

    # ── 4. scaffold sentinel gone ──
    $srcFiles = Get-ChildItem "$dir\src" -Filter *.rs -ErrorAction SilentlyContinue
    $hasSentinel = $false
    foreach ($sf in $srcFiles) {
        if (Select-String -Path $sf.FullName -Pattern "SCAFFOLD_NOT_IMPLEMENTED" -SimpleMatch -Quiet) { $hasSentinel = $true }
    }
    Check "no scaffold sentinel in source" (-not $hasSentinel) "a stub must never reach the freeze"

    # ── 2/3/7/8. functional gates ──
    $bin = @(
        "$Root\target\release\$($info.Binary).exe",
        "$Root\target\debug\$($info.Binary).exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1

    if (-not $bin) {
        Check "binary built" $false "run cargo build --release"
        continue
    }
    Check "binary built" $true (Split-Path $bin -Leaf)

    $dataDir = "$Root\verification\data\track-$t"
    $outDir  = "$Root\verification\out\track-$t"
    New-Item -ItemType Directory -Force -Path $dataDir, $outDir | Out-Null

    $expected = $null
    $expFile = "$dir\verification\expected\$($info.Output).sha256"
    if (Test-Path $expFile) {
        $raw = (Get-Content $expFile -Raw).Trim()
        if ($raw -match '^([0-9a-fA-F]{64})') { $expected = $Matches[1].ToLower() }
    }
    if (-not $expected) {
        $expFile2 = "$dir\verification\expected\sample.sha256"
        if (Test-Path $expFile2) {
            $raw = (Get-Content $expFile2 -Raw).Trim()
            if ($raw -match '^([0-9a-fA-F]{64})') { $expected = $Matches[1].ToLower() }
        }
    }

    # 10 runs for determinism
    $hashes = @()
    $times  = @()
    $ok     = $true
    for ($i = 1; $i -le 10; $i++) {
        Get-ChildItem $outDir -File -ErrorAction SilentlyContinue | Remove-Item -Force
        $env:SD_DATA_DIR = $dataDir; $env:SD_OUT_DIR = $outDir; $env:TRACK = $t
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $code = Invoke-Native $bin @()
        $sw.Stop()
        $times += $sw.Elapsed.TotalSeconds
        if ($code -ne 0) { Check "run $i exits 0" $false "exit=$code"; $ok = $false; break }
        $of = Join-Path $outDir $info.Output
        if (-not (Test-Path $of)) { Check "run $i writes output" $false; $ok = $false; break }
        $hashes += (Get-FileHash $of -Algorithm SHA256).Hash.ToLower()
    }
    if ($ok) {
        # 7. exactly one output file
        $outFiles = @(Get-ChildItem $outDir -File)
        Check "exactly one file written to /out" ($outFiles.Count -eq 1) "count=$($outFiles.Count)"

        # 3. determinism
        # `@()` forces an array: with one run, `Select-Object -Unique` yields a
        # bare string, and `$uniq[0]` would be its first CHARACTER — so the
        # hash comparison below would compare one char against 64 and fail (or,
        # worse, "pass" on a one-character expected value).
        $uniq = @($hashes | Select-Object -Unique)
        Check "byte-identical across 10 runs" ($uniq.Count -eq 1) "distinct=$($uniq.Count)"

        # 2. correctness
        if ($expected) {
            Check "sample SHA-256 green" ($uniq[0] -eq $expected) "expected=$($expected.Substring(0,12)) got=$($uniq[0].Substring(0,12))"
        } else {
            Check "sample SHA-256 green" $true "NO EXPECTED HASH PINNED — cannot be verified" -Skip
        }

        # 8. safety floors
        $s = $times | Sort-Object
        $tMean = ($s[1..8] | Measure-Object -Average).Average
        $worst = ($times | Measure-Object -Maximum).Maximum
        Check "wall time under 70% of the timeout" ($worst -lt ($info.Timeout * 0.7)) ("worst={0:N1}s limit={1:N0}s" -f $worst, ($info.Timeout * 0.7))
        Check "T under 10x the reference" ($tMean -lt ($info.Ref * 10)) ("T={0:N2}s ref={1}s" -f $tMean, $info.Ref)
    }
}

# ── 9/10. repo-level ──────────────────────────────────────────────────────
Write-Host "`n[REPO]" -ForegroundColor Cyan
if (Get-Command git -ErrorAction SilentlyContinue) {
    $rc = Invoke-Native git @("-C", $Root, "rev-parse", "--git-dir")
    if ($rc -eq 0) {
        $head = Get-NativeOut git @("-C", $Root, "rev-parse", "HEAD")
        Check "git repository with a recorded commit" ([bool]$head) "HEAD=$($head.Substring(0,[Math]::Min(12,$head.Length)))"
        $dirty = Get-NativeOut git @("-C", $Root, "status", "--porcelain")
        # The measurement ledger is an append-only log: running the gates
        # legitimately appends to it, so it is not an "uncommitted change".
        # Everything else is.
        $dirtyReal = (($dirty -split "`n") | Where-Object {
            $_.Trim() -and $_.Trim() -notmatch 'verification/ledger\.jsonl$'
        }) -join ""
        Check "working tree clean" ([string]::IsNullOrWhiteSpace($dirtyReal)) $(if($dirtyReal){"uncommitted changes exist"}else{""})
        $remotes = (Get-NativeOut git @("-C", $Root, "remote", "-v") | Out-String) -split "`n" | Where-Object { $_.Trim() }
        Check "public remote configured" (@($remotes).Count -ge 2) "remotes=$(@($remotes).Count) (expect fetch+push entries)"
    } else {
        Check "git repository" $false "not initialised yet"
    }
} else {
    Check "git repository" $true "SKIPPED - git unavailable" -Skip
}
Check "OSI licence present" (Test-Path "$Root\LICENSE") (Split-Path (Get-Content "$Root\LICENSE" -TotalCount 1) -Leaf)

# ── verdict ────────────────────────────────────────────────────────────────
Write-Host "`n################ RESULT ################" -ForegroundColor Magenta
if ($script:failures -eq 0) {
    Write-Host "LOCKDOWN: ALL CHECKS PASS ($($script:checks) checks)" -ForegroundColor Green
    Write-Host "You are clear to freeze." -ForegroundColor Green
    exit 0
} else {
    Write-Host "LOCKDOWN: $($script:failures) of $($script:checks) CHECKS FAILED" -ForegroundColor Red
    Write-Host "Do NOT freeze. Fix every FAIL above." -ForegroundColor Red
    exit 1
}
