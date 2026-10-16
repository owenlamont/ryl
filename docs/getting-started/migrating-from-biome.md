# Migrating from Biome YAML

Biome 2.5.15 does not format YAML. Running
`biome format --stdin-file-path=sample.yaml --write` reports that the formatter
is disabled and leaves the input unchanged.
Biome's [language support](https://biomejs.dev/introduction/language-support/)
lists YAML formatting as pending. There is no released Biome YAML output to
reproduce with `[format]` settings yet.

For a project that uses Biome for other languages, run `ryl format` on YAML files
and keep Biome on its supported files. Start with an empty `[format]` table:

```toml
[format]
```

See [formatter configuration](../formatter.md#configuration) to choose a style.
Biome's unreleased YAML documentation cannot establish a migration mapping;
compare real output again when a released Biome formatter accepts YAML.
