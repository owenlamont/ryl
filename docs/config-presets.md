# ryl config presets

## Applying a preset

Create a starter config in the current directory:

```bash
ryl init                   # default preset
ryl init --preset relaxed
ryl init --preset kubernetes
```

`ryl init` writes `.ryl.toml` with the selected preset and the ryl version in a
header comment. Edit the generated TOML to customise the rules. The command refuses
an existing project config in the current directory, including yamllint configs;
use `ryl --migrate-configs` to preview migration and add `--migrate-write` to apply it.
An unrelated `pyproject.toml` is allowed. An ancestor config is allowed too: ryl
prints a note that the new config takes over for the subtree without inheritance.
Success exits `0`; refusals and I/O failures exit `2`. There is no overwrite flag.

ryl's TOML config (recommended) is the single, explicit source of a file's
rules: it has no `preset` or `extends` key, inherits nothing, and merges
nothing, so what it enables is the file's whole ruleset, with no default-on
rules.

ryl resolves one config per file by searching upward from the file. A TOML
config (`.ryl.toml`, `ryl.toml`, `.config/.ryl.toml`, `.config/ryl.toml`, or
`pyproject.toml` `[tool.ryl]`, in that precedence order) found anywhere up the
tree is preferred over a `.yamllint`, even a nearer one; among configs of the
same kind the nearest wins. A monorepo can therefore hold many `ryl.toml`
files, one per subtree, each governing its own files. If the upward search
finds no project config, ryl falls back to a single user-global config; see the
[quick start](getting-started/quickstart.md).

Presets are starting points for that config, not something ryl inherits behind
the scenes:

- **TOML config (recommended):** there is no `preset` or `extends` key. The
  tables below *are* the presets written by `ryl init`. The generated file
  is your one explicit config.
- **YAML config (deprecated):** a YAML config may `extends:` a
  built-in preset (`default`, `relaxed`, `empty`) or another config file, and
  ryl merges the inherited settings in (with overrides under `rules:`). This
  inheritance is available in YAML config; TOML config is self-contained. See
  [Migrating from yamllint](getting-started/migrating-from-yamllint.md) to
  convert a YAML config to TOML.

The `default` and `relaxed` TOML presets mirror their built-in YAML equivalents.
The eight ecosystem presets below expand `default` with sourced changes; each is
standalone. All allow omitted initial document markers and keep truthy warnings.
Only Ansible enables octal checks. Presets configure YAML linting and formatting;
use ecosystem tools for application semantics and schema validation.

## `default` (TOML equivalent)

```toml
[files]
yaml = [
    "*.yaml",
    "*.yml",
    ".yamllint",
]

[lint.rules]
anchors = "enable"
braces = "enable"
brackets = "enable"
colons = "enable"
commas = "enable"
document-end = "disable"
empty-lines = "enable"
empty-values = "disable"
float-values = "disable"
hyphens = "enable"
indentation = "enable"
key-duplicates = "enable"
key-ordering = "disable"
line-length = "enable"
new-line-at-end-of-file = "enable"
new-lines = "enable"
octal-values = "disable"
quoted-strings = "disable"
trailing-spaces = "enable"

[lint.rules.comments]
level = "warning"

[lint.rules.comments-indentation]
level = "warning"

[lint.rules.document-start]
level = "warning"

[lint.rules.truthy]
level = "warning"
```

## `relaxed` (TOML equivalent, fully expanded)

```toml
[files]
yaml = [
    "*.yaml",
    "*.yml",
    ".yamllint",
]

[lint.rules]
anchors = "enable"
comments = "disable"
comments-indentation = "disable"
document-end = "disable"
document-start = "disable"
empty-values = "disable"
float-values = "disable"
key-duplicates = "enable"
key-ordering = "disable"
new-line-at-end-of-file = "enable"
new-lines = "enable"
octal-values = "disable"
quoted-strings = "disable"
trailing-spaces = "enable"
truthy = "disable"

[lint.rules.braces]
level = "warning"
max-spaces-inside = 1

[lint.rules.brackets]
level = "warning"
max-spaces-inside = 1

[lint.rules.colons]
level = "warning"

[lint.rules.commas]
level = "warning"

[lint.rules.empty-lines]
level = "warning"

[lint.rules.hyphens]
level = "warning"

[lint.rules.indentation]
level = "warning"
indent-sequences = "consistent"

[lint.rules.line-length]
level = "warning"
allow-non-breakable-inline-mappings = true
```

## `ansible`

Use for Ansible playbooks and roles, following ansible-lint’s shipped YAML settings.

