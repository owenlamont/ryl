# What ryl won't do

ryl lints general YAML, not the application that reads it, and its fixes never
change what a document means. Requests outside that line are declined, so check this page before filing
one.

## Rules for one YAML flavour

ryl has no built-in rules for GitHub Actions, Kubernetes, Ansible or any other
YAML dialect. Use the dialect's own linter alongside ryl:
[actionlint](https://github.com/rhysd/actionlint),
[kube-linter](https://github.com/stackrox/kube-linter) or
[ansible-lint](https://github.com/ansible/ansible-lint).

## Rules without evidence, or for contested style

A new rule needs more than one reputable source, such as the YAML 1.2.2 spec,
yaml.org, PyYAML's documentation or the yamllint maintainers, calling it best
practice. Blog posts alone don't count. Style with no broad consensus stays out,
for example:

- `null` versus `~`
- key-naming conventions
- flow versus block sequences
- exact blank-line counts

## Rules about application content

Rules check how a document is written and what YAML itself makes of it, such as
a truthy word, an implicit octal or a merge key overriding a value. They don't
check whether the values suit the application reading the file: sequence order,
allowed keys and value ranges belong to that tool. Mapping key order is form,
since YAML gives it no meaning, so `key-ordering` fits. ryl does no schema validation either: pair it with
[`yaml-language-server`](editor-integration.md) for that.

## Fixes that change meaning or reprint the document

`--fix` edits the document in place rather than reprinting it, and keeps its
resolved value, comments and anchors. That rules out:

- **Fixes that guess intent**, such as rewriting `no` to `false`: the author may
  have meant the string. One would only ever sit behind an explicit opt-in.
- **Whole-document reprinting**, the way Prettier formats YAML. Use a
  reprinting formatter if that is what you want; ryl won't become one.
