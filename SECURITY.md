# Security Policy

`ryl` is a **personal open-source project**. Security reports are genuinely
welcome and taken seriously, but there is **no service-level agreement and no
guaranteed response time** — fixes are made on a best-effort basis.

## Reporting a vulnerability

**Please report vulnerabilities privately — do not open a public issue, pull
request, or discussion.**

Use GitHub's private vulnerability reporting:

1. Open the [Security tab](https://github.com/owenlamont/ryl/security) of the
   repository.
2. Click **"Report a vulnerability"**.
3. Fill in the advisory form.

This creates a private channel visible only to the maintainer and you. If you cannot
use it, contact the maintainer privately via the email on their GitHub profile — but
the GitHub channel is strongly preferred.

### What to include

- The `ryl` version (`ryl --version`) and how it was installed (cargo / pip / npm).
- A minimal reproducer: the YAML and/or config file(s) and the exact command.
- The impact (crash, hang, memory/CPU exhaustion, unexpected file write, CI or
  terminal output injection, …).

## Supported versions

Only the **latest released version** is supported. `ryl` is pre-1.0 and there are no
security backports to older versions — please upgrade to the latest release.

| Version | Supported |
| ------- | --------- |
| latest  | ✅        |
| older   | ❌        |

## Response expectations

Best-effort only. As a rough guide the maintainer aims to acknowledge a report
within 28 days, but **cannot commit to that or to any fix timeline** — this is
a project maintained in spare time. Reports are triaged and addressed as capacity
allows; thank you for your patience.

## Coordinated disclosure

Please give the maintainer a reasonable opportunity to release a fix before
disclosing a vulnerability publicly. When a fix ships, a
[GitHub Security Advisory](https://github.com/owenlamont/ryl/security/advisories)
is published (and, where relevant, a CVE requested and a
[RustSec](https://rustsec.org/) advisory filed so `cargo audit` users are alerted).
Reporters are credited unless they ask otherwise.

## Threat model and scope

`ryl` routinely processes **untrusted input**: YAML files and auto-discovered
configuration (`.yamllint`, `.ryl.toml`, `pyproject.toml`) from a repository, often
in CI on contributor branches.

**In scope** (please report):

- Denial of service — excessive memory or CPU, hangs, or crashes/panics — triggered
  by a crafted YAML file or configuration.
- `--fix` writing to a file other than the one being linted.
- Injection into CI or terminal output (e.g. GitHub Actions workflow-command
  injection, terminal escape sequences) via crafted file contents or filenames.

**Out of scope** (generally not treated as vulnerabilities):

- Resource use merely proportional to input size — a multi-gigabyte input being slow
  is expected, not a vulnerability.
- Issues that require the attacker to already control the environment `ryl` runs in,
  such as a symlink you created yourself, a symlinked parent directory of an
  explicitly-named path, or hostile environment variables (e.g.
  `YAMLLINT_FILE_ENCODING`).

## Vulnerabilities `ryl` finds in others

When auditing `ryl` surfaces a vulnerability in a dependency or another project, the
same coordinated, private disclosure asked for here is practised outward: the
upstream maintainer is contacted privately and given time to fix before any public
mention.