```toml
# Ansible playbooks and roles
[files]
yaml = [
    "*.yaml",
    "*.yml",
    ".yamllint",
]

[lint.rules]
# ansible-lint compatibility: https://github.com/ansible/ansible-lint/blob/main/src/ansiblelint/data/.yamllint
document-start = "disable"
# ansible-lint compatibility: https://github.com/ansible/ansible-lint/blob/main/src/ansiblelint/data/.yamllint
comments-indentation = "disable"
anchors = "enable"
brackets = "enable"
colons = "enable"
commas = "enable"
document-end = "disable"
empty-lines = "enable"
empty-values = "disable"
float-values = "disable"
hyphens = "enable"
indentation = "enable"
key-duplicates = "enable"
key-ordering = "disable"
new-line-at-end-of-file = "enable"
new-lines = "enable"
quoted-strings = "disable"
trailing-spaces = "enable"

[lint.rules.truthy]
level = "warning"

[lint.rules.comments]
level = "warning"
# ansible-lint compatibility: https://github.com/ansible/ansible-lint/blob/main/src/ansiblelint/data/.yamllint
min-spaces-from-content = 1

[lint.rules.line-length]
# ansible-lint compatibility: https://github.com/ansible/ansible-lint/blob/main/src/ansiblelint/data/.yamllint
max = 160

[lint.rules.braces]
# ansible-lint compatibility: https://github.com/ansible/ansible-lint/blob/main/src/ansiblelint/data/.yamllint
max-spaces-inside = 1

[lint.rules.octal-values]
# ansible-lint compatibility: https://github.com/ansible/ansible-lint/blob/main/src/ansiblelint/data/.yamllint
level = "error"

[format]
# ansible-lint compatibility: https://github.com/ansible/ansible-lint/blob/main/src/ansiblelint/data/.yamllint
comment-spacing = 1
# ansible-lint compatibility: https://github.com/ansible/ansible-lint/blob/main/src/ansiblelint/data/.yamllint
brace-spacing = true
```

Sources:

