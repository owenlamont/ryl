#!/usr/bin/env pwsh
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Require-Command([string]$Name) {
    if (-not (Get-Command -Name $Name -ErrorAction SilentlyContinue)) {
        throw "$Name is required to run this script"
    }
}

function Format-LineRanges([int[]]$Lines) {
    if (-not $Lines) { return @() }
    $sorted = $Lines | Sort-Object -Unique
    $start = $sorted[0]
    $prev = $start
    $ranges = @()
    for ($i = 1; $i -lt $sorted.Count; $i++) {
        $current = $sorted[$i]
        if ($current -ne $prev + 1) {
            $ranges += if ($start -eq $prev) { "$start" } else { "$start-$prev" }
            $start = $current
        }
        $prev = $current
    }
    if ($start -eq $prev) {
        $ranges += "$start"
    } else {
        $ranges += "$start-$prev"
    }
    return $ranges
}

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $scriptRoot) { $scriptRoot = Get-Location }
$projectRoot = (Resolve-Path (Join-Path $scriptRoot '..')).Path

Require-Command 'cargo'

$tmp = $null
$locationPushed = $false
try {
    Push-Location $projectRoot
    $locationPushed = $true

    & cargo llvm-cov nextest --summary-only *> $null
    if ($LASTEXITCODE -ne 0) {
        throw 'cargo llvm-cov nextest --summary-only failed; inspect the output above for details.'
    }

    $tmp = [System.IO.Path]::GetTempFileName()

    & cargo llvm-cov report --json --output-path $tmp *> $null
    if ($LASTEXITCODE -ne 0) {
        throw 'Failed to generate coverage report.'
    }

    $report = Get-Content -Path $tmp -Raw | ConvertFrom-Json
    if (-not $report.data) {
        Write-Output 'Coverage OK: no uncovered regions.'
        return
    }

    $entries = @()
    foreach ($dataset in $report.data) {
        foreach ($file in $dataset.files) {
            if (-not ($file.summary -and $file.summary.regions) -or $file.summary.regions.percent -ge 100) { continue }
            if (-not $file.segments) { continue }
            $lines = foreach ($segment in $file.segments) {
                if ($segment[2] -eq 0 -and $segment[3] -and -not $segment[5]) { [int]$segment[0] }
            }
            if (-not $lines) { continue }
            $ranges = Format-LineRanges -Lines $lines
            if (-not $ranges) { continue }
            $path = [System.IO.Path]::GetRelativePath($projectRoot, $file.filename) -replace '\\', '/'
            $entries += [pscustomobject]@{ Path = $path; Ranges = $ranges }
        }
    }

    if (-not $entries) {
        Write-Output 'Coverage OK: no uncovered regions.'
        return
    }

    Write-Output 'Uncovered regions (file:path line ranges):'
    foreach ($entry in $entries) {
        Write-Output ("{0}:{1}" -f $entry.Path, ($entry.Ranges -join ','))
    }
}
finally {
    if ($tmp -and (Test-Path -LiteralPath $tmp)) {
        Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
    }
    if ($locationPushed) { Pop-Location }
}
