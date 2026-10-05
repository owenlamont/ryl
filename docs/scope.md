# What ryl won't do

ryl lints the form of general YAML, and its fixes never change what a document
means. Requests outside that line are declined, so check this page before filing
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

## Rules about content

Rules check how a document is written, not what its values say. Mapping key
order is form, since YAML gives it no meaning, so `key-ordering` fits. Sequence
order, allowed keys and value ranges are content, and belong to the tool that
consumes the file. ryl does no schema validation either: pair it with
[`yaml-language-server`](editor-integration.md) for that.

## Fixes that change meaning or reprint the document

`--fix` edits the document in place rather than reprinting it, and keeps its
resolved value, comments and anchors. That rules out:

- **Fixes that guess intent**, such as rewriting `no` to `false`: the author may
  have meant the string. One would only ever sit behind an explicit opt-in.
- **Whole-document reprinting**, the way Prettier formats YAML. Use a
  reprinting formatter if that is what you want; ryl won't become one.