- [Upstream configuration or example](https://github.com/ansible/ansible-lint/blob/main/src/ansiblelint/data/.yamllint)

## `cloudformation`

Use for CloudFormation and SAM templates. The anchor setting rejects both anchors
and aliases, broader than AWS’s alias-only restriction. Merge keys and removed
standard tags are also rejected; custom intrinsic tags remain unrestricted.

```toml
# CloudFormation and SAM templates
[files]
yaml = [
    "*.yaml",
    "*.yml",
    ".yamllint",
]

[lint.rules]
# Upstream examples omit initial markers: https://github.com/awslabs/aws-cloudformation-templates/blob/main/S3/compliant-bucket.yaml
document-start = "disable"
braces = "enable"
brackets = "enable"
colons = "enable"
commas = "enable"
document-end = "disable"
empty-lines = "enable"
empty-values = "disable"
float-values = "disable"
hyphens = "enable"
indentation = "enable"
key-duplicates = "enable"
key-ordering = "disable"
# Accommodate long templates: https://github.com/awslabs/aws-cloudformation-templates/blob/main/S3/compliant-bucket.yaml
line-length = "disable"
new-line-at-end-of-file = "enable"
new-lines = "enable"
octal-values = "disable"
quoted-strings = "disable"
trailing-spaces = "enable"

[lint.rules.comments]
level = "warning"

[lint.rules.comments-indentation]
level = "warning"

[lint.rules.truthy]
level = "warning"

[lint.rules.anchors]
# Reject aliases (also bans anchors): https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/template-formats.html
forbid-anchors-and-aliases = true

[lint.rules.merge-keys]
# AWS template restrictions: https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/template-formats.html
level = "error"

[lint.rules.tags]
# AWS template restrictions: https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/template-formats.html
forbid-removed-types = true
```

Sources:

- [Upstream configuration or example](https://github.com/awslabs/aws-cloudformation-templates/blob/main/S3/compliant-bucket.yaml)
- [Upstream guide](https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/template-formats.html)

## `home-assistant`

Use for Home Assistant configurations and blueprints, with registered include,
secret, environment, and input tags. Double quotes are preferred for string values;
ordinary quoting stays optional because the guide exempts identifiers and fixed-choice
values. Template strings containing `{{` or `{%` require double quotes. Keys are
exempt. Field-specific quoting requirements need application-aware tooling.

```toml
# Home Assistant configurations and blueprints
[files]
yaml = [
    "*.yaml",
    "*.yml",
    ".yamllint",
]

[lint.rules]
# Upstream examples omit initial markers: https://github.com/home-assistant/core/blob/dev/homeassistant/components/automation/blueprints/motion_light.yaml
document-start = "disable"
anchors = "enable"
brackets = "enable"
colons = "enable"
commas = "enable"
document-end = "disable"
empty-lines = "enable"
empty-values = "disable"
float-values = "disable"
hyphens = "enable"
key-duplicates = "enable"
key-ordering = "disable"
# Home Assistant YAML style: https://github.com/home-assistant/developers.home-assistant/blob/master/docs/documenting/yaml-style-guide.md
line-length = "disable"
new-line-at-end-of-file = "enable"
new-lines = "enable"
octal-values = "disable"
trailing-spaces = "enable"

[lint.rules.comments-indentation]
level = "warning"

[lint.rules.truthy]
level = "warning"

[lint.rules.indentation]
# Home Assistant YAML style: https://github.com/home-assistant/developers.home-assistant/blob/master/docs/documenting/yaml-style-guide.md
spaces = 2

[lint.rules.comments]
level = "warning"
# Home Assistant YAML style: https://github.com/home-assistant/developers.home-assistant/blob/master/docs/documenting/yaml-style-guide.md
min-spaces-from-content = 1

[lint.rules.braces]
# Home Assistant YAML style: https://github.com/home-assistant/developers.home-assistant/blob/master/docs/documenting/yaml-style-guide.md
forbid = "non-empty"

[lint.rules.quoted-strings]
# Home Assistant YAML style: https://github.com/home-assistant/developers.home-assistant/blob/master/docs/documenting/yaml-style-guide.md
quote-type = "double"
# Home Assistant YAML style: https://github.com/home-assistant/developers.home-assistant/blob/master/docs/documenting/yaml-style-guide.md
required = false
# Home Assistant YAML style: https://github.com/home-assistant/developers.home-assistant/blob/master/docs/documenting/yaml-style-guide.md
allow-quoted-quotes = false
# Template strings require quotes: https://github.com/home-assistant/developers.home-assistant/blob/master/docs/documenting/yaml-style-guide.md
extra-required = ['\{\{|\{%']
# Apply string guidance to values: https://github.com/home-assistant/developers.home-assistant/blob/master/docs/documenting/yaml-style-guide.md
check-keys = false

[lint.rules.tags]
# Registered configuration tags: https://github.com/home-assistant-libs/annotatedyaml/blob/main/src/annotatedyaml/loader.py
allowed-tags = ["!include", "!include_dir_list", "!include_dir_named", "!include_dir_merge_list", "!include_dir_merge_named", "!secret", "!env_var", "!input"]

[format]
# Home Assistant YAML style: https://github.com/home-assistant/developers.home-assistant/blob/master/docs/documenting/yaml-style-guide.md
comment-spacing = 1
# Home Assistant YAML style: https://github.com/home-assistant/developers.home-assistant/blob/master/docs/documenting/yaml-style-guide.md
quote-style = "double"
# Home Assistant YAML style: https://github.com/home-assistant/developers.home-assistant/blob/master/docs/documenting/yaml-style-guide.md
keep-quotes = true
# Home Assistant YAML style: https://github.com/home-assistant/developers.home-assistant/blob/master/docs/documenting/yaml-style-guide.md
mapping-style = "block"
# Home Assistant YAML style: https://github.com/home-assistant/developers.home-assistant/blob/master/docs/documenting/yaml-style-guide.md
sequence-style = "block"
```

Sources:

- [Upstream configuration or example](https://github.com/home-assistant/core/blob/dev/homeassistant/components/automation/blueprints/motion_light.yaml)
- [Upstream configuration or example](https://github.com/home-assistant/developers.home-assistant/blob/master/docs/documenting/yaml-style-guide.md)
- [Upstream configuration or example](https://github.com/home-assistant-libs/annotatedyaml/blob/main/src/annotatedyaml/loader.py)

## `kubernetes`

Use for Kubernetes and Kustomize manifests, allowing either sequence indentation
and rejecting nonfinite floats that cannot pass JSON conversion.

```toml
# Kubernetes and Kustomize manifests
[files]
yaml = [
    "*.yaml",
    "*.yml",
    ".yamllint",
]

[lint.rules]
# Upstream examples omit initial markers: https://github.com/kubernetes/website/blob/main/content/en/examples/controllers/nginx-deployment.yaml
document-start = "disable"
anchors = "enable"
braces = "enable"
brackets = "enable"
colons = "enable"
commas = "enable"
document-end = "disable"
empty-lines = "enable"
empty-values = "disable"
hyphens = "enable"
key-duplicates = "enable"
key-ordering = "disable"
line-length = "enable"
new-line-at-end-of-file = "enable"
new-lines = "enable"
octal-values = "disable"
quoted-strings = "disable"
trailing-spaces = "enable"

[lint.rules.comments]
level = "warning"

[lint.rules.comments-indentation]
level = "warning"

[lint.rules.truthy]
level = "warning"

[lint.rules.indentation]
# Kubernetes manifest conventions: https://github.com/kubernetes/website/blob/main/content/en/examples/controllers/nginx-deployment.yaml
indent-sequences = "whatever"

[lint.rules.float-values]
# JSON conversion rejects nonfinite floats: https://github.com/kubernetes-sigs/yaml/blob/master/yaml.go
forbid-nan = true
# JSON conversion rejects nonfinite floats: https://github.com/kubernetes-sigs/yaml/blob/master/yaml.go
forbid-inf = true
```

Sources:

- [Upstream configuration or example](https://github.com/kubernetes/website/blob/main/content/en/examples/controllers/nginx-deployment.yaml)
- [Upstream configuration or example](https://github.com/kubernetes-sigs/yaml/blob/master/yaml.go)

## `github-actions`

Use for GitHub Actions workflows. Truthy values still warn; keys, including `on:`,
are exempt. Merge keys are rejected; anchors and aliases remain available.

```toml
# GitHub Actions workflows
[files]
yaml = [
    "*.yaml",
    "*.yml",
    ".yamllint",
]

[lint.rules]
# Upstream examples omit initial markers: https://github.com/actions/starter-workflows/blob/main/ci/node.js.yml
document-start = "disable"
anchors = "enable"
braces = "enable"

colons = "enable"
commas = "enable"
document-end = "disable"
empty-lines = "enable"
empty-values = "disable"
float-values = "disable"
hyphens = "enable"

key-duplicates = "enable"
key-ordering = "disable"
# Starter workflow conventions: https://github.com/actions/starter-workflows/blob/main/ci/node.js.yml
line-length = "disable"
new-line-at-end-of-file = "enable"
new-lines = "enable"
octal-values = "disable"
quoted-strings = "disable"
trailing-spaces = "enable"

[lint.rules.comments]
level = "warning"

[lint.rules.comments-indentation]
level = "warning"

[lint.rules.truthy]
level = "warning"
# Starter workflow conventions: https://github.com/actions/starter-workflows/blob/main/ci/node.js.yml
check-keys = false

[lint.rules.brackets]
# Starter workflow conventions: https://github.com/actions/starter-workflows/blob/main/ci/node.js.yml
max-spaces-inside = 1

[lint.rules.indentation]
# Starter workflow conventions: https://github.com/actions/starter-workflows/blob/main/ci/node.js.yml
indent-sequences = "whatever"

[lint.rules.merge-keys]
# Workflow merge keys unsupported: https://github.com/rhysd/actionlint/blob/main/parse.go
level = "error"
```

Sources:

- [Upstream configuration or example](https://github.com/actions/starter-workflows/blob/main/ci/node.js.yml)
- [Upstream configuration or example](https://github.com/rhysd/actionlint/blob/main/parse.go)

## `compose`

Use for Docker Compose files, including `!reset` and `!override` merge tags.

```toml
# Docker Compose files
[files]
yaml = [
    "*.yaml",
    "*.yml",
    ".yamllint",
]

[lint.rules]
# Upstream examples omit initial markers: https://github.com/docker/awesome-compose/blob/master/wordpress-mysql/compose.yaml
document-start = "disable"
anchors = "enable"
braces = "enable"
brackets = "enable"
colons = "enable"
commas = "enable"
document-end = "disable"
empty-lines = "enable"
empty-values = "disable"
float-values = "disable"
hyphens = "enable"
indentation = "enable"
key-duplicates = "enable"
key-ordering = "disable"
line-length = "enable"
new-line-at-end-of-file = "enable"
new-lines = "enable"
octal-values = "disable"
quoted-strings = "disable"
trailing-spaces = "enable"

[lint.rules.comments]
level = "warning"

[lint.rules.comments-indentation]
level = "warning"

[lint.rules.truthy]
level = "warning"

[lint.rules.tags]
# Compose merge tags: https://docs.docker.com/reference/compose-file/merge/
allowed-tags = ["!reset", "!override"]
```

Sources:

- [Upstream configuration or example](https://github.com/docker/awesome-compose/blob/master/wordpress-mysql/compose.yaml)
- [Upstream guide](https://docs.docker.com/reference/compose-file/merge/)

## `helm`

Use for Helm `values.yaml` and `Chart.yaml`, excluding templates. String values
must be quoted, using either quote type; mapping keys are exempt. The values guide
calls for quoting strings without prescribing a quote type. Existing quotes are
preserved by the formatter.

```toml
# Helm values.yaml and Chart.yaml files
[files]
# Helm chart conventions: https://helm.sh/docs/chart_best_practices/conventions/
yaml = ["values.yaml", "Chart.yaml"]

[lint.rules]
# Accommodate chart examples: https://github.com/prometheus-community/helm-charts/blob/main/charts/prometheus/values.yaml
comments-indentation = "disable"
# Upstream examples omit initial markers: https://github.com/prometheus-community/helm-charts/blob/main/charts/prometheus/Chart.yaml
document-start = "disable"
anchors = "enable"
braces = "enable"
brackets = "enable"
colons = "enable"
commas = "enable"
document-end = "disable"
empty-lines = "enable"
empty-values = "disable"
float-values = "disable"
hyphens = "enable"
key-duplicates = "enable"
key-ordering = "disable"
# Accommodate chart examples: https://github.com/prometheus-community/helm-charts/blob/main/charts/prometheus/values.yaml
line-length = "disable"
new-line-at-end-of-file = "enable"
new-lines = "enable"
octal-values = "disable"
trailing-spaces = "enable"

[lint.rules.comments]
level = "warning"

[lint.rules.truthy]
level = "warning"

[lint.rules.indentation]
# Helm chart conventions: https://helm.sh/docs/chart_best_practices/conventions/
spaces = 2
# Accommodate chart examples: https://github.com/prometheus-community/helm-charts/blob/main/charts/prometheus/values.yaml
indent-sequences = "whatever"

[format]
# Quote string values: https://helm.sh/docs/chart_best_practices/values/
keep-quotes = true

[lint.rules.quoted-strings]
# Quote string values: https://helm.sh/docs/chart_best_practices/values/
quote-type = "any"
# Quote string values: https://helm.sh/docs/chart_best_practices/values/
required = true
# Quote string values: https://helm.sh/docs/chart_best_practices/values/
check-keys = false
```

Sources:

- [Upstream guide](https://helm.sh/docs/chart_best_practices/conventions/)
- [Upstream configuration or example](https://github.com/prometheus-community/helm-charts/blob/main/charts/prometheus/values.yaml)
- [Upstream configuration or example](https://github.com/prometheus-community/helm-charts/blob/main/charts/prometheus/Chart.yaml)
- [Upstream guide](https://helm.sh/docs/chart_best_practices/values/)

## `gitlab-ci`

Use for GitLab CI pipelines, including the native `!reference` tag.

```toml
# GitLab CI pipelines
[files]
yaml = [
    "*.yaml",
    "*.yml",
    ".yamllint",
]

[lint.rules]
# Upstream examples omit initial markers: https://github.com/gitlabhq/gitlabhq/blob/master/lib/gitlab/ci/templates/Nodejs.gitlab-ci.yml
document-start = "disable"
anchors = "enable"
braces = "enable"
brackets = "enable"
colons = "enable"
commas = "enable"
document-end = "disable"
empty-lines = "enable"
empty-values = "disable"
float-values = "disable"
hyphens = "enable"
indentation = "enable"
key-duplicates = "enable"
key-ordering = "disable"
# GitLab CI conventions: https://docs.gitlab.com/development/cicd/templates/
line-length = "disable"
new-line-at-end-of-file = "enable"
new-lines = "enable"
octal-values = "disable"
quoted-strings = "disable"
trailing-spaces = "enable"

[lint.rules.comments]
level = "warning"

[lint.rules.comments-indentation]
level = "warning"

[lint.rules.truthy]
level = "warning"

[lint.rules.tags]
# Native reference tag: https://docs.gitlab.com/ci/yaml/yaml_optimization/
allowed-tags = ["!reference"]
```

Sources:

- [Upstream configuration or example](https://github.com/gitlabhq/gitlabhq/blob/master/lib/gitlab/ci/templates/Nodejs.gitlab-ci.yml)
- [Upstream guide](https://docs.gitlab.com/development/cicd/templates/)
- [Upstream guide](https://docs.gitlab.com/ci/yaml/yaml_optimization/)

## `empty`

There is no usable TOML equivalent: ryl requires at least one rule to be
enabled, so an empty `[lint.rules]` table is rejected with "configuration enables no
rules". The `empty` preset survives only as a base to `extends:` in YAML config;
in TOML, list the rules you want under `[lint.rules]` directly.
