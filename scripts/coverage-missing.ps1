#!/usr/bin/env pwsh
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Fail([string]$Message) {
    [Console]::Error.WriteLine($Message)
    exit 1
}

function Require-Command([string]$Name) {
    if (-not (Get-Command -Name $Name -ErrorAction SilentlyContinue)) {
        Fail "$Name is required to run this script"
    }
}

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $scriptRoot) { $scriptRoot = Get-Location }
$projectRoot = (Resolve-Path (Join-Path $scriptRoot '..')).Path
$normalizedRoot = $projectRoot -replace '\\', '/'
if (-not $normalizedRoot.EndsWith('/')) { $normalizedRoot += '/' }

Require-Command 'cargo'
Require-Command 'jq'

$tmp = $null
$locationPushed = $false
try {
    Push-Location $projectRoot
    $locationPushed = $true

    & cargo llvm-cov nextest --summary-only *> $null
    if ($LASTEXITCODE -ne 0) {
        Fail 'cargo llvm-cov nextest --summary-only failed; inspect the output above for details.'
    }

    $tmp = [System.IO.Path]::GetTempFileName()

    & cargo llvm-cov report --json --output-path $tmp *> $null
    if ($LASTEXITCODE -ne 0) {
        Fail 'Failed to generate coverage report.'
    }

    $jqFilter = @'
def ranges:
  sort
  | unique
  | reduce .[] as $line ([];
      if length > 0 and $line == (.[-1][1] + 1) then
        (.[-1] = [.[-1][0], $line])
      else
        . + [[ $line, $line ]]
      end)
  | map(if .[0] == .[1] then (.[0] | tostring) else "\(.[0])-\(.[1])" end);

.data[].files[]
| select(.summary.regions.percent < 100)
| {file: (.filename | gsub("\\\\"; "/") | sub("^" + $prefix; "")),
   uncovered: [ .segments[]
                | select(.[2] == 0 and .[3] == true and .[5] == false)
                | .[0]
              ] }
| select(.uncovered | length > 0)
| "\(.file):\(.uncovered | ranges | join(","))"
'@

    $report = (& jq -r --arg prefix $normalizedRoot $jqFilter $tmp)

    if ([string]::IsNullOrWhiteSpace($report)) {
        Write-Output 'Coverage OK: no uncovered regions.'
    } else {
        Write-Output 'Uncovered regions (file:path line ranges):'
        $report | ForEach-Object { Write-Output $_ }
    }
}
finally {
    if ($tmp -and (Test-Path -LiteralPath $tmp)) {
        Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
    }
    if ($locationPushed) { Pop-Location }
}
