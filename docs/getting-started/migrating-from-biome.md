# Migrating from Biome YAML

Biome 2.5.15 does not format YAML: `biome format --stdin-file-path=sample.yaml --write`
reports that the formatter is disabled and leaves the input unchanged, and Biome's
[language support](https://biomejs.dev/introduction/language-support/) lists YAML
formatting as in progress. There is no Biome YAML output yet to map onto `[format]`
settings.

For a project that uses Biome for other languages, run `ryl format` on YAML files
and keep Biome on its supported files. Start with an empty `[format]` table:

```toml
[format]
```

See [formatter configuration](../formatter.md#configuration) to choose a style.
