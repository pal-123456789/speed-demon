param(
  <#
    Peak working-set sampler for a track binary.

    cgroup memory.peak is what the board scores, and it only exists inside a
    container. Outside one, the closest local proxy is the process's peak working
    set, which on this project has tracked the board's figure closely (Track C:
    295.8 MB local vs 386 MB board; Track B: 138.1 MB vs 132 MB; Track A: 47.8 MB
    vs 64 MB). It is a proxy, not the same counter, so treat it as directional.

    Usage:
      .\tooling\peakwss.ps1 -Exe track-c\run -DataDir D:\sd-data\track-c `
                            -Output aggregates.txt -Track c
  #>
  [Parameter(Mandatory=$true)][string]$Exe,
  [Parameter(Mandatory=$true)][string]$DataDir,
  [Parameter(Mandatory=$true)][string]$Output,
  [string]$Track = '?',
  [string]$OutDir = "$env:TEMP\sd-peak",
  [int]$IntervalMs = 15
)
$env:SD_DATA_DIR = $DataDir
$env:SD_OUT_DIR  = $OutDir
if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$p = Start-Process -FilePath $Exe -PassThru -NoNewWindow
$peak = 0L
$samples = New-Object System.Collections.Generic.List[double]
while (-not $p.HasExited) {
    try {
        $ws = $p.WorkingSet64
        if ($ws -gt $peak) { $peak = $ws }
        $samples.Add([math]::Round($ws / 1MB, 1))
    } catch {}
    Start-Sleep -Milliseconds $IntervalMs
    $p.Refresh()
}

# The SHAPE matters as much as the number: a peak during the scan and a peak at
# merge time call for completely different fixes, and only the profile tells you
# which you have.
$n = $samples.Count
Write-Output ("track               = {0}" -f $Track)
Write-Output ("peak_working_set_MB = {0}" -f [math]::Round($peak / 1MB, 1))
Write-Output ("samples             = {0}" -f $n)
if ($n -ge 12) {
    $head = ($samples[0..5] | ForEach-Object { $_ }) -join ', '
    $mid  = ($samples[[int]($n/2)..([int]($n/2)+5)] | ForEach-Object { $_ }) -join ', '
    $tail = ($samples[($n-6)..($n-1)] | ForEach-Object { $_ }) -join ', '
    Write-Output ("first 6 (MB)        = {0}" -f $head)
    Write-Output ("middle 6 (MB)       = {0}" -f $mid)
    Write-Output ("last 6 (MB)         = {0}" -f $tail)
}

$f = Join-Path $OutDir $Output
if (Test-Path $f) {
    Write-Output ("sha256              = " + (Get-FileHash $f -Algorithm SHA256).Hash.ToLower())
} else {
    Write-Output "sha256              = (no output file)"
}
