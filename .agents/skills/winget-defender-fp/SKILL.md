---
name: winget-defender-fp
description: >-
  Use when a ryl winget-pkgs validation PR is blocked by a Microsoft Defender
  false positive (`Validation-Defender-Error` / "Installer failed security
  check"). Covers the transient-vs-reproducible decision, local Defender +
  VirusTotal reproduction, the WDSI false-positive submission, and the log
  noise to ignore.
---

# Winget Defender False-Positive Triage

ryl ships **unsigned** Windows binaries (`InstallerType: zip` + `NestedInstallerType:
portable`). Microsoft Defender's cloud/ML heuristics intermittently flag the *downloaded*
ZIP (mark-of-the-web) as a trojan, failing the winget-pkgs validation. The packaging is
correct — this is a false positive, not a ryl bug. Code signing is the durable fix but is
deferred (#342); this loop is the interim process.

## Recognise it

- Labels: **`Validation-Defender-Error`** is the root cause; `Validation-Installation-Error`
  and `Validation-Executable-Error` are downstream cascades of the same block;
  `Needs-Author-Feedback` means the PR is waiting on you (reply, or it can be auto-closed).
- The moderator's "… Validation ended with" comment names the **detection** and the
  **artifact** (usually the x64 zip), e.g. `[FAIL] Installer failed security check. Url:
  …/ryl-x86_64-pc-windows-msvc.zip … Detection: Trojan:Win32/Sprisky.U!cl`.
- The signature name **varies per release** (`Wacatac.H!ml`, `Sprisky.U!cl`, …); the
  `!ml`/`!cl` suffix means heuristic/cloud, not a content-signature match.
- Error codes: `0x8A15002D` = `APPINSTALLER_CLI_ERROR_INSTALLER_SECURITY_CHECK_FAILED`;
  `0x80004005` = `E_FAIL`.
- **Log noise to ignore** (always present, never the cause): the validator's WMI
  `wminet_utils.dll` `TypeInitializationException`, and `ryl.exe returned exit code: 2`
  (ryl's by-design no-args usage exit).

## Diagnose

The Azure validation pipeline is **public/anonymous**. From the wingetbot "Validation
Pipeline Run" link grab the `buildId`, then (project GUID
`8b78618a-7973-49d8-9174-4360829d979b`) read the timeline and the failed task's log (look
for the `Installation Validation` task):

```sh
base="https://dev.azure.com/shine-oss/8b78618a-7973-49d8-9174-4360829d979b/_apis/build/builds"
curl -s "$base/<buildId>/timeline?api-version=7.1"
curl -s "$base/<buildId>/logs/<logId>?api-version=7.1"
```

Reproduce locally (Windows, real-time protection ON — always record the signature build,
it is the key datum for the PR reply):

```powershell
$u = "https://github.com/owenlamont/ryl/releases/download/vX.Y.Z/ryl-x86_64-pc-windows-msvc.zip"
$zip = "$env:TEMP\ryl-check.zip"
Invoke-WebRequest $u -OutFile $zip
# Invoke-WebRequest does NOT set mark-of-the-web, so the heuristic that flags *downloaded*
# zips never fires on an IWR file. Tag it Internet-zone (3) with the real source to mimic a
# browser/winget download — without this the repro tests an unmarked file and proves nothing:
Set-Content -Path $zip -Stream Zone.Identifier -Value "[ZoneTransfer]`r`nZoneId=3`r`nHostUrl=$u"
Start-MpScan -ScanType CustomScan -ScanPath $zip   # file-at-rest scan; weaker than on-access
# THIS run only — Get-MpThreatDetection is cumulative, so a stale hit from a prior version
# reads as a current-version flag unless filtered by date:
Get-MpThreatDetection | Where-Object { $_.InitialDetectionTime -ge (Get-Date).Date } |
  Sort-Object InitialDetectionTime -Descending | Select-Object InitialDetectionTime, Resources
(Get-MpComputerStatus).AntivirusSignatureVersion
```

A clean `Start-MpScan` is reassuring but not conclusive: the `!ml`/`!cl` verdict is a
cloud/ML call on the real-time on-access/download path, which a file-at-rest custom scan
may not exercise — VirusTotal's Microsoft engine is the stronger corroboration.

Cross-check **VirusTotal** — its Microsoft engine is the best proxy for the validator.
Look up by hash first (`https://www.virustotal.com/gui/file/<sha256-lowercase>`); upload
if absent (anonymous, no login wall; upload + scan ~30s each). Check **all four** artifacts:
both zips **and** the unzipped `.exe`s — the bare exe usually scans clean; only the MOTW
download trips it. VT's GUI is shadow-DOM heavy: walk shadow roots, read the `Microsoft`
engine row, and rebuild the `n/NN` ratio from its DOM ancestors (no `positives`/`total`
attribute); let the "analysing" state clear before reading the final ratio.

## Decide

- **Both VT (Microsoft = Undetected) and local Defender clean** → the verdict has aged out
  / is environment-specific. **Do not file a WDSI submission** (Microsoft would likely
  reject it as "not detected"). Reply to `Needs-Author-Feedback` with the evidence —
  the Defender **signature build** tested, VT links, "Microsoft: Undetected" — and ask
  the assigned/active moderator to re-run validation (`@wingetbot run` is
  moderator-triggered — an author/maintainer comment does not reliably start a run; the
  bot also auto-retries ~every 18h).
  *(0.20.0 / `microsoft/winget-pkgs#391230`.)*
- **Still reproducibly flagged** (local Defender and/or VT's Microsoft engine) → file a
  WDSI false-positive submission, then wait for propagation + re-run.
  *(0.19.1 / `microsoft/winget-pkgs#390120`.)*

## WDSI submission (only when reproducible)

<https://www.microsoft.com/en-us/wdsi/filesubmission> → **Software developer** persona →
requires a Microsoft account sign-in **and a CAPTCHA** (human steps; not automatable).

- Submit the **flagged ZIP**. Defender blocks *copying* a quarantined file, so obtain an
  uploadable copy via a temporary exclusion: `Add-MpPreference -ExclusionPath <folder>`
  (admin), re-download into it, submit, then `Remove-MpPreference -ExclusionPath <folder>`.
- Fields: detection name, definition version, "Incorrectly detected as malware",
  company = your own name (ryl is a personal project, not an employer), notes cite the
  VT link.
- **"Accepted" ≠ propagated.** The per-submission details page can transiently error
  ("Unable to access your submission details") — that is not a failure; verify via the
  **submission history** list. After Microsoft clears it, propagation to the validator takes
  ~1–3 days; then re-run.

## Gotchas

- Cross-repo refs: always `microsoft/winget-pkgs#NNN`; a bare `#NNN` links to ryl (see the
  `filing-issues` skill).
- Never put a local home path or machine specifics in a public PR comment — use `$env:TEMP`
  or placeholders.
- **Delegating to a headless/WSL agent:** launch it with `cwd` = the ryl repo (so it loads
  this skill) and hand it the **full** step list (local Defender repro + VT for zips *and*
  exes + tie back to the PR). A prompt scoped to "scan the zips on VT" misses the
  reproduction and PR-triage half.

## History

- `microsoft/winget-pkgs#387703` (0.17.0, merged), `#390120` (0.19.1, `Wacatac.H!ml` —
  needed a WDSI submission + propagation), `#391230` (0.20.0, `Sprisky.U!cl` — transient;
  re-run requested, outcome pending as of writing).
- Durable fix: code signing (#342). Microsoft label reference:
  <https://github.com/microsoft/winget-pkgs/blob/master/doc/ValidationFailureGuide.md>.
