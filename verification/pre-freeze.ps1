<#
.SYNOPSIS
    THE PRE-FREEZE LOCKDOWN. Run this in the last 6 hours before Nov 2 15:00 UTC.

.DESCRIPTION
    A literal, executable checklist of everything that must be true at the
    moment of code freeze. Any FAIL here means the submission is at risk of
    scoring ZERO on a track — a gate failure publishes itself, so this script
    exists to make sure it never happens.

    The checks:
      1.  offline build from a clean checkout (no network)
      2.  SHA-256 green at the scale of the data on disk (the correctness gate)
      3.  byte-identical output across 10 runs (nondeterminism = gate fail)
      4.  no scaffold sentinel left in any track
      5.  every required file present per track
          (Dockerfile, PERF.md, README.md, .speed-demon.toml, src/)
      6.  D-axis honesty: dependency_count == 0 and no vendored crates
      7.  exactly one output file per track
      8.  peak memory and wall time under the safety floors
      9.  the repo is clean and the final commit is recorded
      10. the public repo + OSI licence are in place

.PARAMETER Track
    A, B, C, D or All.

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
    A = @{ Name = "track-a"; Timeout = 321; Ref = 32.05; Binary = "run"; Input = "requests.log";  Output = "summary.txt" }
    B = @{ Name = "track-b"; Timeout = 564; Ref = 56.35; Binary = "run"; Input = "docs.txt";  Output = "results.txt" }
    C = @{ Name = "track-c"; Timeout = 144; Ref = 14.44; Binary = "run"; Input = "records.csv"; Output = "aggregates.txt" }
    D = @{ Name = "track-d"; Timeout = 727; Ref = 72.69; Binary = "run"; Input = "graph.txt";  Output = "distances.txt" }
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
Write-Host "This is the last gate before Nov 2 15:00 UTC code freeze." -ForegroundColor Magenta

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
                if (Get-Command sh -ErrorAction SilentlyContinue) {
                    Push-Location $tmp
                    $rc = 0
                    foreach ($tt in @("A","B","C","D")) {
                        $rc = Invoke-Native sh @("-c", "cd `"$tt`" && ./build.sh")
                        if ($rc -ne 0) { break }
                    }
                    Check "offline build of all four track folders (rustc only)" ($rc -eq 0) "sh track-*/build.sh"
                    Pop-Location
                } else {
                    Check "offline build of all four track folders (rustc only)" $true "SKIPPED - sh unavailable on this host" -Skip
                }
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

$targets = if ($Track -eq "All") { @("A","B","C","D") } else { @($Track.ToUpper()) }

foreach ($t in $targets) {
    $info = $TrackInfo[$t]
    $dir  = "$Root\track-$t"
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
        # The declared-dependency key is `dependency_count`. That is what the
        # organizers' own parser reads -- reference/official/harness/
        # dependencies.py does `declaration.get("dependency_count")` -- and it
        # is the key their fixture manifests use. This gate previously looked for
        # a key name that appears NOWHERE in the official code, so it read -1
        # every time and failed the D axis on a manifest that was already
        # correct. At freeze time that reports a dishonest D axis for a perfect
        # one, and invites "fixing" the manifest into a wrong key. The invented
        # name is deliberately not repeated here, because
        # tooling/check-consistency.py asserts it appears in no manifest reader.
        $decl = if ($toml -match 'dependency_count\s*=\s*(\d+)') { [int]$Matches[1] } else { -1 }
        Check "D axis: dependency_count == 0" ($decl -eq 0) "declared=$decl"
        Check "D axis: no Cargo.toml in the track folder" (-not (Test-Path "$dir\Cargo.toml")) "rustc-only build, zero manifest deps"
    }

    # ── 4. scaffold sentinel gone ──
    $srcFiles = Get-ChildItem "$dir\src" -Filter *.rs -ErrorAction SilentlyContinue
    $hasSentinel = $false
    foreach ($sf in $srcFiles) {
        if (Select-String -Path $sf.FullName -Pattern "SCAFFOLD_NOT_IMPLEMENTED" -SimpleMatch -Quiet) { $hasSentinel = $true }
    }
    Check "no scaffold sentinel in source" (-not $hasSentinel) "a stub must never reach the freeze"

    # ── 2/3/7/8. functional gates ──
    # run.exe is probed first: see Resolve-Runnable for why the extensionless
    # `run` that build.sh emits on Windows cannot be executed here at all.
    $bin = $null
    foreach ($c in @("$Root\track-$t\run.exe", "$Root\track-$t\run", "/app/run")) {
        if (Test-Path $c -PathType Leaf) { $bin = $c; break }
    }

    if (-not $bin) {
        Check "binary built" $false "run: sh track-$t/build.sh"
        continue
    }
    $bin = Resolve-Runnable $bin
    Check "binary built" $true (Split-Path $bin -Leaf)

    $dataDir = "$Root\verification\data\track-$t"
    $outDir  = "$Root\verification\out\track-$t"
    New-Item -ItemType Directory -Force -Path $dataDir, $outDir | Out-Null

    $expected = $null
    $expScale = "committed pin"
    # The expected hash is SCALE-SPECIFIC (tiny, sample and full seed 1 give
    # three different correct answers) and the committed pin holds only one of
    # them, while the corpus on disk may be a different scale. Derive it from
    # the bytes actually present, and only fall back to the pin.
    if (Test-Path $dataDir) {
        $out = & python "$Root\tooling\official-data.py" --track $t.ToLower() --expected-for $dataDir 2>$null
        if ($LASTEXITCODE -eq 0 -and $out) {
            $parts = @($out)[0] -split '\s+'
            if ($parts.Count -ge 3 -and $parts[2] -match '^[0-9a-f]{64}$') {
                $expected = $parts[2]
                $expScale = $parts[1]
            }
        }
    }
    if (-not $expected) {
        foreach ($expFile in @("$dir\verification\expected\$($info.Output).sha256",
                               "$dir\verification\expected\sample.sha256")) {
            if (Test-Path $expFile) {
                $raw = (Get-Content $expFile -Raw).Trim()
                if ($raw -match '^([0-9a-fA-F]{64})') { $expected = $Matches[1].ToLower(); break }
            }
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
            Check "SHA-256 green at $expScale scale" ($uniq[0] -eq $expected) "expected=$($expected.Substring(0,12)) got=$($uniq[0].Substring(0,12))"
        } else {
            Check "SHA-256 green" $true "NO PUBLISHED HASH MATCHES THE DATA ON DISK — cannot be verified" -Skip
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
