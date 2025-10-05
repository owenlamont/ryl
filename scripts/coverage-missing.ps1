#!/usr/bin/env pwsh
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Ensure-Command {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    if (-not (Get-Command -Name $Name -ErrorAction SilentlyContinue)) {
        [Console]::Error.WriteLine("$Name is required to run this script")
        exit 1
    }
}

function Write-Fatal {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Message
    )

    [Console]::Error.WriteLine($Message)
    exit 1
}

function ConvertTo-LineRanges {
    param(
        [int[]]$Lines
    )

    if (-not $Lines -or $Lines.Count -eq 0) {
        return @()
    }

    $sorted = $Lines | Sort-Object -Unique
    $ranges = @()
    $start = $null
    $previous = $null

    foreach ($line in $sorted) {
        if ($null -eq $start) {
            $start = $line
            $previous = $line
            continue
        }

        if ($line -eq ($previous + 1)) {
            $previous = $line
            continue
        }

        if ($start -eq $previous) {
            $ranges += "$start"
        } else {
            $ranges += "$start-$previous"
        }

        $start = $line
        $previous = $line
    }

    if ($null -ne $start) {
        if ($start -eq $previous) {
            $ranges += "$start"
        } else {
            $ranges += "$start-$previous"
        }
    }

    return $ranges
}

function Get-RelativePathNormalized {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Root,
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $relative = [System.IO.Path]::GetRelativePath($Root, $Path)
    return ($relative -replace '\\', '/')
}

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $scriptRoot) {
    $scriptRoot = Get-Location
}

$projectRoot = (Resolve-Path (Join-Path $scriptRoot '..')).Path

Ensure-Command -Name 'cargo'

$tmpFile = $null
$locationPushed = $false
try {
    Push-Location $projectRoot
    $locationPushed = $true

    & cargo llvm-cov nextest --summary-only | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Fatal 'cargo llvm-cov nextest --summary-only failed; inspect the output above for details.'
    }

    $tmpFile = [System.IO.Path]::GetTempFileName()

    & cargo llvm-cov report --json --output-path $tmpFile | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Fatal 'Failed to generate coverage report.'
    }

    $jsonContent = Get-Content -Path $tmpFile -Raw
    $report = $jsonContent | ConvertFrom-Json

    if (-not $report.data) {
        Write-Output 'Coverage OK: no uncovered regions.'
        return
    }

    $uncovered = @()

    foreach ($dataset in $report.data) {
        foreach ($file in $dataset.files) {
            if (-not $file.summary -or -not $file.summary.regions) {
                continue
            }

            $percent = $file.summary.regions.percent
            if ($percent -ge 100) {
                continue
            }

            if (-not $file.segments) {
                continue
            }

            $lines = @()
            foreach ($segment in $file.segments) {
                if ($segment[2] -eq 0 -and $segment[3] -eq $true -and $segment[5] -eq $false) {
                    $lines += [int]$segment[0]
                }
            }

            if ($lines.Count -eq 0) {
                continue
            }

            $ranges = ConvertTo-LineRanges -Lines $lines
            if ($ranges.Count -eq 0) {
                continue
            }

            $relativePath = Get-RelativePathNormalized -Root $projectRoot -Path $file.filename
            $uncovered += "${relativePath}:$($ranges -join ',')"
        }
    }

    if ($uncovered.Count -eq 0) {
        Write-Output 'Coverage OK: no uncovered regions.'
    } else {
        Write-Output 'Uncovered regions (file:path line ranges):'
        foreach ($entry in $uncovered) {
            Write-Output $entry
        }
    }
}
finally {
    if ($null -ne $tmpFile -and (Test-Path -LiteralPath $tmpFile)) {
        Remove-Item -LiteralPath $tmpFile -Force -ErrorAction SilentlyContinue
    }

    if ($locationPushed) {
        Pop-Location
    }
}
