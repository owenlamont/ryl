# JSON Schema fixtures

- `github-workflow.json`, `pre-commit-config.json`, `pre-commit-hooks.json`:
  [SchemaStore](https://github.com/SchemaStore/schemastore/tree/master/src/schemas/json),
  Apache-2.0, copyright SchemaStore contributors.
- `compose-spec.json`: [Compose specification](https://github.com/compose-spec/compose-spec/blob/main/schema/compose-spec.json),
  Apache-2.0, copyright Compose Specification contributors.

The pre-commit schemas use relative `$id` values so sibling references remain local.
Schema `$id` values identify upstream resources and do not authorize network access.
