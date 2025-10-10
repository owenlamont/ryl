#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required to run this script" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required to run this script" >&2
  exit 1
fi

if ! cargo llvm-cov nextest --summary-only >/dev/null 2>&1; then
  echo "cargo llvm-cov nextest --summary-only failed; inspect the output above for details." >&2
  exit 1
fi

prefix="${project_root}/"
tmp_json="$(mktemp)"
trap 'rm -f "$tmp_json"' EXIT

cargo llvm-cov report --json --output-path "$tmp_json" >/dev/null 2>&1 || {
  echo "Failed to generate coverage report." >&2
  exit 1
}

report=$(jq -r --arg prefix "$prefix" '
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
    | {file: (.filename | sub("^" + $prefix; "")),
       uncovered: [ .segments[]
                    | select(.[2] == 0 and .[3] == true and .[5] == false)
                    | .[0]
                  ] }
    | select(.uncovered | length > 0)
    | "\(.file):\(.uncovered | ranges | join(","))"
  ' "$tmp_json")

if [[ -z "$report" ]]; then
  echo "Coverage OK: no uncovered regions."
else
  echo "Uncovered regions (file:path line ranges):"
  printf '%s
' "$report"
fi
